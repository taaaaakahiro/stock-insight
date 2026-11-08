use super::{internal, json_error, json_response, AuthUser, DB_BINDING};
use adapter_cf::d1_reit_scoring as repo;
use domain::reit_scoring::{self, comparison_codes};
use domain::{Date, StockCode};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use worker::{D1Database, Request, Response, Result, RouteContext};

type Params = BTreeMap<String, String>;

fn bad(message: &str) -> Result<Response> {
    json_error(400, "bad_request", message)
}
fn missing() -> Result<Response> {
    json_error(
        404,
        "not_found",
        "no J-REIT in this snapshot for the requested ticker",
    )
}
fn ok(data: &Value) -> Result<Response> {
    json_response(data, 200, "private, no-store")
}
fn query(req: &Request) -> Result<Params> {
    Ok(req.url()?.query_pairs().into_owned().collect())
}
fn as_of(params: &Params) -> std::result::Result<Option<&str>, ()> {
    params
        .get("as_of")
        .map(|s| {
            let d = Date::parse(s).ok_or(())?;
            (d.y >= 1900 && d.to_string() == *s && Date::from_days(d.to_days()) == d)
                .then_some(s.as_str())
                .ok_or(())
        })
        .transpose()
}
fn threshold(params: &Params, key: &str, max: Option<f64>) -> std::result::Result<Option<f64>, ()> {
    params
        .get(key)
        .map(|s| {
            let value = s.parse::<f64>().map_err(|_| ())?;
            (value.is_finite() && value >= 0.0 && max.is_none_or(|m| value <= m))
                .then_some(value)
                .ok_or(())
        })
        .transpose()
}

pub async fn rankings(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let params = query(&req)?;
    let profile = params.get("profile").map_or("total", String::as_str);
    if ![
        "total",
        "distribution",
        "profitability",
        "finance",
        "nav",
        "valuation",
    ]
    .contains(&profile)
    {
        return bad("invalid ranking profile");
    }
    let sector = params.get("sector").map(String::as_str);
    if sector.is_some_and(|s| s != "unknown" && !domain::reit::SECTORS.contains(&s)) {
        return bad("invalid sector");
    }
    let (Ok(day), Ok(min_yield), Ok(max_float), Ok(max_ltv), Ok(min_coverage)) = (
        as_of(&params),
        threshold(&params, "min_yield", None),
        threshold(&params, "max_floating_ratio", Some(1.0)),
        threshold(&params, "max_ltv", Some(1.0)),
        threshold(&params, "min_coverage", Some(100.0)),
    ) else {
        return bad("invalid date or filter value");
    };
    let label = reit_scoring::CATEGORIES
        .iter()
        .find(|(key, _)| *key == profile)
        .map_or("総合", |(_, label)| *label);
    let db = ctx.env.d1(DB_BINDING)?;
    let run = match repo::run(&db, day).await {
        Ok(v) => v,
        Err(e) => return internal(&e.to_string()),
    };
    let Some(run) = run else {
        return ok(
            &json!({"status":"not_generated","run":null,"profile":profile,"profile_label":label,"items":[]}),
        );
    };
    let mut items = match repo::rankings(&db, &run).await {
        Ok(v) => v,
        Err(e) => return internal(&e.to_string()),
    };
    items.retain(|r| {
        let at_least =
            |k: &str, n: Option<f64>| n.is_none_or(|n| r[k].as_f64().is_some_and(|v| v >= n));
        let at_most =
            |k: &str, n: Option<f64>| n.is_none_or(|n| r[k].as_f64().is_some_and(|v| v <= n));
        sector.is_none_or(|s| r["sector"] == s)
            && at_least("actual_yield", min_yield)
            && at_most("floating_ratio", max_float)
            && at_most("ltv", max_ltv)
            && at_least("coverage_pct", min_coverage)
    });
    items.sort_by_key(|r| r["ranks"][profile].as_u64().unwrap_or(u64::MAX));
    for item in &mut items {
        item["rank"] = item["ranks"][profile].clone();
        item["sector_rank"] = item["sector_ranks"][profile].clone();
        item["profile_score"] = item["profiles"][profile].clone();
    }
    ok(
        &json!({"status":"ready","run":run.json(),"profile":profile,"profile_label":label,"items":items}),
    )
}

pub async fn compare(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let params = query(&req)?;
    let Some(codes) = comparison_codes(params.get("tickers").map_or("", String::as_str)) else {
        return bad("tickers must contain 1 to 5 unique J-REIT codes");
    };
    scores(ctx.env.d1(DB_BINDING)?, &params, &codes).await
}

pub async fn detail(req: Request, ctx: RouteContext<AuthUser>) -> Result<Response> {
    let params = query(&req)?;
    let Some(code) = ctx.param("ticker").and_then(|s| StockCode::parse(s)) else {
        return bad("invalid ticker");
    };
    scores(ctx.env.d1(DB_BINDING)?, &params, &[code.as_str().into()]).await
}

async fn scores(db: D1Database, params: &Params, codes: &[String]) -> Result<Response> {
    let Ok(day) = as_of(params) else {
        return bad("invalid as_of date");
    };
    let run = if let Some(id) = params.get("run_id") {
        if day.is_some()
            || id.is_empty()
            || id.len() > 100
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return bad("invalid run_id");
        }
        match repo::run_by_id(&db, id).await {
            Ok(Some(run)) => Ok(Some(run)),
            Ok(None) => return json_error(404, "not_found", "no such published snapshot"),
            Err(e) => Err(e),
        }
    } else {
        repo::run(&db, day).await
    };
    match run {
        Ok(Some(run)) => match repo::details(&db, &run, codes).await {
            Ok(items) if items.len() == codes.len() => {
                ok(&json!({"status":"ready","run":run.json(),"items":items}))
            }
            Ok(_) => missing(),
            Err(e) => internal(&e.to_string()),
        },
        Ok(None) => match repo::listed(&db, codes).await {
            Ok(true) => ok(&json!({"status":"not_generated","run":null,"items":[]})),
            Ok(false) => missing(),
            Err(e) => internal(&e.to_string()),
        },
        Err(e) => internal(&e.to_string()),
    }
}
