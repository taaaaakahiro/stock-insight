//! 画面向けの API Worker。D1 を読むだけで、重い計算はしない。
//!
//! すべての /api/* は Clerk のセッショントークン（Authorization: Bearer）と許可リストで守る。
//!   GET /api/me       ログイン中のユーザー
//!   GET /api/stocks   スクリーニング一覧（F2）
//!   GET /api/status   データ鮮度（F8）
//!   GET /api/portfolio              登録した銘柄と売り時のサイン
//!   PUT /api/portfolio/:code        銘柄を登録
//!   DELETE /api/portfolio/:code     登録を外す
//!   GET /api/recommendations        最新日の AIレコメンド（F7）
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
use usecase::auth::{authenticate, AuthError, AuthUser, RequestMeta};
use usecase::portfolio::PortfolioError;
use usecase::screening::{data_status, screen, StockRow};
use usecase::Clock;
use worker::{
    event, Context, Env, Request, Response, Result, RouteContext, Router, ScheduleContext,
    ScheduledEvent,
};

const DB_BINDING: &str = "DB";
const AI_BINDING: &str = "AI";
/// wrangler.jsonc の triggers.crons のうち、AIレコメンドを作るほう
const RECOMMEND_CRON: &str = "0 12 * * 1-5";

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
    let result = authenticate(
        token,
        &meta,
        cfg.admin_email.as_deref(),
        &ClerkVerifier::new(&cfg.issuer, cfg.authorized_parties),
        &ClerkDirectory::new(&cfg.api_url, cfg.secret_key),
        &D1Users::new(db(env)?),
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
    let repo = D1Indicators::new(ctx.env.d1(DB_BINDING)?);
    match screen(&repo, &filter).await {
        Ok(page) => json_ok(&json!({
            "rows": page.rows.iter().map(row_json).collect::<Vec<_>>(),
            "total": page.total,
            "page": page.page,
            "page_size": page.page_size,
        })),
        Err(e) => internal(&e.0),
    }
}

async fn status(_req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let repo = D1Indicators::new(ctx.env.d1(DB_BINDING)?);
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
            })),
        })),
        Err(e) => internal(&e.0),
    }
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
    })
}

/// from_json が付ける Content-Type を残したまま Cache-Control を足す
fn json_response(body: &Value, status: u16, cache: &str) -> Result<Response> {
    let mut res = Response::from_json(body)?.with_status(status);
    res.headers_mut().set("Cache-Control", cache)?;
    Ok(res)
}

fn json_ok(body: &Value) -> Result<Response> {
    json_response(body, 200, "private, max-age=300")
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
