//! 日次バッチの入口（技術検証版）。
//!
//! 使い方:
//!   batch-cli migrate                                  D1 にスキーマを適用
//!   batch-cli ingest --codes 7203,8058 [options]       取得 → 指標計算 → 保存
//!
//! ingest のオプション:
//!   --dry-run <file>     D1 に書かず、SQL をファイルに出力
//!   --shard <i>/<n>      銘柄を n 分割した i 番目だけ処理（GitHub Actions の matrix 用）
//!   --concurrency <n>    同時に処理する銘柄数（既定 8）
//!   --edinet-days <n>    有報を探す日数（既定 400）
//!
//! 環境変数（値はリポジトリに書かない）:
//!   EDINET_API_KEY                                       未設定なら財務はスキップ
//!   CLOUDFLARE_ACCOUNT_ID, D1_DATABASE_ID, CLOUDFLARE_API_TOKEN   --dry-run 以外で必須

use adapter_batch::{d1, edinet::EdinetClient, http, yahoo::YahooClient};
use domain::{Date, StockCode};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Semaphore;
use usecase::{ingest_one, BatchRun, Clock, Outcome, SnapshotStore};

struct SystemClock;
impl Clock for SystemClock {
    fn now_unix(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }
}

#[derive(Debug)]
struct Args {
    cmd: String,
    codes: Vec<StockCode>,
    dry_run: Option<String>,
    shard: (usize, usize),
    concurrency: usize,
    edinet_days: i64,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or("command required: migrate | ingest")?;
    let mut a = Args {
        cmd,
        codes: vec![],
        dry_run: None,
        shard: (0, 1),
        concurrency: 8,
        edinet_days: 400,
    };
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--codes" => {
                a.codes = val()?
                    .split(',')
                    .map(|s| StockCode::parse(s).ok_or(format!("bad code: {s}")))
                    .collect::<Result<_, _>>()?
            }
            "--dry-run" => a.dry_run = Some(val()?),
            "--shard" => {
                let v = val()?;
                let (i, n) = v.split_once('/').ok_or("--shard i/n")?;
                a.shard = (
                    i.parse().map_err(|_| "bad shard")?,
                    n.parse().map_err(|_| "bad shard")?,
                );
                if a.shard.1 == 0 || a.shard.0 >= a.shard.1 {
                    return Err("shard out of range".into());
                }
            }
            "--concurrency" => a.concurrency = val()?.parse().map_err(|_| "bad concurrency")?,
            "--edinet-days" => a.edinet_days = val()?.parse().map_err(|_| "bad edinet-days")?,
            other => return Err(format!("unknown flag: {other}")),
        }
    }
    Ok(a)
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or(format!("{name} is not set"))
}

fn d1_store(client: http::Client) -> Result<d1::D1Store, String> {
    Ok(d1::D1Store::new(
        client,
        &env("CLOUDFLARE_ACCOUNT_ID")?,
        &env("D1_DATABASE_ID")?,
        env("CLOUDFLARE_API_TOKEN")?,
    ))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let code = match args.cmd.as_str() {
        "migrate" => migrate().await,
        "ingest" => tokio::task::LocalSet::new().run_until(ingest(args)).await,
        other => Err(format!("unknown command: {other}")),
    };
    if let Err(e) = code {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn migrate() -> Result<(), String> {
    let store = d1_store(http::client())?;
    let script = include_str!("../../../migrations/0001_init.sql");
    let n = store.exec_script(script).await.map_err(|e| e.0)?;
    println!("migrate: applied {n} statements");
    Ok(())
}

async fn ingest(args: Args) -> Result<(), String> {
    if args.codes.is_empty() {
        return Err("--codes is required in the PoC".into());
    }
    let clock = SystemClock;
    let started = clock.now_unix();
    let run_id = format!("{}-{}of{}", started, args.shard.0, args.shard.1);
    let codes: Vec<StockCode> = args
        .codes
        .iter()
        .enumerate()
        .filter(|(i, _)| i % args.shard.1 == args.shard.0)
        .map(|(_, c)| c.clone())
        .collect();

    let client = http::client();
    let prices = Rc::new(YahooClient::new(client.clone()));

    // EDINET: 対象銘柄の最新の有報を索引化（4並列で日ごとの一覧を取得）
    let mut edinet = match env("EDINET_API_KEY") {
        Ok(key) => Some(EdinetClient::new(client.clone(), key)),
        Err(_) => {
            println!("warn: EDINET_API_KEY is not set; financials are skipped");
            None
        }
    };
    if let Some(ed) = edinet.as_mut() {
        let today = Date::from_unix_jst(started);
        let ed_rc = Rc::new(std::mem::replace(
            ed,
            EdinetClient::new(client.clone(), String::new()),
        ));
        let sem = Rc::new(Semaphore::new(4));
        let mut tasks = vec![];
        for i in 0..args.edinet_days {
            let (ed, sem) = (ed_rc.clone(), sem.clone());
            tasks.push(tokio::task::spawn_local(async move {
                let _p = sem.acquire().await.ok()?;
                ed.list_day(today.add_days(-i)).await.ok()
            }));
        }
        let mut lists = vec![];
        let mut failed_days = 0;
        for t in tasks {
            match t.await.ok().flatten() {
                Some(l) => lists.push(l),
                None => failed_days += 1,
            }
        }
        let mut owned = Rc::try_unwrap(ed_rc).map_err(|_| "edinet client still shared")?;
        for l in lists {
            owned.add_to_index(l, &codes);
        }
        println!(
            "edinet: indexed {} annual reports ({} days failed)",
            owned.indexed(),
            failed_days
        );
        *ed = owned;
    }

    let fins = Rc::new(edinet.map(OptionalFins::Some).unwrap_or(OptionalFins::None));
    let file_store = args
        .dry_run
        .as_ref()
        .map(|_| Rc::new(d1::SqlFileStore::default()));
    let d1 = match &args.dry_run {
        Some(_) => None,
        None => Some(Rc::new(d1_store(client.clone())?)),
    };
    let store: Rc<Store> = Rc::new(match (&file_store, &d1) {
        (Some(f), _) => Store::File(f.clone()),
        (_, Some(d)) => Store::D1(d.clone()),
        _ => unreachable!(),
    });

    let mut run = BatchRun {
        run_id: run_id.clone(),
        started_at: started,
        finished_at: None,
        target: codes.len(),
        succeeded: 0,
        failed: 0,
        status: "running",
    };
    store.save_run(&run).await.map_err(|e| e.0)?;

    // 銘柄ごとに並列処理（同時実行数は Semaphore で制限）
    let sem = Rc::new(Semaphore::new(args.concurrency.max(1)));
    let mut tasks = vec![];
    for code in codes {
        let (prices, fins, store, sem, run_id) = (
            prices.clone(),
            fins.clone(),
            store.clone(),
            sem.clone(),
            run_id.clone(),
        );
        tasks.push(tokio::task::spawn_local(async move {
            let _p = sem.acquire().await.expect("semaphore");
            let out = ingest_one(
                &code,
                &run_id,
                prices.as_ref(),
                fins.as_ref(),
                store.as_ref(),
                &SystemClock,
            )
            .await;
            (code, out)
        }));
    }
    for t in tasks {
        let (code, out) = t.await.map_err(|e| e.to_string())?;
        match out {
            Outcome::Saved { warning } => {
                run.succeeded += 1;
                match warning {
                    Some(w) => println!("ok   {} (warn: {w})", code.as_str()),
                    None => println!("ok   {}", code.as_str()),
                }
            }
            Outcome::Failed(e) => {
                run.failed += 1;
                println!("fail {} ({})", code.as_str(), e);
            }
        }
    }
    run.finished_at = Some(clock.now_unix());
    run.status = if run.failed == 0 {
        "succeeded"
    } else if run.succeeded > 0 {
        "partial"
    } else {
        "failed"
    };
    store.save_run(&run).await.map_err(|e| e.0)?;
    println!(
        "run {}: {} ok, {} failed, {}s",
        run.run_id,
        run.succeeded,
        run.failed,
        run.finished_at.unwrap() - started
    );

    if let (Some(path), Some(f)) = (&args.dry_run, file_store) {
        drop(store);
        let f = Rc::try_unwrap(f).map_err(|_| "store still shared")?;
        std::fs::write(path, f.into_string()).map_err(|e| e.to_string())?;
        println!("dry-run: wrote {path}");
    }
    if run.succeeded == 0 {
        return Err("all stocks failed".into());
    }
    Ok(())
}

/// EDINET キーが無い場合でも同じ型で扱うための包み
enum OptionalFins {
    Some(EdinetClient),
    None,
}
impl usecase::FinancialSource for OptionalFins {
    async fn fetch_financials(
        &self,
        code: &StockCode,
    ) -> Result<Option<domain::FinancialData>, usecase::PortError> {
        match self {
            OptionalFins::Some(c) => c.fetch_financials(code).await,
            OptionalFins::None => Ok(None),
        }
    }
}

enum Store {
    File(Rc<d1::SqlFileStore>),
    D1(Rc<d1::D1Store>),
}
impl SnapshotStore for Store {
    async fn save_run(&self, run: &BatchRun) -> Result<(), usecase::PortError> {
        match self {
            Store::File(s) => s.save_run(run).await,
            Store::D1(s) => s.save_run(run).await,
        }
    }
    async fn save_snapshot(&self, snap: &usecase::Snapshot) -> Result<(), usecase::PortError> {
        match self {
            Store::File(s) => s.save_snapshot(snap).await,
            Store::D1(s) => s.save_snapshot(snap).await,
        }
    }
}
