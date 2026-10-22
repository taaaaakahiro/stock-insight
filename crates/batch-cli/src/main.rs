//! 日次バッチの入口。
//!
//! 使い方:
//!   batch-cli migrate                     D1 にスキーマを適用
//!   batch-cli sync-master [--dry-run <file>] [--from-file <xlsx>]
//!                                         JPX の上場銘柄一覧 → stocks（名称・種類・業種・規模）
//!   batch-cli sync-jq [options]           J-Quants から株価（終値）と財務（本決算）を日付ごとに取り込む。
//!                                         前回の続きから今日まで。初回は過去5年分（中断しても続きから再開）
//!   batch-cli sync-edinet [options]       EDINET から有報・半期報告書（銘柄・種類ごとに最新の1通）を取り、
//!                                         配当政策などの文章を R2 用に書き出し、配当方針の手がかりを D1 に保存する。
//!                                         前回の続きから今日まで。初回は過去400日分の書類一覧から
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
//!   --persist-every <n>     何日分ごとに D1 へ保存するか（既定 1500 = 基本は最後に1回。保存のたびに全銘柄の
//!                           株価の履歴を書き直すので、小さくすると D1 の1日の書き込み上限に当たる）
//!   --dry-run <file>        D1 を読み書きせず、取得結果を SQL ファイルに書く（--from と --until が必要）
//!   --raw-dir <dir>         J-Quants の応答を全項目そのまま gzip JSON で置く場所（既定 raw。あとで R2 に上げる）
//!
//! sync-edinet のオプション（--from / --until / --rate / --max-minutes / --raw-dir は sync-jq と同じ）:
//!   --dry-run <file>        D1 を読み書きせず、--from〜--until の一覧から --limit 件だけ取って SQL と文章を書く
//!   --limit <n>             --dry-run で取る書類の数（既定 5）
//!   R2 から消すファイルのキーは {raw-dir}/edinet-delete.txt に1行ずつ書く（ワークフローが消す）
//!
//! 環境変数（値はリポジトリに書かない）:
//!   JQUANTS_API_KEY                                          sync-jq で必須
//!   EDINET_API_KEY                                           sync-edinet で必須
//!   CLOUDFLARE_ACCOUNT_ID, D1_DATABASE_ID, CLOUDFLARE_API_TOKEN  D1 を使うコマンドで必須

use adapter_batch::{
    d1,
    edinet::EdinetClient,
    http, jpx,
    jquants::{self, JQuantsClient},
    raw,
};
use domain::{Date, PriceBar, StockCode};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use usecase::edinet::{DocRef, EdinetDoc};
use usecase::fins::{AnnualReport, FinancialStore, PeriodReport};
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
    raw_dir: String,
    limit: usize,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(
        "command required: migrate | sync-master | sync-jq | sync-edinet | compute | should-run",
    )?;
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
        persist_every: 1500,
        raw_dir: "raw".into(),
        limit: 5,
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
            "--raw-dir" => a.raw_dir = val()?,
            "--limit" => a.limit = val()?.parse().map_err(|_| "bad limit")?,
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
        "sync-edinet" => sync_edinet(args).await,
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
    (
        "0005_analysis",
        include_str!("../../../migrations/0005_analysis.sql"),
    ),
    (
        "0006_portfolio",
        include_str!("../../../migrations/0006_portfolio.sql"),
    ),
    (
        "0007_edinet",
        include_str!("../../../migrations/0007_edinet.sql"),
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

/// まだ D1 に書いていない取得結果
#[derive(Default)]
struct Pending {
    changed: HashSet<String>,
    annuals: Vec<AnnualReport>,
    periods: Vec<PeriodReport>,
}

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
    let raw_dir = PathBuf::from(&args.raw_dir);

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
    let mut pending = Pending::default();
    let mut done_until: Option<Date> = None;
    let mut since_persist = 0;
    let deadline = std::time::Duration::from_secs(args.max_minutes * 60);

    for (i, &day) in days.iter().enumerate() {
        if day >= start {
            let rows = jq
                .bars_on(day)
                .await
                .map_err(|e| format!("{day} prices: {}", e.0))?;
            raw::write_day(&raw_dir, "bars", day, &rows).map_err(|e| e.0)?;
            let closes = jquants::closes(&rows);
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
                pending.changed.insert(code);
            }
            if n > 0 {
                println!("{day}: {n} closes");
            }
        }
        let rows = jq
            .summaries_on(day)
            .await
            .map_err(|e| format!("{day} financials: {}", e.0))?;
        raw::write_day(&raw_dir, "fins", day, &rows).map_err(|e| e.0)?;
        let annuals = jquants::annual_reports(&rows);
        let periods = jquants::period_reports(&rows);
        if !periods.is_empty() {
            println!(
                "{day}: {} financial rows ({} annual)",
                periods.len(),
                annuals.len()
            );
        }
        pending.annuals.extend(annuals);
        pending.periods.extend(periods);
        if day >= start {
            done_until = Some(day);
        }
        since_persist += 1;

        let last = i + 1 == days.len();
        let timed_out = started.elapsed() > deadline;
        if last || timed_out || since_persist >= args.persist_every {
            persist(&store, &histories, &mut pending, done_until, now).await?;
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

/// D1 に触れずに、指定期間の取得結果を SQL と生データにする（取得元の形の確認用）
async fn sync_jq_dry_run(args: &Args, path: &str) -> Result<(), String> {
    let (Some(from), Some(until)) = (args.from, args.until) else {
        return Err("--dry-run needs --from and --until".into());
    };
    let jq = JQuantsClient::new(http::client(), env("JQUANTS_API_KEY")?, args.rate);
    let raw_dir = PathBuf::from(&args.raw_dir);
    let file = d1::SqlFileStore::default();
    let mut histories: HashMap<String, Vec<PriceBar>> = HashMap::new();
    let mut annuals = vec![];
    let mut periods = vec![];
    for day in usecase::fins::weekdays(from, until) {
        let bars = jq.bars_on(day).await.map_err(|e| e.0)?;
        raw::write_day(&raw_dir, "bars", day, &bars).map_err(|e| e.0)?;
        let fins = jq.summaries_on(day).await.map_err(|e| e.0)?;
        raw::write_day(&raw_dir, "fins", day, &fins).map_err(|e| e.0)?;
        let closes = jquants::closes(&bars);
        let a = jquants::annual_reports(&fins);
        let p = jquants::period_reports(&fins);
        println!(
            "{day}: {} closes, {} financial rows ({} annual)",
            closes.len(),
            p.len(),
            a.len()
        );
        for (code, bar) in closes {
            domain::price_history::merge(
                histories.entry(code.as_str().to_string()).or_default(),
                &[bar],
            );
        }
        annuals.extend(a);
        periods.extend(p);
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
    file.save_annual_reports(&annuals, 0)
        .await
        .map_err(|e| e.0)?;
    file.save_period_reports(&periods, 0)
        .await
        .map_err(|e| e.0)?;
    let mut sql = file.into_string();
    for s in adapter_batch::sql::price_history_stmts(&rows, 0) {
        sql.push_str(&s.render());
        sql.push('\n');
    }
    std::fs::write(path, sql).map_err(|e| e.to_string())?;
    println!(
        "dry-run: {} codes, {} annual reports, {} period rows -> {path} (raw in {})",
        rows.len(),
        annuals.len(),
        periods.len(),
        args.raw_dir
    );
    Ok(())
}

async fn persist(
    store: &d1::D1Store,
    histories: &HashMap<String, Vec<PriceBar>>,
    pending: &mut Pending,
    done_until: Option<Date>,
    now: i64,
) -> Result<(), String> {
    let rows: Vec<(String, String, String)> = pending
        .changed
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
        .save_annual_reports(&pending.annuals, now)
        .await
        .map_err(|e| e.0)?;
    store
        .save_period_reports(&pending.periods, now)
        .await
        .map_err(|e| e.0)?;
    println!(
        "persist: {} price histories, {} annual reports, {} quarterly rows",
        rows.len(),
        pending.annuals.len(),
        pending.periods.len()
    );
    *pending = Pending::default();
    if let Some(d) = done_until {
        store
            .set_state(CURSOR_KEY, &d.to_string(), now)
            .await
            .map_err(|e| e.0)?;
    }
    Ok(())
}

/// sync-edinet がどの日付までの書類一覧を読んだか
const EDINET_CURSOR_KEY: &str = "edinet_listed_until";
/// 初回に読む書類一覧の期間。有報は年1回なので、提出の遅い会社も含めて1年強
const EDINET_BACKFILL_DAYS: i64 = 400;
/// 何件ごとに D1 へ保存するか
const EDINET_PERSIST_EVERY: usize = 20;
/// R2 から消すファイルのキー（ワークフローが読んで消す）
const EDINET_DELETE_FILE: &str = "edinet-delete.txt";

async fn sync_edinet(args: Args) -> Result<(), String> {
    if let Some(path) = &args.dry_run {
        return sync_edinet_dry_run(&args, path).await;
    }
    let started = Instant::now();
    let deadline = std::time::Duration::from_secs(args.max_minutes * 60);
    let client = http::client();
    let ed = EdinetClient::new(client.clone(), env("EDINET_API_KEY")?, args.rate);
    let store = d1_store(client)?;
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    let until = args.until.unwrap_or(today);
    let raw_dir = PathBuf::from(&args.raw_dir);

    let cursor = store
        .get_state(EDINET_CURSOR_KEY)
        .await
        .map_err(|e| e.0)?
        .and_then(|s| Date::parse(&s));
    let start = match (cursor, args.from) {
        (Some(c), _) => c.add_days(1),
        (None, Some(f)) => f,
        (None, None) => today.add_days(-EDINET_BACKFILL_DAYS),
    };
    let days = usecase::fins::weekdays(start, until);
    println!(
        "sync-edinet: listing {start}..{until} ({} weekdays)",
        days.len()
    );

    // 1. 書類一覧（1日1回の呼び出し）。時間切れなら読めたところまでで先に進む
    let mut listed = vec![];
    let mut listed_until = until;
    for &day in &days {
        if started.elapsed() > deadline {
            listed_until = day.add_days(-1);
            println!("sync-edinet: time limit reached while listing; listed until {listed_until}");
            break;
        }
        let docs = ed
            .list_day(day)
            .await
            .map_err(|e| format!("{day} list: {}", e.0))?;
        if !docs.is_empty() {
            println!("{day}: {} reports", docs.len());
        }
        listed.extend(docs);
    }

    // 2. 銘柄・種類ごとに最新の1通で、まだ持っていないものだけを取る
    let current: HashSet<String> = store
        .current_codes(&["stock", "reit"])
        .await
        .map_err(|e| e.0)?
        .into_iter()
        .collect();
    if current.is_empty() {
        return Err("no listed codes; run sync-master first".into());
    }
    let stored = store.load_edinet_docs().await.map_err(|e| e.0)?;
    let n_listed = listed.len();
    let plan = usecase::edinet::plan(listed, &stored, &current);
    println!(
        "sync-edinet: {n_listed} listed, {} stored, {} to fetch, {} to delete",
        stored.len(),
        plan.fetch.len(),
        plan.delete.len()
    );

    // 3. 古くなった書類を消す（D1 の行はここで、R2 のファイルはワークフローの最後に）
    store
        .delete_edinet_docs(&plan.delete)
        .await
        .map_err(|e| e.0)?;
    let keys: Vec<String> = plan
        .delete
        .iter()
        .map(|(code, kind)| usecase::edinet::r2_key(code, *kind))
        .collect();
    append_lines(&raw_dir.join(EDINET_DELETE_FILE), &keys)?;

    // 4. 書類を取る。止まったら、まだ取っていない最初の書類の前日から次回やり直す
    let mut pending = vec![];
    let mut fetched = 0;
    let mut error = None;
    for doc in &plan.fetch {
        if started.elapsed() > deadline {
            println!(
                "sync-edinet: time limit reached; will resume from {}",
                doc.submitted
            );
            break;
        }
        match fetch_doc(&ed, &raw_dir, doc).await {
            Ok(e) => pending.push(e),
            Err(e) => {
                error = Some(format!("{} {}: {e}", doc.code, doc.doc_id));
                break;
            }
        }
        fetched += 1;
        if pending.len() >= EDINET_PERSIST_EVERY {
            store
                .save_edinet_docs(&pending, now)
                .await
                .map_err(|e| e.0)?;
            pending.clear();
        }
    }
    store
        .save_edinet_docs(&pending, now)
        .await
        .map_err(|e| e.0)?;
    let next = usecase::edinet::resume_cursor(listed_until, plan.fetch.get(fetched));
    store
        .set_state(EDINET_CURSOR_KEY, &next.to_string(), now)
        .await
        .map_err(|e| e.0)?;
    println!(
        "sync-edinet: {fetched} fetched, listed until {next}, {}s",
        started.elapsed().as_secs()
    );
    error.map_or(Ok(()), Err)
}

/// 1通を取り、文章を R2 用のファイルに書き、配当方針の手がかりを返す
async fn fetch_doc(
    ed: &EdinetClient,
    raw_dir: &std::path::Path,
    doc: &DocRef,
) -> Result<EdinetDoc, String> {
    let sections = ed.fetch_sections(&doc.doc_id).await.map_err(|e| e.0)?;
    let refs: Vec<(domain::disclosure::Section, &str)> =
        sections.iter().map(|(s, t)| (*s, t.as_str())).collect();
    let signals = domain::disclosure::detect(&refs);
    let body = serde_json::json!({
        "source": "EDINET",
        "code": doc.code,
        "kind": doc.kind.as_str(),
        "doc_id": doc.doc_id,
        "submitted": doc.submitted.to_string(),
        "period_end": doc.period_end.map(|d| d.to_string()),
        "sections": sections.iter().map(|(s, t)| (s.key().to_string(), serde_json::Value::from(t.as_str()))).collect::<serde_json::Map<_, _>>(),
        "signals": {
            "progressive_dividend": signals.progressive,
            "no_dividend_cut": signals.no_cut,
            "dividend_raise": signals.raise,
            "doe_target": signals.doe_target,
            "payout_target": signals.payout_target,
            "phrases": signals.phrases,
        },
    });
    raw::write_object(
        raw_dir,
        &usecase::edinet::r2_key(&doc.code, doc.kind),
        &body,
    )
    .map_err(|e| e.0)?;
    let flags = [
        (signals.progressive, "累進配当"),
        (signals.no_cut, "非減配"),
        (signals.raise, "増配"),
        (signals.doe_target, "DOE"),
        (signals.payout_target.is_some(), "配当性向目標"),
    ];
    let on: Vec<&str> = flags.iter().filter(|(b, _)| *b).map(|(_, l)| *l).collect();
    println!(
        "{} {} {} ({} sections){}",
        doc.submitted,
        doc.code,
        doc.kind.as_str(),
        sections.len(),
        if on.is_empty() {
            String::new()
        } else {
            format!(": {}", on.join(", "))
        }
    );
    Ok(EdinetDoc {
        doc: doc.clone(),
        signals,
    })
}

fn append_lines(path: &std::path::Path, lines: &[String]) -> Result<(), String> {
    use std::io::Write;
    if lines.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    for l in lines {
        writeln!(f, "{l}").map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// D1 に触れずに、指定期間の一覧から数件だけ取る（API キーと取得元の形の確認用）
async fn sync_edinet_dry_run(args: &Args, path: &str) -> Result<(), String> {
    let (Some(from), Some(until)) = (args.from, args.until) else {
        return Err("--dry-run needs --from and --until".into());
    };
    let ed = EdinetClient::new(http::client(), env("EDINET_API_KEY")?, args.rate);
    let raw_dir = PathBuf::from(&args.raw_dir);
    let mut listed = vec![];
    for day in usecase::fins::weekdays(from, until) {
        let docs = ed
            .list_day(day)
            .await
            .map_err(|e| format!("{day} list: {}", e.0))?;
        println!("{day}: {} reports", docs.len());
        listed.extend(docs);
    }
    let all: HashSet<String> = listed.iter().map(|d| d.code.clone()).collect();
    let plan = usecase::edinet::plan(listed, &HashMap::new(), &all);
    let mut docs = vec![];
    for doc in plan.fetch.iter().take(args.limit) {
        docs.push(fetch_doc(&ed, &raw_dir, doc).await?);
    }
    let sql: String = adapter_batch::sql::edinet_docs_stmts(&docs, 0)
        .iter()
        .map(|s| s.render() + "\n")
        .collect();
    std::fs::write(path, sql).map_err(|e| e.to_string())?;
    println!(
        "dry-run: {} of {} reports -> {path} (texts in {}/edinet)",
        docs.len(),
        plan.fetch.len(),
        args.raw_dir
    );
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
    let quarterlies = store.load_quarterlies().await.map_err(|e| e.0)?;
    println!(
        "compute: {} codes, {} price histories, {} with annual financials, {} with quarterly rows",
        codes.len(),
        histories.len(),
        annuals.len(),
        quarterlies.len()
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
        usecase::daily::compute_all(&codes, histories, &annuals, &quarterlies, &run_id, started);
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
