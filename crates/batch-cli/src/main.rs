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
//!   batch-cli sync-reit [options]         EDINET の REIT の有報から借入金と物件を読み、要約・明細・決算ごとの推移を D1 に保存する。
//!                                         前回の続きから今日まで。初回は過去400日分の書類一覧から
//!   batch-cli reit-probe [options]        EDINET の REIT の有報から借入金明細と物件一覧を読み、BS・表の合計と
//!                                         突き合わせて読み取りの精度を出す（D1 には触れない）
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
//!   --refill-prices         取り込み済みの期間の株価（終値）も取り直して履歴を埋める（財務と進み具合はそのまま）。
//!                           範囲は --from から（既定: 5年前）。新しい日の生データだけを --raw-dir に書く
//!   --refill-forecasts      取り込み済みの期間の決算短信を読み直し、会社予想の配当（dividend_forecasts）だけを
//!                           埋める（年次・四半期の財務、進み具合、R2 の生データはそのまま）。範囲は --from から
//!                           （既定: 1年前）
//!
//! sync-edinet のオプション（--from / --until / --rate / --max-minutes / --raw-dir は sync-jq と同じ）:
//!   --dry-run <file>        D1 を読み書きせず、--from〜--until の一覧から --limit 件だけ取って SQL と文章を書く
//!   --limit <n>             --dry-run で取る書類の数（既定 5）
//!   --refetch-doe           新しい書類のあと、保存済みで方針の数字が欠けている書類（DOE に触れているのに下限・
//!                           目標がない、配当性向の目標があるのに下限・目標に分けていない）を取り直す
//!                           （数字を拾う仕組みより前に取り込んだ分。文章も R2 用に書き直す）
//!   --refetch-business      新しい書類のあと、保存済みで「事業の内容」の文をまだ持っていない有報を取り直す
//!                           （注目テーマの照合を始める前に取り込んだ分。数千通あるので数回に分かれる）
//!   R2 から消すファイルのキーは {raw-dir}/edinet-delete.txt に1行ずつ書く（ワークフローが消す）
//!
//! sync-reit のオプション（--from / --until / --rate / --max-minutes / --raw-dir は sync-jq と同じ）:
//!   --dry-run <file>        D1 を読み書きせず、--from〜--until の書類を取り込む SQL をファイルに書く
//!   {raw-dir}/reit/<docID>.zip があればそれを使う（reit-probe が置いたもの。新しくは置かない）
//!
//! reit-probe のオプション（--from / --until / --rate / --raw-dir は sync-jq と同じ）:
//!   --from <YYYY-MM-DD>     書類一覧を読む開始日（既定: 200日前。REIT の決算は半年ごとなので全銘柄が入る）
//!   --out <file>            読んだ結果の JSON（既定 {raw-dir}/reit-probe.json）
//!   書類一覧と書類の zip は {raw-dir}/reit/ に置き、次からはそれを使う
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
use usecase::fins::{AnnualReport, FinancialStore, ForecastReport, PeriodReport};
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
    refill_prices: bool,
    refetch_doe: bool,
    refetch_business: bool,
    refill_forecasts: bool,
    out: Option<String>,
    rag: adapter_batch::reit_rag::StageOptions,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or(
        "command required: migrate | sync-master | sync-jq | sync-edinet | sync-reit | reit-probe | compute | should-run",
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
        refill_prices: false,
        refetch_doe: false,
        refetch_business: false,
        refill_forecasts: false,
        out: None,
        rag: adapter_batch::reit_rag::StageOptions {
            stage: "all".into(),
            run_id: None,
            shard: 0,
            shards: 1,
        },
    };
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        let date = |s: String| Date::parse(&s).ok_or(format!("bad date: {s}"));
        match flag.as_str() {
            "--dry-run" => a.dry_run = Some(val()?),
            "--from-file" => a.from_file = Some(val()?),
            "--force" => a.force = true,
            "--refill-prices" => a.refill_prices = true,
            "--refetch-doe" => a.refetch_doe = true,
            "--refetch-business" => a.refetch_business = true,
            "--refill-forecasts" => a.refill_forecasts = true,
            "--from" => a.from = Some(date(val()?)?),
            "--until" => a.until = Some(date(val()?)?),
            "--rate" => a.rate = val()?.parse().map_err(|_| "bad rate")?,
            "--max-minutes" => a.max_minutes = val()?.parse().map_err(|_| "bad max-minutes")?,
            "--persist-every" => {
                a.persist_every = val()?.parse().map_err(|_| "bad persist-every")?
            }
            "--raw-dir" => a.raw_dir = val()?,
            "--limit" => a.limit = val()?.parse().map_err(|_| "bad limit")?,
            "--out" => a.out = Some(val()?),
            "--stage" => a.rag.stage = val()?,
            "--run-id" => a.rag.run_id = Some(val()?),
            "--shard" => a.rag.shard = val()?.parse().map_err(|_| "bad shard")?,
            "--shards" => a.rag.shards = val()?.parse().map_err(|_| "bad shards")?,
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
        "sync-reit" => sync_reit(args).await,
        "reit-rag" if args.dry_run.is_some() || args.from_file.is_some() || args.from.is_some() || args.until.is_some() => Err("reit-rag does not support dry-run/from-file/date overrides; use the offline RAG tests".into()),
        "reit-rag" => match d1_store(http::client()) {
            Ok(store) => {
                adapter_batch::reit_rag::run_stage(&store, SystemClock.now_unix(), args.max_minutes.min(120), &args.rag)
                    .await
                    .map_err(|e| e.0)
                    .and_then(|id| {
                        if args.rag.stage == "prepare" {
                            if let Ok(path) = std::env::var("GITHUB_OUTPUT") {
                                use std::io::Write;
                                let mut f=std::fs::OpenOptions::new().append(true).open(path).map_err(|e|e.to_string())?;
                                writeln!(f,"run_id={id}").map_err(|e|e.to_string())?;
                            }
                        }
                        println!("reit-rag: stage={} completed run={id}", args.rag.stage);
                        Ok(())
                    })
            }
            Err(e) => Err(e),
        },
        "reit-probe" => reit_probe(args).await,
        "reit-dump" => reit_dump(args),
        "compute" => compute(args).await,
        "evaluate-recommendations" => evaluate_recommendations().await,
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
    (
        "0008_recommendations",
        include_str!("../../../migrations/0008_recommendations.sql"),
    ),
    (
        "0009_splits",
        include_str!("../../../migrations/0009_splits.sql"),
    ),
    (
        "0010_dividend_forecast",
        include_str!("../../../migrations/0010_dividend_forecast.sql"),
    ),
    (
        "0011_reit",
        include_str!("../../../migrations/0011_reit.sql"),
    ),
    (
        "0012_dividend_cut",
        include_str!("../../../migrations/0012_dividend_cut.sql"),
    ),
    (
        "0013_doe_policy",
        include_str!("../../../migrations/0013_doe_policy.sql"),
    ),
    (
        "0014_payout_policy",
        include_str!("../../../migrations/0014_payout_policy.sql"),
    ),
    (
        "0015_quarter_quality",
        include_str!("../../../migrations/0015_quarter_quality.sql"),
    ),
    (
        "0016_recommendation_grade",
        include_str!("../../../migrations/0016_recommendation_grade.sql"),
    ),
    (
        "0017_themes",
        include_str!("../../../migrations/0017_themes.sql"),
    ),
    (
        "0018_recommendation_source",
        include_str!("../../../migrations/0018_recommendation_source.sql"),
    ),
    (
        "0021_recommendation_tracking",
        include_str!("../../../migrations/0021_recommendation_tracking.sql"),
    ),
    (
        "0022_reit_rag",
        include_str!("../../../migrations/0022_reit_rag.sql"),
    ),
    (
        "0023_reit_rag_facts",
        include_str!("../../../migrations/0023_reit_rag_facts.sql"),
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

async fn should_run(args: Args) -> Result<(), String> {
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    let client = http::client();
    let (run, reason) = if args.force {
        (true, "forced".to_string())
    } else {
        let csv = http::get_bytes(&client, http::HOLIDAYS_URL)
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
/// --refill-forecasts で読み直す既定の期間。決算短信は3か月ごとなので、1年あれば各社の最新の予想が入る
const FORECAST_REFILL_DAYS: i64 = 366;

/// まだ D1 に書いていない取得結果
#[derive(Default)]
struct Pending {
    changed: HashSet<String>,
    annuals: Vec<AnnualReport>,
    periods: Vec<PeriodReport>,
    /// 株式分割・併合（AdjFactor が 1 でない日）
    splits: Vec<(String, domain::split::Split)>,
    forecasts: Vec<ForecastReport>,
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
    // 取り込み済みの期間の株価を取り直す（履歴の保存期間を延ばしたときに、切り詰めた分を埋める）
    let refill_from = (args.refill_prices && cursor.is_some())
        .then(|| args.from.unwrap_or(today.add_days(-BACKFILL_DAYS)))
        .filter(|&r| r < start);
    // 予想の配当を取り始める前（dividend_forecasts を足す前）の決算短信から、予想だけを読み直す
    let forecast_from = (args.refill_forecasts && cursor.is_some())
        .then(|| args.from.unwrap_or(today.add_days(-FORECAST_REFILL_DAYS)))
        .filter(|&f| f < fin_start);
    let first = [refill_from, forecast_from]
        .into_iter()
        .flatten()
        .fold(fin_start, Date::min);
    let days = usecase::fins::weekdays(first, until);
    if let Some(f) = forecast_from {
        println!(
            "sync-jq: dividend forecasts {f}..{} (refill)",
            fin_start.add_days(-1)
        );
    }
    match refill_from {
        Some(r) => println!(
            "sync-jq: prices {start}..{until} (refill from {r}), financials {fin_start}..{until} ({} weekdays)",
            days.len()
        ),
        None => println!(
            "sync-jq: prices {start}..{until}, financials {fin_start}..{until} ({} weekdays)",
            days.len()
        ),
    }
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
        let refill = refill_from.is_some_and(|r| day >= r) && day < start;
        if day >= start || refill {
            let rows = jq
                .bars_on(day)
                .await
                .map_err(|e| format!("{day} prices: {}", e.0))?;
            // 取り直した日の生データは前に取り込んだときのものと同じなので、R2 には上げ直さない
            if !refill {
                raw::write_day(&raw_dir, "bars", day, &rows).map_err(|e| e.0)?;
            }
            for (code, s) in jquants::splits(&rows) {
                println!("{day}: split {} x{}", code.as_str(), s.factor);
                pending.splits.push((code.as_str().to_string(), s));
            }
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
        if day >= fin_start {
            let rows = jq
                .summaries_on(day)
                .await
                .map_err(|e| format!("{day} financials: {}", e.0))?;
            raw::write_day(&raw_dir, "fins", day, &rows).map_err(|e| e.0)?;
            let annuals = jquants::annual_reports(&rows);
            let periods = jquants::period_reports(&rows);
            pending.forecasts.extend(jquants::forecast_reports(&rows));
            if !periods.is_empty() {
                println!(
                    "{day}: {} financial rows ({} annual)",
                    periods.len(),
                    annuals.len()
                );
            }
            pending.annuals.extend(annuals);
            pending.periods.extend(periods);
        } else if forecast_from.is_some_and(|f| day >= f) {
            // 前に取り込んだ日なので、財務と生データはそのまま。予想の配当だけを足す
            let rows = jq
                .summaries_on(day)
                .await
                .map_err(|e| format!("{day} forecasts: {}", e.0))?;
            let forecasts = jquants::forecast_reports(&rows);
            if !forecasts.is_empty() {
                println!("{day}: {} dividend forecasts (refill)", forecasts.len());
            }
            pending.forecasts.extend(forecasts);
        }
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
    let mut forecasts = vec![];
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
        forecasts.extend(jquants::forecast_reports(&fins));
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
    file.save_forecasts(&forecasts, 0).await.map_err(|e| e.0)?;
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
        .save_splits(&pending.splits, now)
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
    store
        .save_forecasts(&pending.forecasts, now)
        .await
        .map_err(|e| e.0)?;
    println!(
        "persist: {} price histories, {} splits, {} annual reports, {} quarterly rows, {} dividend forecasts",
        rows.len(),
        pending.splits.len(),
        pending.annuals.len(),
        pending.periods.len(),
        pending.forecasts.len()
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

    // 4. 書類を取る。通信などで止まったら、まだ取っていない最初の書類の前日から次回やり直す。
    // 中身が読めない書類は、やり直しても読めないので飛ばして続ける
    let mut pending = vec![];
    let mut fetched = 0;
    let mut skipped = 0;
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
            Err(FetchError::Unreadable(e)) => {
                println!("sync-edinet: skip {} {}: {e}", doc.code, doc.doc_id);
                skipped += 1;
            }
            Err(FetchError::Stop(e)) => {
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
    let next = usecase::edinet::resume_cursor(listed_until, plan.fetch.get(fetched), today);
    store
        .set_state(EDINET_CURSOR_KEY, &next.to_string(), now)
        .await
        .map_err(|e| e.0)?;
    println!(
        "sync-edinet: {} fetched, {skipped} skipped, listed until {next}, {}s",
        fetched - skipped,
        started.elapsed().as_secs()
    );
    if let Some(e) = error {
        return Err(e);
    }
    if args.refetch_doe {
        let docs = store
            .edinet_docs_without_policy_values(now)
            .await
            .map_err(|e| e.0)?;
        refetch(
            &ed,
            &store,
            &raw_dir,
            now,
            started,
            deadline,
            &docs,
            "DOE / payout policy",
        )
        .await?;
    }
    if args.refetch_business {
        let docs = store
            .edinet_docs_without_business(now)
            .await
            .map_err(|e| e.0)?;
        refetch(
            &ed,
            &store,
            &raw_dir,
            now,
            started,
            deadline,
            &docs,
            "business text",
        )
        .await?;
    }
    Ok(())
}

/// 保存済みの書類を取り直す（この実行で取った分は除く。`what` はログに出す目的）。
/// 書類一覧の進み具合（EDINET_CURSOR_KEY）は変えない。時間切れなら、次に同じオプションで続きを取る
#[allow(clippy::too_many_arguments)]
async fn refetch(
    ed: &EdinetClient,
    store: &d1::D1Store,
    raw_dir: &std::path::Path,
    now: i64,
    started: Instant,
    deadline: std::time::Duration,
    docs: &[DocRef],
    what: &str,
) -> Result<(), String> {
    println!(
        "sync-edinet: {} stored reports to refetch for {what}",
        docs.len()
    );
    let mut pending = vec![];
    let mut done = 0;
    for doc in docs {
        if started.elapsed() > deadline {
            println!("sync-edinet: time limit reached; refetch the rest next time");
            break;
        }
        match fetch_doc(ed, raw_dir, doc).await {
            Ok(e) => pending.push(e),
            // 前は読めた書類でも、読めなければ飛ばす（行はそのまま残る）
            Err(FetchError::Unreadable(e)) => {
                println!("sync-edinet: skip {} {}: {e}", doc.code, doc.doc_id);
                continue;
            }
            Err(e) => return Err(format!("{} {}: {e}", doc.code, doc.doc_id)),
        }
        done += 1;
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
    println!("sync-edinet: {done} refetched for {what}");
    Ok(())
}

/// 1通を取れなかった理由
#[derive(Debug)]
enum FetchError {
    /// 書類の中身が読めない（本体の CSV がないなど）。やり直しても同じなので飛ばす
    Unreadable(String),
    /// 通信・書き出しの失敗など。次回やり直す
    Stop(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Unreadable(e) | FetchError::Stop(e) => f.write_str(e),
        }
    }
}

/// 1通を取り、文章を R2 用のファイルに書き、配当方針の手がかりを返す
async fn fetch_doc(
    ed: &EdinetClient,
    raw_dir: &std::path::Path,
    doc: &DocRef,
) -> Result<EdinetDoc, FetchError> {
    let zip = ed
        .fetch_csv_zip(&doc.doc_id)
        .await
        .map_err(|e| FetchError::Stop(e.0))?;
    let sections =
        adapter_batch::edinet::sections_from_zip(&zip).map_err(|e| FetchError::Unreadable(e.0))?;
    let refs: Vec<(domain::disclosure::Section, &str)> =
        sections.iter().map(|(s, t)| (*s, t.as_str())).collect();
    let signals = domain::disclosure::detect(&refs);
    // 「事業の内容」は注目テーマとの照合用に D1 にも短く持つ（項目がなければ空文字 = 読んだが無かった）
    let business_text = sections
        .iter()
        .find(|(s, _)| *s == domain::disclosure::Section::Business)
        .map(|(_, t)| domain::theme::business_excerpt(t))
        .unwrap_or_default();
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
            "doe_floor": signals.doe_floor,
            "doe_goal": signals.doe_goal,
            "doe_phrase": signals.doe_phrase,
            "payout_target": signals.payout_target,
            "phrases": signals.phrases,
        },
    });
    raw::write_object(
        raw_dir,
        &usecase::edinet::r2_key(&doc.code, doc.kind),
        &body,
    )
    .map_err(|e| FetchError::Stop(e.0))?;
    let flags = [
        (signals.progressive, "累進配当"),
        (signals.no_cut, "非減配"),
        (signals.raise, "増配"),
        (signals.doe_target, "DOE"),
        (signals.doe_floor.is_some(), "DOE下限"),
        (signals.doe_goal.is_some(), "DOE目標"),
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
        business_text,
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
        docs.push(
            fetch_doc(&ed, &raw_dir, doc)
                .await
                .map_err(|e| format!("{} {}: {e}", doc.code, doc.doc_id))?,
        );
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

/// sync-reit がどの日付までの書類一覧を読んだか
const REIT_CURSOR_KEY: &str = "reit_listed_until";
/// 用途の対応表（コード → 用途）
const REIT_SECTORS: &str = include_str!("../../../data/reit_sectors.csv");

/// REIT の有報を古い順に取り込む。同じ銘柄のもっと新しい期がすでにあれば、要約・明細は上書きされない（SQL 側で判定）
async fn sync_reit(args: Args) -> Result<(), String> {
    let started = Instant::now();
    let deadline = std::time::Duration::from_secs(args.max_minutes * 60);
    let client = http::client();
    let ed = EdinetClient::new(client.clone(), env("EDINET_API_KEY")?, args.rate);
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    let until = args.until.unwrap_or(today);
    let cache = PathBuf::from(&args.raw_dir).join("reit");
    let sectors = domain::reit::parse_sectors(REIT_SECTORS);
    let dry = args.dry_run.clone();
    let store = match &dry {
        Some(_) => None,
        None => Some(d1_store(client)?),
    };
    let start = match (&store, args.from) {
        (Some(st), from) => {
            let cursor = st
                .get_state(REIT_CURSOR_KEY)
                .await
                .map_err(|e| e.0)?
                .and_then(|s| Date::parse(&s));
            match (cursor, from) {
                (Some(c), _) => c.add_days(1),
                (None, Some(f)) => f,
                (None, None) => today.add_days(-EDINET_BACKFILL_DAYS),
            }
        }
        (None, Some(f)) => f,
        (None, None) => return Err("--dry-run needs --from".into()),
    };
    let days = usecase::fins::weekdays(start, until);
    println!(
        "sync-reit: listing {start}..{until} ({} weekdays)",
        days.len()
    );

    // 1. 書類一覧（1日1回の呼び出し）。時間切れなら読めた日までで先に進む
    let mut docs = vec![];
    let mut listed_until = until;
    for day in &days {
        if started.elapsed() > deadline {
            listed_until = day.add_days(-1);
            println!("sync-reit: time limit while listing; listed until {listed_until}");
            break;
        }
        docs.extend(
            ed.list_reit_day(*day)
                .await
                .map_err(|e| format!("{day} list: {}", e.0))?,
        );
    }
    docs.sort_by(|a, b| (a.submitted, &a.doc_id).cmp(&(b.submitted, &b.doc_id)));
    println!("sync-reit: {} REIT annual reports", docs.len());

    // 2. 書類ごとに読んで、要約・明細・推移を書く。時間切れなら、取り込めた書類の提出日の前日までを済みとする
    let mut sql = String::new();
    let mut saved = 0;
    for (i, d) in docs.iter().enumerate() {
        if started.elapsed() > deadline {
            listed_until = d.submitted.add_days(-1);
            println!(
                "sync-reit: time limit; {} of {} reports left",
                docs.len() - i,
                docs.len()
            );
            break;
        }
        let zip = match std::fs::read(cache.join(format!("{}.zip", d.doc_id))) {
            Ok(b) => b,
            Err(_) => ed
                .fetch_ixbrl_zip(&d.doc_id)
                .await
                .map_err(|e| format!("{}: {}", d.doc_id, e.0))?,
        };
        let record = match adapter_batch::edinet::read_reit_report(&zip) {
            Ok(rep) => adapter_batch::edinet::reit_record(d.clone(), rep),
            Err(e) => {
                println!(
                    "{} {} {}: 読めない ({})",
                    d.submitted, d.edinet_code, d.filer_name, e.0
                );
                continue;
            }
        };
        let Some(r) = record else {
            println!(
                "{} {} {}: 証券コードか期末日がない",
                d.submitted, d.edinet_code, d.filer_name
            );
            continue;
        };
        let body = adapter_batch::edinet::reit_detail_json(&r);
        let stmts = [
            adapter_batch::sql::reit_profile_stmt(
                &r,
                sectors.get(&r.code).map(|s| s.as_str()),
                now,
            ),
            adapter_batch::sql::reit_detail_stmt(&r, &body, now),
            adapter_batch::sql::reit_history_stmt(&r, now),
        ];
        match &store {
            Some(st) => {
                for s in &stmts {
                    st.exec(s)
                        .await
                        .map_err(|e| format!("{}: {}", r.code, e.0))?;
                }
            }
            None => stmts.iter().for_each(|s| {
                sql.push_str(&s.render());
                sql.push('\n');
            }),
        }
        saved += 1;
        println!(
            "{} {} {} 期末 {} 借入 {}件 {} / 物件 {}件 {}",
            d.submitted,
            r.code,
            d.filer_name,
            r.period_end,
            r.report.debt.loans.len(),
            r.checks.loans,
            r.portfolio.property_count,
            r.checks.properties
        );
    }

    // 3. 用途の対応表を反映し、続きの日付を残す
    let sector_stmts = adapter_batch::sql::reit_sector_stmts(&sectors);
    match (&store, &dry) {
        (Some(st), _) => {
            for s in &sector_stmts {
                st.exec(s).await.map_err(|e| e.0)?;
            }
            st.set_state(REIT_CURSOR_KEY, &listed_until.to_string(), now)
                .await
                .map_err(|e| e.0)?;
        }
        (None, Some(path)) => {
            sector_stmts.iter().for_each(|s| {
                sql.push_str(&s.render());
                sql.push('\n');
            });
            std::fs::write(path, sql).map_err(|e| e.to_string())?;
            println!("dry-run: SQL -> {path}");
        }
        (None, None) => unreachable!(),
    }
    println!("sync-reit: saved {saved} reports, listed until {listed_until}");
    Ok(())
}

/// 書類一覧（1日分）。{raw-dir}/reit/list-{日付}.json があればそれを使う
async fn reit_list_day(
    ed: &EdinetClient,
    dir: &std::path::Path,
    day: Date,
) -> Result<Vec<adapter_batch::edinet::ReitDocRef>, String> {
    let path = dir.join(format!("list-{day}.json"));
    if let Ok(body) = std::fs::read(&path) {
        let v: Vec<(String, String, String, String, Option<String>)> =
            serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        return Ok(v
            .into_iter()
            .filter_map(|(doc_id, edinet_code, filer_name, sub, pe)| {
                Some(adapter_batch::edinet::ReitDocRef {
                    doc_id,
                    edinet_code,
                    filer_name,
                    submitted: Date::parse(&sub)?,
                    period_end: pe.as_deref().and_then(Date::parse),
                })
            })
            .collect());
    }
    let docs = ed
        .list_reit_day(day)
        .await
        .map_err(|e| format!("{day} list: {}", e.0))?;
    let v: Vec<_> = docs
        .iter()
        .map(|d| {
            (
                &d.doc_id,
                &d.edinet_code,
                &d.filer_name,
                d.submitted.to_string(),
                d.period_end.map(|p| p.to_string()),
            )
        })
        .collect();
    std::fs::write(&path, serde_json::to_vec(&v).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(docs)
}

/// 読めた額と答え（BS・合計行）の比べ方: 一致 / 端数（0.1% 以内。百万円未満の切り捨てが行の数だけ積もるなど）/ ずれ
fn agree(got: f64, want: f64) -> &'static str {
    if (got - want).abs() < 0.5 {
        "一致"
    } else if want != 0.0 && ((got - want) / want).abs() <= 0.001 {
        "端数"
    } else {
        "ずれ"
    }
}

async fn reit_probe(args: Args) -> Result<(), String> {
    use domain::reit::{rate_mix, LoanKind};
    let ed = EdinetClient::new(http::client(), env("EDINET_API_KEY")?, args.rate);
    let today = Date::from_unix_jst(SystemClock.now_unix());
    let until = args.until.unwrap_or(today);
    let from = args.from.unwrap_or(until.add_days(-200));
    let dir = PathBuf::from(&args.raw_dir).join("reit");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 1. 書類一覧から、投資法人ごとに最新の有報
    let mut latest: HashMap<String, adapter_batch::edinet::ReitDocRef> = HashMap::new();
    for day in usecase::fins::weekdays(from, until) {
        for d in reit_list_day(&ed, &dir, day).await? {
            let newer = latest
                .get(&d.edinet_code)
                .is_none_or(|old| old.submitted < d.submitted);
            if newer {
                latest.insert(d.edinet_code.clone(), d);
            }
        }
    }
    let mut docs: Vec<_> = latest.into_values().collect();
    docs.sort_by(|a, b| a.edinet_code.cmp(&b.edinet_code));
    println!("reit-probe: {} reports ({from}..{until})", docs.len());

    // 2. 書類ごとに読んで突き合わせる
    let mut out = vec![];
    let mut tally: HashMap<&str, usize> = HashMap::new();
    for d in &docs {
        let path = dir.join(format!("{}.zip", d.doc_id));
        let zip = match std::fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                let b = ed
                    .fetch_ixbrl_zip(&d.doc_id)
                    .await
                    .map_err(|e| format!("{}: {}", d.doc_id, e.0))?;
                std::fs::write(&path, &b).map_err(|e| e.to_string())?;
                b
            }
        };
        let r = match adapter_batch::edinet::read_reit_report(&zip) {
            Ok(r) => r,
            Err(e) => {
                println!("{} {} 読めない: {}", d.edinet_code, d.filer_name, e.0);
                *tally.entry("読めない").or_default() += 1;
                continue;
            }
        };
        let sum = |k: Option<LoanKind>| {
            r.debt
                .loans
                .iter()
                .filter(|l| l.kind == k)
                .map(|l| l.balance_yen)
                .sum::<i64>()
        };
        let (short, long, nokind) = (
            sum(Some(LoanKind::Short)),
            sum(Some(LoanKind::Long)),
            sum(None),
        );
        let bs_loans = r.balance.short_loans + r.balance.long_loans;
        let loans_total = short + long + nokind;
        let total_check = if r.debt.loans.is_empty() {
            "明細なし"
        } else {
            agree(loans_total as f64, bs_loans as f64)
        };
        let kind_check = if nokind > 0 {
            "区分不明あり"
        } else if agree(short as f64, r.balance.short_loans as f64) == "ずれ" {
            "短期長期ずれ"
        } else {
            "区分OK"
        };
        let mix = rate_mix(&r.debt, r.balance.bonds);
        let debt_all = (mix.fixed
            + mix.swap_fixed
            + mix.swap_amount_only
            + mix.floating
            + mix.unknown
            + mix.bonds)
            .max(1) as f64;
        let pct = |v: i64| (v as f64 / debt_all * 1000.0).round() / 10.0;
        let leasable: f64 = r.properties.iter().map(|p| p.leasable_m2).sum();
        let prop_check = match (&r.property_total, r.properties.is_empty()) {
            (_, true) => "物件なし",
            (Some(t), false) => match agree(leasable, t.leasable_m2) {
                "ずれ" if ((leasable - t.leasable_m2) / t.leasable_m2).abs() <= 0.001 => "端数",
                a => a,
            },
            (None, false) => "合計行なし",
        };
        let occ_off = r
            .properties
            .iter()
            .filter(|p| !p.occupancy_computed)
            .filter(|p| match (p.leased_m2, p.occupancy_pct) {
                (Some(l), Some(o)) if p.leasable_m2 > 0.0 => {
                    (l / p.leasable_m2 * 100.0 - o).abs() > 0.15
                }
                _ => false,
            })
            .count();
        let rents = r
            .properties
            .iter()
            .filter(|p| p.annual_rent_yen.is_some())
            .count();
        for k in [total_check, kind_check, prop_check] {
            *tally.entry(k).or_default() += 1;
        }
        if mix.unknown == 0 && mix.swap_amount_only == 0 {
            *tally.entry("金利の種類が全額わかる").or_default() += 1;
        }
        println!(
            "{} {:<6} {:<28} 借入 {:>3}件 {} {} | 固定 {}% スワップ {}%(額のみ {}%) 変動 {}% 記載なし {}% 債 {}% | 物件 {:>3}件 {} 稼働率ずれ {} 賃料 {}件{}",
            d.edinet_code,
            r.code.as_deref().unwrap_or("-"),
            d.filer_name,
            r.debt.loans.len(),
            total_check,
            kind_check,
            pct(mix.fixed),
            pct(mix.swap_fixed + mix.swap_amount_only),
            pct(mix.swap_amount_only),
            pct(mix.floating),
            pct(mix.unknown),
            pct(mix.bonds),
            r.properties.len(),
            prop_check,
            occ_off,
            rents,
            if r.debt.rate_notes.is_empty() { String::new() } else { format!(" 注記[{}]", r.debt.rate_notes.join(", ")) },
        );
        out.push(serde_json::json!({
            "edinet_code": d.edinet_code,
            "code": r.code,
            "name": d.filer_name,
            "doc_id": d.doc_id,
            "submitted": d.submitted.to_string(),
            "period_end": d.period_end.map(|p| p.to_string()),
            "checks": { "loans": total_check, "kind": kind_check, "properties": prop_check, "occupancy_off": occ_off },
            "balance": { "short_loans": r.balance.short_loans, "long_loans": r.balance.long_loans, "bonds": r.balance.bonds },
            "loans_total": loans_total,
            "table_total": r.debt.table_total_yen,
            "swap": r.debt.swap_yen,
            "rate_mix": { "fixed": mix.fixed, "swap_fixed": mix.swap_fixed, "swap_amount_only": mix.swap_amount_only, "floating": mix.floating, "unknown": mix.unknown, "bonds": mix.bonds },
            "rate_notes": r.debt.rate_notes,
            "ladder": r.debt.ladder.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
            "loans": r.debt.loans.iter().map(|l| serde_json::json!({
                "kind": l.kind.map(|k| format!("{k:?}")), "lender": l.lender, "balance": l.balance_yen,
                "rate": l.rate_pct, "due": l.due.map(|d| d.to_string()), "rate_type": l.rate_type.map(|t| format!("{t:?}")),
            })).collect::<Vec<_>>(),
            "properties": r.properties.iter().map(|p| serde_json::json!({
                "id": p.id, "name": p.name, "leasable": p.leasable_m2, "leased": p.leased_m2,
                "occupancy": p.occupancy_pct, "occupancy_computed": p.occupancy_computed,
                "rent": p.annual_rent_yen, "rent_raw": p.rent_raw,
            })).collect::<Vec<_>>(),
            "property_total": r.property_total.as_ref().map(|t| serde_json::json!({ "leasable": t.leasable_m2, "leased": t.leased_m2 })),
        }));
    }
    let path = args
        .out
        .clone()
        .unwrap_or(format!("{}/reit-probe.json", args.raw_dir));
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&out).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut t: Vec<_> = tally.into_iter().collect();
    t.sort();
    println!(
        "summary ({} reports): {}",
        out.len(),
        t.iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("-> {path}");
    Ok(())
}

/// 調べる用: --from-file の zip の借入金明細表と注記を出す（--limit 行まで。既定 5 は少ないので大きめに）。
/// --label props なら、ファンドの状況の部分で見出しに 賃貸可能面積・稼働率 を含む表を、直前の文の末尾と一緒に出す
fn reit_dump(args: Args) -> Result<(), String> {
    let path = args.from_file.ok_or("--from-file <zip> required")?;
    let zip = std::fs::read(&path).map_err(|e| e.to_string())?;
    let (fund, accounts) = adapter_batch::edinet::reit_blocks(&zip).map_err(|e| e.0)?;
    if args.label == "props" {
        let mut prev = String::new();
        for (i, b) in fund.iter().enumerate() {
            match b {
                domain::reit::Block::Text(t) => {
                    prev = t
                        .chars()
                        .rev()
                        .take(80)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                }
                domain::reit::Block::Table(t, _) => {
                    let head: String = t.iter().take(3).flatten().cloned().collect();
                    if head.contains("賃貸可能面積")
                        || head.contains("稼働率")
                        || head.contains("賃貸面積")
                    {
                        println!(
                            "--- block {i} ({} rows, {} cols) prev: …{prev}",
                            t.len(),
                            t.iter().map(|r| r.len()).max().unwrap_or(0)
                        );
                        for r in t.iter().take(args.limit) {
                            println!("{}", r.join(" | "));
                        }
                    }
                }
            }
        }
        return Ok(());
    }
    let (tables, notes, unit) = domain::reit::loan_tables(&accounts);
    println!("unit {unit:?}, {} tables", tables.len());
    for (i, (t, flags)) in tables.iter().enumerate() {
        println!("--- table {i} ({} rows)", t.len());
        for (r, f) in t.iter().zip(flags.iter()).take(args.limit) {
            // 縦の結合の続きのマスは ^ を付ける
            let cells: Vec<String> = r
                .iter()
                .zip(f)
                .map(|(c, cont)| if *cont { format!("^{c}") } else { c.clone() })
                .collect();
            println!("{}", cells.join(" | "));
        }
    }
    println!(
        "--- notes: {}",
        notes.chars().take(1500).collect::<String>()
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
    let stored = usecase::daily::StoredFinancials {
        annuals: store.load_annuals().await.map_err(|e| e.0)?,
        forecasts: store.load_forecasts().await.map_err(|e| e.0)?,
        quarterlies: store.load_quarterlies().await.map_err(|e| e.0)?,
        splits: store.load_splits().await.map_err(|e| e.0)?,
    };
    println!(
        "compute: {} codes, {} price histories, {} with annual financials, {} with quarterly rows, {} with splits, \
         {} with dividend forecasts",
        codes.len(),
        histories.len(),
        stored.annuals.len(),
        stored.quarterlies.len(),
        stored.splits.len(),
        stored.forecasts.len()
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
        usecase::daily::compute_all(&codes, histories, &stored, &run_id, started);
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

/// 価格取得・計算の後に実行する。推薦を新しく作らない日も追跡を継続する。
async fn evaluate_recommendations() -> Result<(), String> {
    let now = SystemClock.now_unix();
    let client = http::client();
    let csv = http::get_bytes(&client, http::HOLIDAYS_URL)
        .await
        .map_err(|e| e.0)?;
    let holidays = domain::calendar::parse_holiday_csv(&csv);
    // 引け前の手動実行では前日まで。日次バッチは18:30以降。
    let today = Date::from_unix_jst(now);
    let hour = (now + 9 * 3600).rem_euclid(86400) / 3600;
    let as_of = if hour < 16 { today.add_days(-1) } else { today };
    let count =
        adapter_batch::recommendation_evaluation::run(&d1_store(client)?, &holidays, as_of, now)
            .await
            .map_err(|e| e.0)?;
    println!("recommendation evaluation: {count} new results through {as_of}");
    Ok(())
}
