//! 画面向けの API Worker。D1 を読むだけで、重い計算はしない。
//!
//! 現時点のエンドポイント（認証は次の段階で Clerk を入れる。それまで公開デプロイはしない）:
//!   GET /api/stocks   スクリーニング一覧（F2）
//!   GET /api/status   データ鮮度（F8）

use adapter_cf::d1::D1Indicators;
use domain::screening::ScreeningFilter;
use serde_json::{json, Value};
use usecase::screening::{data_status, screen, StockRow};
use worker::{event, Context, Env, Request, Response, Result, RouteContext, Router};

const DB_BINDING: &str = "DB";

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    Router::new()
        .get_async("/api/stocks", stocks)
        .get_async("/api/status", status)
        .or_else_any_method("/api/*path", |_, _| {
            json_error(404, "not_found", "no such endpoint")
        })
        .run(req, env)
        .await
}

async fn stocks(req: Request, ctx: RouteContext<()>) -> Result<Response> {
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

async fn status(_req: Request, ctx: RouteContext<()>) -> Result<Response> {
    let repo = D1Indicators::new(ctx.env.d1(DB_BINDING)?);
    match data_status(&repo).await {
        Ok(s) => json_ok(&json!({
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
