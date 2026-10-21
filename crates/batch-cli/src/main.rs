//! 日次バッチの入口。
//!
//! 使い方:
//!   batch-cli migrate                     D1 にスキーマを適用
//!   batch-cli sync-master [--dry-run <file>] [--from-file <xlsx>]
//!                                         JPX の上場銘柄一覧 → stocks（名称・種類・業種・規模）
//!   batch-cli sync-jq [options]           J-Quants から株価（終値）と財務（本決算）を日付ごとに取り込む。
//!                                         前回の続きから今日まで。初回は過去5年分（中断しても続きから再開）
//!   batch-cli compute [--label daily]     保存済みの株価と財務から、全銘柄の指標を計算して保存
//!   batch-cli should-run [--label daily] [--force]
//!                                         今日バッチを動かすか（休場日・当日成功済みなら false）。
//!                                         GITHUB_OUTPUT があれば run=true|false を書く
//!
//! sync-jq のオプション:
//!   --from <YYYY-MM-DD>     初回の開始日（既定: 5年前）。2回目以降は前回の続きから
//!   --until <YYYY-MM-DD>    終了日（既定: 今日）
//!   --rate <n>              1分あたりの呼び出し数（既定 60 = Light。Free は 5）
//!   --max-minutes <n>       この時間を過ぎたら、そこまでを保存して終わる（既定 300）
//!   --persist-every <n>     何日分ごとに D1 へ保存するか（既定 120）
//!   --dry-run <file>        D1 を読み書きせず、取得結果を SQL ファイルに書く（--from と --until が必要）
//!
//! 環境変数（値はリポジトリに書かない）:
//!   JQUANTS_API_KEY                                          sync-jq で必須
//!   CLOUDFLARE_ACCOUNT_ID, D1_DATABASE_ID, CLOUDFLARE_API_TOKEN  D1 を使うコマンドで必須

use adapter_batch::{d1, http, jpx, jquants::JQuantsClient};
use domain::{Date, PriceBar, StockCode};
use std::collections::{HashMap, HashSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use usecase::fins::{AnnualReport, FinancialStore, FinancialSummarySource};
use usecase::{BatchRun, Clock, SnapshotStore};

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
    dry_run: Option<String>,
    from_file: Option<String>,
    label: String,
    force: bool,
    from: Option<Date>,
    until: Option<Date>,
    rate: u32,
    max_minutes: u64,
    persist_every: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it
        .next()
        .ok_or("command required: migrate | sync-master | sync-jq | compute | should-run")?;
    let mut a = Args {
        cmd,
        dry_run: None,
        from_file: None,
        label: "manual".into(),
        force: false,
        from: None,
        until: None,
        rate: 60,
        max_minutes: 300,
        persist_every: 120,
    };
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        let date = |s: String| Date::parse(&s).ok_or(format!("bad date: {s}"));
        match flag.as_str() {
            "--dry-run" => a.dry_run = Some(val()?),
            "--from-file" => a.from_file = Some(val()?),
            "--force" => a.force = true,
            "--from" => a.from = Some(date(val()?)?),
            "--until" => a.until = Some(date(val()?)?),
            "--rate" => a.rate = val()?.parse().map_err(|_| "bad rate")?,
            "--max-minutes" => a.max_minutes = val()?.parse().map_err(|_| "bad max-minutes")?,
            "--persist-every" => {
                a.persist_every = val()?.parse().map_err(|_| "bad persist-every")?
            }
            "--label" => {
                a.label = val()?;
                if !a.label.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return Err("--label must be alphanumeric".into());
                }
            }
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
        "sync-master" => sync_master(args).await,
        "sync-jq" => sync_jq(args).await,
        "compute" => compute(args).await,
        "should-run" => should_run(args).await,
        other => Err(format!("unknown command: {other}")),
    };
    if let Err(e) = code {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init",
        include_str!("../../../migrations/0001_init.sql"),
    ),
    (
        "0002_users",
        include_str!("../../../migrations/0002_users.sql"),
    ),
    (
        "0003_stocks",
        include_str!("../../../migrations/0003_stocks.sql"),
    ),
    (
        "0004_jquants",
        include_str!("../../../migrations/0004_jquants.sql"),
    ),
];

async fn sync_master(args: Args) -> Result<(), String> {
    let client = http::client();
    let source = jpx::JpxSource {
        client: client.clone(),
        from_file: args.from_file.clone(),
    };
    let n = match &args.dry_run {
        Some(path) => {
            let store = d1::SqlFileStore::default();
            let n = usecase::master::sync_master(&source, &store, &SystemClock)
                .await
                .map_err(|e| e.0)?;
            std::fs::write(path, store.into_string()).map_err(|e| e.to_string())?;
            println!("dry-run: wrote {path}");
            n
        }
        None => usecase::master::sync_master(&source, &d1_store(client)?, &SystemClock)
            .await
            .map_err(|e| e.0)?,
    };
    println!("sync-master: {n} stocks");
    Ok(())
}

async fn migrate() -> Result<(), String> {
    let store = d1_store(http::client())?;
    // すべて CREATE ... IF NOT EXISTS で書くため、毎回番号順に全部流してよい
    for (name, script) in MIGRATIONS {
        let n = store
            .exec_script(script)
            .await
            .map_err(|e| format!("{name}: {}", e.0))?;
        println!("migrate: {name} applied {n} statements");
    }
    Ok(())
}

/// 内閣府「国民の祝日・休日」（公式・無料）
const HOLIDAYS_URL: &str = "https://www8.cao.go.jp/chosei/shukujitsu/syukujitsu.csv";

async fn should_run(args: Args) -> Result<(), String> {
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    let client = http::client();
    let (run, reason) = if args.force {
        (true, "forced".to_string())
    } else {
        let csv = http::get_bytes(&client, HOLIDAYS_URL)
            .await
            .map_err(|e| format!("holidays: {}", e.0))?;
        let holidays = domain::calendar::parse_holiday_csv(&csv);
        if holidays.is_empty() {
            return Err("holidays: no dates parsed".into());
        }
        if !domain::calendar::is_trading_day(today, &holidays) {
            (false, format!("{today} is not a trading day"))
        } else {
            // 今日の 0:00（日本時間）以降に、同じラベルで成功した実行があれば重ねて動かさない
            let since = today.to_days() * 86_400 - 9 * 3600;
            let done = d1_store(client)?
                .succeeded_since(&args.label, since)
                .await
                .map_err(|e| e.0)?;
            if done {
                (
                    false,
                    format!("{} batch already succeeded on {today}", args.label),
                )
            } else {
                (true, format!("{today} is a trading day"))
            }
        }
    };
    println!("should-run: {run} ({reason})");
    if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        writeln!(f, "run={run}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// sync-jq がどの日付まで取り込んだか
const CURSOR_KEY: &str = "jq_synced_until";
/// 初回に取る期間（Light プランで取れる5年）
const BACKFILL_DAYS: i64 = 5 * 365;
/// 夕方以降の開示を取りこぼさないよう、財務は直近この日数を毎回取り直す
const REFETCH_FIN_DAYS: i64 = 3;

async fn sync_jq(args: Args) -> Result<(), String> {
    if let Some(path) = &args.dry_run {
        return sync_jq_dry_run(&args, path).await;
    }
    let started = Instant::now();
    let client = http::client();
    let jq = JQuantsClient::new(client.clone(), env("JQUANTS_API_KEY")?, args.rate);
    let store = d1_store(client)?;
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    let until = args.until.unwrap_or(today);

    let cursor = store
        .get_state(CURSOR_KEY)
        .await
        .map_err(|e| e.0)?
        .and_then(|s| Date::parse(&s));
    let start = match (cursor, args.from) {
        (Some(c), _) => c.add_days(1),
        (None, Some(f)) => f,
        (None, None) => today.add_days(-BACKFILL_DAYS),
    };
    let fin_start = start.min(until.add_days(-REFETCH_FIN_DAYS));
    let days = usecase::fins::weekdays(fin_start, until);
    println!(
        "sync-jq: prices {start}..{until}, financials {fin_start}..{until} ({} weekdays)",
        days.len()
    );
    if days.is_empty() {
        return Ok(());
    }

    let mut histories = store.load_price_histories().await.map_err(|e| e.0)?;
    println!("sync-jq: loaded {} price histories", histories.len());
    let mut changed: HashSet<String> = HashSet::new();
    let mut reports: Vec<AnnualReport> = vec![];
    let mut done_until: Option<Date> = None;
    let mut since_persist = 0;
    let deadline = std::time::Duration::from_secs(args.max_minutes * 60);

    for (i, &day) in days.iter().enumerate() {
        if day >= start {
            let closes = jq
                .closes_on(day)
                .await
                .map_err(|e| format!("{day} prices: {}", e.0))?;
            let n = closes.len();
            let mut by_code: HashMap<String, Vec<PriceBar>> = HashMap::new();
            for (code, bar) in closes {
                by_code
                    .entry(code.as_str().to_string())
                    .or_default()
                    .push(bar);
            }
            for (code, bars) in by_code {
                domain::price_history::merge(histories.entry(code.clone()).or_default(), &bars);
                changed.insert(code);
            }
            if n > 0 {
                println!("{day}: {n} closes");
            }
        }
        let fins = jq
            .annual_reports_on(day)
            .await
            .map_err(|e| format!("{day} financials: {}", e.0))?;
        if !fins.is_empty() {
            println!("{day}: {} annual reports", fins.len());
        }
        reports.extend(fins);
        if day >= start {
            done_until = Some(day);
        }
        since_persist += 1;

        let last = i + 1 == days.len();
        let timed_out = started.elapsed() > deadline;
        if last || timed_out || since_persist >= args.persist_every {
            persist(
                &store,
                &histories,
                &mut changed,
                &mut reports,
                done_until,
                now,
            )
            .await?;
            since_persist = 0;
            if timed_out && !last {
                println!("sync-jq: time limit reached; will resume after {done_until:?}");
                return Ok(());
            }
        }
    }
    // 休場日などで最後の日が平日でなくても、until まで進めておく
    store
        .set_state(
            CURSOR_KEY,
            &until.max(done_until.unwrap_or(until)).to_string(),
            now,
        )
        .await
        .map_err(|e| e.0)?;
    println!("sync-jq: done in {}s", started.elapsed().as_secs());
    Ok(())
}

/// D1 に触れずに、指定期間の取得結果を SQL にする（取得元の形の確認用）
async fn sync_jq_dry_run(args: &Args, path: &str) -> Result<(), String> {
    let (Some(from), Some(until)) = (args.from, args.until) else {
        return Err("--dry-run needs --from and --until".into());
    };
    let jq = JQuantsClient::new(http::client(), env("JQUANTS_API_KEY")?, args.rate);
    let file = d1::SqlFileStore::default();
    let mut histories: HashMap<String, Vec<PriceBar>> = HashMap::new();
    let mut reports = vec![];
    for day in usecase::fins::weekdays(from, until) {
        let closes = jq.closes_on(day).await.map_err(|e| e.0)?;
        let fins = jq.annual_reports_on(day).await.map_err(|e| e.0)?;
        println!(
            "{day}: {} closes, {} annual reports",
            closes.len(),
            fins.len()
        );
        for (code, bar) in closes {
            domain::price_history::merge(
                histories.entry(code.as_str().to_string()).or_default(),
                &[bar],
            );
        }
        reports.extend(fins);
    }
    let rows: Vec<(String, String, String)> = histories
        .iter()
        .filter_map(|(c, b)| {
            Some((
                c.clone(),
                domain::price_history::encode(b),
                b.last()?.date.to_string(),
            ))
        })
        .collect();
    file.save_annual_reports(&reports, 0)
        .await
        .map_err(|e| e.0)?;
    let mut sql = file.into_string();
    for s in adapter_batch::sql::price_history_stmts(&rows, 0) {
        sql.push_str(&s.render());
        sql.push('\n');
    }
    std::fs::write(path, sql).map_err(|e| e.to_string())?;
    println!(
        "dry-run: {} codes, {} annual reports -> {path}",
        rows.len(),
        reports.len()
    );
    Ok(())
}

async fn persist(
    store: &d1::D1Store,
    histories: &HashMap<String, Vec<PriceBar>>,
    changed: &mut HashSet<String>,
    reports: &mut Vec<AnnualReport>,
    done_until: Option<Date>,
    now: i64,
) -> Result<(), String> {
    let rows: Vec<(String, String, String)> = changed
        .iter()
        .filter_map(|code| {
            let bars = histories.get(code)?;
            let last = bars.last()?;
            Some((
                code.clone(),
                domain::price_history::encode(bars),
                last.date.to_string(),
            ))
        })
        .collect();
    store
        .save_price_histories(&rows, now)
        .await
        .map_err(|e| e.0)?;
    store
        .save_annual_reports(reports, now)
        .await
        .map_err(|e| e.0)?;
    println!(
        "persist: {} price histories, {} annual reports",
        rows.len(),
        reports.len()
    );
    changed.clear();
    reports.clear();
    if let Some(d) = done_until {
        store
            .set_state(CURSOR_KEY, &d.to_string(), now)
            .await
            .map_err(|e| e.0)?;
    }
    Ok(())
}

async fn compute(args: Args) -> Result<(), String> {
    let store = d1_store(http::client())?;
    let started = SystemClock.now_unix();
    let run_id = format!("{}-{}", args.label, started);
    let codes: Vec<StockCode> = store
        .listed_codes(&["stock", "reit", "etf"])
        .await
        .map_err(|e| e.0)?
        .iter()
        .filter_map(|c| StockCode::parse(c))
        .collect();
    let histories = store.load_price_histories().await.map_err(|e| e.0)?;
    let annuals = store.load_annuals().await.map_err(|e| e.0)?;
    println!(
        "compute: {} codes, {} price histories, {} with financials",
        codes.len(),
        histories.len(),
        annuals.len()
    );
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
    let (snaps, skipped) =
        usecase::daily::compute_all(&codes, histories, &annuals, &run_id, started);
    store.save_snapshots(&snaps).await.map_err(|e| e.0)?;
    run.succeeded = snaps.len();
    run.failed = skipped;
    run.finished_at = Some(SystemClock.now_unix());
    // 株価のない銘柄（上場直後など）があるのは普通なので、半分以上計算できれば成功とする
    run.status = if snaps.len() * 2 >= codes.len().max(1) {
        "succeeded"
    } else if !snaps.is_empty() {
        "partial"
    } else {
        "failed"
    };
    store.save_run(&run).await.map_err(|e| e.0)?;
    println!(
        "compute: {} saved, {} without prices, status {}",
        snaps.len(),
        skipped,
        run.status
    );
    if run.status == "failed" {
        return Err("no indicators computed".into());
    }
    Ok(())
}
