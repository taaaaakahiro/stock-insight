//! 画面向けの API Worker。D1 を読むだけで、重い計算はしない。
//!
//! すべての /api/* は Clerk のセッショントークン（Authorization: Bearer）と許可リストで守る。
//!   GET /api/me       ログイン中のユーザー
//!   GET /api/stocks   スクリーニング一覧（F2）
//!   GET /api/status   データ鮮度（F8）
//!   GET /api/stocks/:code/edinet    有報・半期報告書（最新1通ずつ）から拾った配当方針の手がかり
//!   GET /api/portfolio              登録した銘柄と売り時のサイン
//!   PUT /api/portfolio/:code        銘柄を登録
//!   DELETE /api/portfolio/:code     登録を外す
//!   GET /api/recommendations        最新日の AIレコメンド（F7）
//!
//! 一覧・配当方針・状態は、データの版（最新バッチと EDINET の取り込み）をキーに Cache API へ置く。
//! D1 の無料枠（読み取り 500万行/日）を画面の操作で使い切らないため。
//!
//! Cron Trigger（平日18:30 JST）では、GitHub Actions の日次バッチ（daily-batch.yml）を起動する。
//! もう1つの Cron（平日21:00 JST）では、その日のバッチが成功していれば Workers AI でレコメンドを作って保存する。

use adapter_cf::ai::WorkersAi;
use adapter_cf::clerk::{ClerkDirectory, ClerkVerifier};
use adapter_cf::d1::{D1Indicators, D1Portfolio};
use adapter_cf::d1_recommend::D1Recommendations;
use adapter_cf::users::D1Users;
use domain::access::Access;
use domain::screening::ScreeningFilter;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use usecase::auth::{authenticate, AuthError, AuthUser, CachedUsers, RequestMeta, UserMemo};
use usecase::detail::{stock_detail, StockDetail};
use usecase::portfolio::PortfolioError;
use usecase::screening::{data_status, disclosures, screen, StockRow};
use usecase::Clock;
use worker::{
    event, Cache, Context, Env, Request, Response, Result, RouteContext, Router, ScheduleContext,
    ScheduledEvent,
};

const DB_BINDING: &str = "DB";
const AI_BINDING: &str = "AI";
/// wrangler.jsonc の triggers.crons のうち、AIレコメンドを作るほう
const RECOMMEND_CRON: &str = "0 12 * * 1-5";

/// ブラウザに持たせる時間（一覧・配当方針・状態）
const BROWSER_CACHE: &str = "private, max-age=300";
/// Cache API に置く時間。版が変わればキーも変わるので、長くてよい
const SHARED_TTL_SECS: u32 = 86_400;
/// データの版を D1 に聞き直す間隔（秒）。バッチの完了はこの時間だけ遅れて反映される
const VERSION_TTL_SECS: i64 = 60;

thread_local! {
    // isolate 内で使い回す: users の行と、データの版（確認時刻, 版）
    static USERS: Rc<UserMemo> = Rc::default();
    static DATA_VERSION: RefCell<Option<(i64, Option<String>)>> = const { RefCell::new(None) };
}

struct WorkerClock;
impl Clock for WorkerClock {
    fn now_unix(&self) -> i64 {
        (worker::Date::now().as_millis() / 1000) as i64
    }
}

/// 設定（wrangler.jsonc の vars と Worker Secret）。足りなければ認証を通さない
struct AuthConfig {
    issuer: String,
    api_url: String,
    authorized_parties: Vec<String>,
    secret_key: String,
    admin_email: Option<String>,
}

impl AuthConfig {
    fn from_env(env: &Env) -> Option<Self> {
        let var = |k: &str| {
            env.var(k)
                .ok()
                .map(|v| v.to_string())
                .filter(|v| !v.is_empty())
        };
        let secret = |k: &str| {
            env.secret(k)
                .ok()
                .map(|v| v.to_string())
                .filter(|v| !v.is_empty())
        };
        Some(Self {
            issuer: var("CLERK_ISSUER")?,
            api_url: var("CLERK_API_URL").unwrap_or_else(|| "https://api.clerk.com/v1".into()),
            authorized_parties: var("AUTHORIZED_PARTIES")?
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            secret_key: secret("CLERK_SECRET_KEY")?,
            admin_email: secret("ADMIN_NOTIFY_EMAIL"),
        })
    }
}

/// 日次バッチの起動。重複や休場日の判定は GitHub 側（batch-cli should-run）で行う。
/// AIレコメンドの Cron はここで分岐する
#[event(scheduled)]
async fn scheduled(event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    if event.cron() == RECOMMEND_CRON {
        recommend_cron(&env).await;
        return;
    }
    let repo = env
        .var("GITHUB_REPO")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let Ok(token) = env.secret("GITHUB_DISPATCH_TOKEN").map(|v| v.to_string()) else {
        worker::console_error!("cron {}: GITHUB_DISPATCH_TOKEN is not set", event.cron());
        return;
    };
    match adapter_cf::github::dispatch_workflow(&repo, "daily-batch.yml", "main", &token).await {
        Ok(()) => worker::console_log!("cron {}: dispatched daily-batch", event.cron()),
        Err(e) => worker::console_error!("cron {}: {e}", event.cron()),
    }
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let user = match auth(&req, &env).await {
        Ok(u) => u,
        Err(res) => return res,
    };
    Router::with_data(user)
        .get_async("/api/me", me)
        .get_async("/api/stocks", stocks)
        .get_async("/api/stocks/:code", stock_detail_route)
        .get_async("/api/stocks/:code/edinet", edinet)
        .get_async("/api/status", status)
        .get_async("/api/portfolio", portfolio)
        .put_async("/api/portfolio/:code", portfolio_add)
        .delete_async("/api/portfolio/:code", portfolio_remove)
        .get_async("/api/recommendations", recommendations)
        .or_else_any_method("/api/*path", |_, _| {
            json_error(404, "not_found", "no such endpoint")
        })
        .run(req, env)
        .await
}

/// 本人確認。失敗したらそのまま返す応答を Err に入れる
async fn auth(req: &Request, env: &Env) -> std::result::Result<AuthUser, Result<Response>> {
    let Some(cfg) = AuthConfig::from_env(env) else {
        worker::console_error!("auth is not configured");
        return Err(json_error(
            503,
            "unavailable",
            "authentication is not configured",
        ));
    };
    let header = req.headers().get("Authorization").ok().flatten();
    let token = header.as_deref().and_then(|h| h.strip_prefix("Bearer "));
    let meta = RequestMeta {
        ip_address: req.headers().get("CF-Connecting-IP").ok().flatten(),
        user_agent: req.headers().get("User-Agent").ok().flatten(),
    };
    let db = |e: &Env| e.d1(DB_BINDING).map_err(|e| internal(&e.to_string()));
    let users = D1Users::new(db(env)?);
    let memo = USERS.with(Rc::clone);
    let result = authenticate(
        token,
        &meta,
        cfg.admin_email.as_deref(),
        &ClerkVerifier::new(&cfg.issuer, cfg.authorized_parties),
        &ClerkDirectory::new(&cfg.api_url, cfg.secret_key),
        &CachedUsers::new(&users, &memo, WorkerClock.now_unix()),
        &WorkerClock,
    )
    .await;
    result.map_err(|e| match e {
        AuthError::Unauthenticated(reason) => {
            // 理由はログにだけ出す（トークンの中身は出さない）
            worker::console_log!("unauthenticated: {reason}");
            json_error(401, "unauthenticated", "sign in required").and_then(|mut r| {
                r.headers_mut().set("WWW-Authenticate", "Bearer")?;
                Ok(r)
            })
        }
        AuthError::Forbidden(access) => json_error(
            403,
            "forbidden",
            match access {
                Access::DeniedNotGoogle => "sign in with Google",
                _ => "this account is not allowed",
            },
        ),
        AuthError::Unavailable(detail) => {
            worker::console_error!("auth unavailable: {detail}");
            json_error(503, "unavailable", "try again later")
        }
    })
}

async fn me(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let u = &ctx.data;
    json_response(
        &json!({ "id": u.id, "email": u.email, "display_name": u.display_name, "is_admin": u.is_admin }),
        200,
        "no-store",
    )
}

async fn stocks(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let url = req.url()?;
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let filter =
        match ScreeningFilter::from_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str()))) {
            Ok(f) => f,
            Err(e) => return json_error(400, "bad_request", &e.0),
        };
    shared(&req, &ctx.env, |repo| async move {
        match screen(&repo, &filter).await {
            Ok(page) => json_ok(&json!({
                "rows": page.rows.iter().map(row_json).collect::<Vec<_>>(),
                "total": page.total,
                "page": page.page,
                "page_size": page.page_size,
            })),
            Err(e) => internal(&e.0),
        }
    })
    .await
}

async fn edinet(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let code = ctx.param("code").cloned().unwrap_or_default();
    shared(&req, &ctx.env, |repo| async move {
        match disclosures(&repo, &code).await {
            Ok(Some(docs)) => json_ok(&json!({
                "code": code,
                "docs": docs.iter().map(|d| json!({
                    "doc_kind": d.doc_kind,
                    "doc_id": d.doc_id,
                    "submitted": d.submitted,
                    "period_end": d.period_end,
                    "progressive_dividend": d.progressive_dividend,
                    "no_dividend_cut": d.no_dividend_cut,
                    "dividend_raise": d.dividend_raise,
                    "doe_target": d.doe_target,
                    "payout_target": d.payout_target,
                    "phrases": d.phrases,
                })).collect::<Vec<_>>(),
            })),
            Ok(None) => json_error(400, "bad_request", "bad stock code"),
            Err(e) => internal(&e.0),
        }
    })
    .await
}

async fn status(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    shared(&req, &ctx.env, |repo| async move {
        match data_status(&repo).await {
            Ok(s) => json_ok(&json!({
                // リポジトリの Cargo.toml（workspace.package.version）。画面の表示と GitHub のタグに合わせる
                "version": env!("CARGO_PKG_VERSION"),
                "latest_run": s.map(|s| json!({
                    "run_id": s.run_id,
                    "started_at": s.started_at,
                    "finished_at": s.finished_at,
                    "status": s.status,
                    "latest_price_date": s.latest_price_date,
                    "latest_fin_date": s.latest_fin_date,
                })),
            })),
            Err(e) => internal(&e.0),
        }
    })
    .await
}

/// 全員に同じ中身を返す GET を、データの版ごとに Cache API へ置く。
/// 版が変わればキーも ETag も変わるので、バッチの後に古い中身は返さない。
/// 版がわからない（バッチの実行中・D1 のエラー）ときはキャッシュを使わずに D1 を読む
async fn shared<F, Fut>(req: &Request, env: &Env, build: F) -> Result<Response>
where
    F: FnOnce(D1Indicators) -> Fut,
    Fut: Future<Output = Result<Response>>,
{
    let repo = D1Indicators::new(env.d1(DB_BINDING)?);
    let Some(version) = data_version(&repo).await else {
        return build(repo).await;
    };
    // 画面の形が変わる版の更新でも切り替わるよう、アプリの版も混ぜる
    let tag = format!(
        "W/\"{:016x}\"",
        fnv1a(&format!("{}|{version}", env!("CARGO_PKG_VERSION")))
    );
    let not_modified = req
        .headers()
        .get("If-None-Match")?
        .is_some_and(|h| h.split(',').any(|t| t.trim() == tag));
    if not_modified {
        let mut res = Response::empty()?.with_status(304);
        res.headers_mut().set("ETag", &tag)?;
        res.headers_mut().set("Cache-Control", BROWSER_CACHE)?;
        return Ok(res);
    }
    // キーはパスとクエリと版だけ（Authorization は含めない。中身は誰が見ても同じ）
    let mut key = req.url()?;
    key.query_pairs_mut()
        .append_pair("_v", &tag[3..tag.len() - 1]);
    let cache = Cache::default();
    match cache.get(key.as_str(), false).await {
        Ok(Some(mut hit)) => return json_bytes(hit.bytes().await?, BROWSER_CACHE, Some(&tag)),
        Ok(None) => {}
        Err(e) => worker::console_error!("cache get: {e}"),
    }
    let mut res = build(repo).await?;
    if res.status_code() != 200 {
        return Ok(res);
    }
    let body = res.bytes().await?;
    let stored = json_bytes(
        body.clone(),
        &format!("public, max-age={SHARED_TTL_SECS}"),
        None,
    )?;
    if let Err(e) = cache.put(key.as_str(), stored).await {
        worker::console_error!("cache put: {e}");
    }
    json_bytes(body, BROWSER_CACHE, Some(&tag))
}

/// データの版。isolate のメモリに VERSION_TTL_SECS だけ持つ
async fn data_version(repo: &D1Indicators) -> Option<String> {
    let now = WorkerClock.now_unix();
    let cached = DATA_VERSION.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(at, _)| now - at < VERSION_TTL_SECS)
            .map(|(_, v)| v.clone())
    });
    if let Some(v) = cached {
        return v;
    }
    match repo.data_version(now).await {
        Ok(v) => {
            DATA_VERSION.with(|c| *c.borrow_mut() = Some((now, v.clone())));
            v
        }
        Err(e) => {
            worker::console_error!("data version: {e}");
            None
        }
    }
}

/// ETag 用の短いハッシュ（FNV-1a 64bit）
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

async fn portfolio(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let repo = D1Portfolio::new(ctx.env.d1(DB_BINDING)?);
    match usecase::portfolio::list(&repo, &ctx.data.id).await {
        // 登録・解除の直後に古い一覧を見せないよう、キャッシュしない
        Ok(p) => json_response(
            &json!({
                "codes": p.codes,
                "rows": p.rows.iter().map(|h| {
                    let mut v = row_json(&h.stock);
                    v["signals"] = json!(h.signals.iter().map(|s| s.as_str()).collect::<Vec<_>>());
                    v
                }).collect::<Vec<_>>(),
            }),
            200,
            "no-store",
        ),
        Err(e) => internal(&e.0),
    }
}

async fn portfolio_add(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let repo = D1Portfolio::new(ctx.env.d1(DB_BINDING)?);
    let code = ctx.param("code").cloned().unwrap_or_default();
    let now = WorkerClock.now_unix();
    portfolio_result(usecase::portfolio::add(&repo, &ctx.data.id, &code, now).await)
}

async fn portfolio_remove(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let repo = D1Portfolio::new(ctx.env.d1(DB_BINDING)?);
    let code = ctx.param("code").cloned().unwrap_or_default();
    portfolio_result(usecase::portfolio::remove(&repo, &ctx.data.id, &code).await)
}

fn portfolio_result(r: std::result::Result<(), PortfolioError>) -> Result<Response> {
    match r {
        Ok(()) => json_response(&json!({ "ok": true }), 200, "no-store"),
        Err(PortfolioError::BadCode) => json_error(400, "bad_request", "bad stock code"),
        Err(PortfolioError::TooMany) => json_error(
            409,
            "too_many",
            &format!("up to {} stocks", domain::portfolio::MAX_ITEMS),
        ),
        Err(PortfolioError::Port(e)) => internal(&e.0),
    }
}

/// F7: その日のバッチが成功していて、まだ結果がなければ AI に上位10銘柄を選ばせて保存する
async fn recommend_cron(env: &Env) {
    let (db, ai) = match (env.d1(DB_BINDING), env.ai(AI_BINDING)) {
        (Ok(db), Ok(ai)) => (db, ai),
        (Err(e), _) | (_, Err(e)) => {
            worker::console_error!("cron {RECOMMEND_CRON}: binding: {e}");
            return;
        }
    };
    let repo = D1Recommendations::new(db);
    let now = WorkerClock.now_unix();
    match usecase::recommend::generate(&repo, &WorkersAi::new(ai), now).await {
        Ok(usecase::recommend::Outcome::Saved { date, picks }) => {
            worker::console_log!("cron {RECOMMEND_CRON}: saved {picks} recommendations for {date}")
        }
        Ok(usecase::recommend::Outcome::Skipped(why)) => {
            worker::console_log!("cron {RECOMMEND_CRON}: skipped ({why})")
        }
        Err(e) => worker::console_error!("cron {RECOMMEND_CRON}: {e}"),
    }
}

async fn recommendations(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let repo = D1Recommendations::new(ctx.env.d1(DB_BINDING)?);
    match usecase::recommend::latest(&repo).await {
        Ok(day) => json_ok(&json!({
            "day": day.map(|d| json!({
                "date": d.date,
                "model": d.model,
                "created_at": d.created_at,
                "rows": d.rows.iter().map(|r| {
                    let mut v = row_json(&r.stock);
                    v["rank"] = json!(r.rank);
                    v["reason"] = json!(r.reason);
                    v
                }).collect::<Vec<_>>(),
            })),
        })),
        Err(e) => internal(&e.0),
    }
}

/// 銘柄の詳細（F3）: 5年の年次、四半期単独の損益、利回りと株価の推移
async fn stock_detail_route(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let Some(code) = ctx.param("code").and_then(|c| domain::StockCode::parse(c)) else {
        return json_error(400, "bad_request", "bad stock code");
    };
    let repo = D1Indicators::new(ctx.env.d1(DB_BINDING)?);
    match stock_detail(&repo, &code).await {
        Ok(Some(d)) => json_ok(&detail_json(&d)),
        Ok(None) => json_error(404, "not_found", "no such stock"),
        Err(e) => internal(&e.0),
    }
}

fn detail_json(d: &StockDetail) -> Value {
    let ratio = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    };
    json!({
        "stock": row_json(&d.latest),
        "annual": d.annual.iter().map(|y| json!({
            "period_end": y.period_end.map(|p| p.to_string()),
            "net_sales": y.net_sales,
            "operating_income": y.operating_income,
            "ordinary_income": y.ordinary_income,
            "net_income": y.net_income,
            "equity": y.equity,
            "eps": y.eps,
            "bps": y.bps,
            "dps": y.dps,
            "operating_cf": y.operating_cf,
            "payout_ratio": ratio(y.dps, y.eps),
            "operating_margin": ratio(y.operating_income, y.net_sales),
        })).collect::<Vec<_>>(),
        "quarters": d.quarters.iter().map(|q| json!({
            "fiscal_year_end": q.fiscal_year_end.to_string(),
            "quarter": q.quarter,
            "period_end": q.period_end.to_string(),
            "net_sales": q.net_sales,
            "operating_income": q.operating_income,
            "ordinary_income": q.ordinary_income,
            "net_income": q.net_income,
        })).collect::<Vec<_>>(),
        "yield_series": d.yield_series.iter().map(|(dt, y)| json!([dt.to_string(), y])).collect::<Vec<_>>(),
        "price_series": d.price_series.iter().map(|b| json!([b.date.to_string(), b.close])).collect::<Vec<_>>(),
        "latest_disclosed": d.latest_disclosed.map(|d| d.to_string()),
    })
}
fn row_json(r: &StockRow) -> Value {
    json!({
        "code": r.code,
        "price": r.price,
        "price_date": r.price_date,
        "dividend_yield": r.dividend_yield,
        "yield_percentile": r.yield_percentile,
        "pbr": r.pbr,
        "per": r.per,
        "payout_ratio": r.payout_ratio,
        "doe": r.doe,
        "consecutive_increases": r.consecutive_increases,
        "operating_margin": r.operating_margin,
        "no_loss_years": r.no_loss_years,
        "profit_uptrend": r.profit_uptrend,
        "valuation": r.valuation,
        "fetched_at": r.fetched_at,
        "name": r.name,
        "kind": r.kind,
        "market": r.market,
        "sector33": r.sector33,
        "size_class": r.size_class,
        "dividend_every_year": r.dividend_every_year,
        "no_quarterly_loss": r.no_quarterly_loss,
        "is_value": r.is_value,
        "data_years": r.data_years,
        "sales_growth": r.sales_growth,
        "op_growth": r.op_growth,
        "progressive_dividend": r.progressive_dividend,
        "no_dividend_cut": r.no_dividend_cut,
    })
}

/// from_json が付ける Content-Type を残したまま Cache-Control を足す
fn json_response(body: &Value, status: u16, cache: &str) -> Result<Response> {
    let mut res = Response::from_json(body)?.with_status(status);
    res.headers_mut().set("Cache-Control", cache)?;
    Ok(res)
}

fn json_ok(body: &Value) -> Result<Response> {
    json_response(body, 200, BROWSER_CACHE)
}

/// 組み立て済みの JSON を返す（キャッシュから出すとき・置くとき）
fn json_bytes(body: Vec<u8>, cache: &str, etag: Option<&str>) -> Result<Response> {
    let mut res = Response::from_bytes(body)?;
    let h = res.headers_mut();
    h.set("Content-Type", "application/json")?;
    h.set("Cache-Control", cache)?;
    if let Some(t) = etag {
        h.set("ETag", t)?;
    }
    Ok(res)
}

fn json_error(status: u16, code: &str, message: &str) -> Result<Response> {
    json_response(
        &json!({ "error": { "code": code, "message": message } }),
        status,
        "no-store",
    )
}

/// 内部エラーの詳細は応答に出さず、Worker のログにだけ残す
fn internal(detail: &str) -> Result<Response> {
    worker::console_error!("internal error: {detail}");
    json_error(500, "internal", "internal error")
}
