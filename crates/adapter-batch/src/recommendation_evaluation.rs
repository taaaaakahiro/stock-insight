//! 日次バッチの推薦追跡。データと評価の訂正は追記し、完了した計算だけを公開する。
use crate::{
    d1::D1Store,
    sql::{Stmt, Val},
};
use domain::{
    recommendation_evaluation::{evaluate, Evaluation, HORIZONS, VERSION},
    split::Split,
    Date, PriceBar,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use usecase::PortError;

fn text(s: impl Into<String>) -> Val {
    Val::Text(s.into())
}
fn stmt(sql: &str, params: Vec<Val>) -> Stmt {
    Stmt {
        sql: sql.into(),
        params,
    }
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, PortError> {
    v[key]
        .as_str()
        .ok_or_else(|| PortError(format!("evaluation: missing {key}")))
}

pub fn result_json(e: &Evaluation) -> Value {
    json!({
        "status":e.status, "entry_date":e.entry_date.map(|d|d.to_string()),
        "end_date":e.end_date.map(|d|d.to_string()), "entry_price":e.entry_price,
        "end_price":e.end_price,"price_return":e.price_return,"max_drawdown":e.max_drawdown,
        "missing_days":e.missing_days,"total_return":null,"dividend_status":"not_available",
        "version":VERSION,
    })
}

/// 欠損を除いて平均すると結果を良く見せるため、全構成銘柄が揃った場合だけ集計する。
pub fn aggregate(values: &[&Value]) -> Value {
    let mut out = json!({"status":"pending","price_return":null,"max_drawdown":null,
        "entry_date":null,"end_date":null,"total_return":null,"dividend_status":"not_available",
        "count":values.len(),"available":0,"version":VERSION});
    if values.is_empty() {
        return out;
    }
    let available = values
        .iter()
        .filter(|v| v["price_return"].as_f64().is_some())
        .count();
    out["available"] = json!(available);
    if available != values.len() {
        if values.iter().any(|v| {
            matches!(
                v["status"].as_str(),
                Some("data_missing" | "entry_unavailable")
            )
        }) {
            out["status"] = json!("data_missing");
        }
        return out;
    }
    let first = values[0];
    if values
        .iter()
        .any(|v| v["entry_date"] != first["entry_date"] || v["end_date"] != first["end_date"])
    {
        out["status"] = json!("data_missing");
        return out;
    }
    out["status"] = first["status"].clone();
    out["entry_date"] = first["entry_date"].clone();
    out["end_date"] = first["end_date"].clone();
    out["price_return"] = json!(
        values
            .iter()
            .map(|v| v["price_return"].as_f64().unwrap())
            .sum::<f64>()
            / values.len() as f64
    );
    // 個別銘柄の最大下落率の平均はポートフォリオの最大下落率ではないため算出しない。
    out
}

/// 毎日全期間を書き直さず、最新観測から値が変わった日だけ追記する。
pub fn market_insert(rows: &[(String, Date, f64, f64)], now: i64) -> Vec<Stmt> {
    rows.chunks(20).map(|chunk| {
        let mut params=Vec::new();
        for (code, date, close, factor) in chunk {
            params.extend([text(code),text(date.to_string()),Val::Real(*close),Val::Real(*factor),Val::Int(now)]);
        }
        stmt(&format!("INSERT OR IGNORE INTO evaluation_market_data(code,trade_date,close,split_factor,observed_at) VALUES {}",
            vec!["(?,?,?,?,?)";chunk.len()].join(",")), params)
    }).collect()
}

/// 元表が更新・訂正されても過去の配当／財務／方針を残す。取得時刻だけの変更は除く。
async fn capture_fundamentals(store: &D1Store, code: &str, now: i64) -> Result<(), PortError> {
    for (source, query) in [
        ("annual", "SELECT * FROM financials_annual WHERE code=? ORDER BY period_end,doc_id"),
        ("quarterly", "SELECT * FROM financials_quarterly WHERE code=? ORDER BY fiscal_year_end,period_type,doc_id"),
        ("dividend_forecast", "SELECT * FROM dividend_forecasts WHERE code=? ORDER BY disclosed,doc_id"),
        ("dividend_policy", "SELECT * FROM edinet_docs WHERE code=? ORDER BY doc_kind,doc_id"),
    ] {
        let mut rows = store.query(&stmt(query, vec![text(code)])).await?;
        for r in &mut rows {
            if let Some(o) = r.as_object_mut() { o.remove("fetched_at"); }
        }
        let payload = json!(rows).to_string();
        store.exec(&stmt(
            "INSERT OR IGNORE INTO recommendation_fundamentals(code,source,observed_at,payload) \
             SELECT ?,?,?,? WHERE COALESCE((SELECT payload FROM recommendation_fundamentals \
               WHERE code=? AND source=? ORDER BY id DESC LIMIT 1),'') != ?",
            vec![text(code),text(source),Val::Int(now),text(&payload),text(code),text(source),text(&payload)],
        )).await?;
    }
    Ok(())
}

pub async fn run(
    store: &D1Store,
    holidays: &[Date],
    as_of: Date,
    now: i64,
) -> Result<usize, PortError> {
    // 公開済みrunをページングで全件読む（履歴の古い推薦も消さない）。
    let mut runs = Vec::new();
    let mut after = 0;
    loop {
        let page=store.query(&stmt("SELECT id,published_at,price_date,model,prompt_version,rule_version,is_primary FROM recommendation_runs WHERE status='published' AND id>? ORDER BY id LIMIT 200",vec![Val::Int(after)])).await?;
        if page.is_empty() {
            break;
        }
        after = page.last().unwrap()["id"]
            .as_i64()
            .ok_or_else(|| PortError("invalid run id".into()))?;
        runs.extend(page);
    }
    if runs.is_empty() {
        return Ok(0);
    }
    let earliest = runs
        .iter()
        .filter_map(|r| r["published_at"].as_i64())
        .map(Date::from_unix_jst)
        .min()
        .ok_or_else(|| PortError("missing publication date".into()))?;
    let min_year = holidays.iter().map(|d| d.y).min().unwrap_or(i32::MAX);
    let max_year = holidays.iter().map(|d| d.y).max().unwrap_or(i32::MIN);
    if earliest.y < min_year || as_of.y > max_year {
        return Err(PortError(
            "holiday calendar does not cover evaluation dates".into(),
        ));
    }
    let calendar: Vec<_> = (earliest.to_days()..=as_of.to_days())
        .map(Date::from_days)
        .filter(|d| domain::calendar::is_trading_day(*d, holidays))
        .collect();
    let mut candidates: HashMap<i64, Vec<Value>> = HashMap::new();
    let mut codes = BTreeMap::<String, Date>::new();
    codes.insert("1306".into(), earliest);
    for run in &runs {
        let id = run["id"].as_i64().unwrap();
        let published = Date::from_unix_jst(run["published_at"].as_i64().unwrap());
        let rows=store.query(&stmt("SELECT code,rule_rank,rank FROM recommendation_candidates WHERE run_id=? ORDER BY rule_rank",vec![Val::Int(id)])).await?;
        for r in &rows {
            codes
                .entry(field(r, "code")?.into())
                .and_modify(|d| *d = (*d).min(published))
                .or_insert(published);
        }
        candidates.insert(id, rows);
    }
    let all_splits = store.load_splits().await?;
    let mut market = HashMap::new();
    let mut saved_splits = HashMap::new();
    for (code, since) in codes {
        capture_fundamentals(store, &code, now).await?;
        let mut existing = BTreeMap::new();
        let mut cursor = String::new();
        loop {
            let rows=store.query(&stmt(
                "SELECT m.trade_date,m.close,m.split_factor FROM evaluation_market_data m \
                 WHERE m.code=? AND m.trade_date>? AND m.id=(SELECT MAX(x.id) FROM evaluation_market_data x WHERE x.code=m.code AND x.trade_date=m.trade_date) \
                 ORDER BY m.trade_date LIMIT 500",vec![text(&code),text(&cursor)])).await?;
            if rows.is_empty() {
                break;
            }
            cursor = field(rows.last().unwrap(), "trade_date")?.into();
            for r in rows {
                let d = Date::parse(field(&r, "trade_date")?)
                    .ok_or_else(|| PortError("invalid price date".into()))?;
                existing.insert(
                    d,
                    (
                        r["close"].as_f64().unwrap(),
                        r["split_factor"].as_f64().unwrap(),
                    ),
                );
            }
        }
        let rows = store
            .query(&stmt(
                "SELECT closes FROM price_history WHERE code=?",
                vec![text(&code)],
            ))
            .await?;
        let bars = rows
            .first()
            .and_then(|r| r["closes"].as_str())
            .map(domain::price_history::decode)
            .unwrap_or_default();
        let splits = all_splits.get(&code).map(Vec::as_slice).unwrap_or(&[]);
        let mut changed = Vec::new();
        for b in bars
            .into_iter()
            .filter(|b| b.date > since && b.date <= as_of && b.close.is_finite() && b.close > 0.0)
        {
            let factor = splits
                .iter()
                .find(|s| s.date == b.date)
                .map_or(1.0, |s| s.factor);
            if existing.get(&b.date) != Some(&(b.close, factor)) {
                changed.push((code.clone(), b.date, b.close, factor));
                existing.insert(b.date, (b.close, factor));
            }
        }
        for q in market_insert(&changed, now) {
            store.exec(&q).await?;
        }
        let bars: Vec<_> = existing
            .iter()
            .filter(|(d, _)| **d <= as_of)
            .map(|(date, (close, _))| PriceBar {
                date: *date,
                close: *close,
            })
            .collect();
        let splits: Vec<_> = existing
            .iter()
            .filter_map(|(d, (_, f))| Split::new(*d, *f))
            .collect();
        market.insert(code.clone(), bars);
        saved_splits.insert(code, splits);
    }
    let revision = store
        .query(&stmt(
            "SELECT COALESCE(MAX(id),0) AS revision FROM evaluation_market_data",
            vec![],
        ))
        .await?[0]["revision"]
        .as_i64()
        .unwrap();
    let calendar_json =
        json!(calendar.iter().map(|d| d.to_string()).collect::<Vec<_>>()).to_string();
    let rows=store.query(&stmt("INSERT INTO evaluation_batches(created_at,as_of,source_revision,calendar_json,version) VALUES (?,?,?,?,?) RETURNING id",
        vec![Val::Int(now),text(as_of.to_string()),Val::Int(revision),text(calendar_json),text(VERSION)])).await?;
    let batch_id = rows[0]["id"].as_i64().unwrap();
    let mut count = 0;
    let mut report_cohorts = Vec::new();
    for mut run in runs {
        let id = run["id"].as_i64().unwrap();
        let published = Date::from_unix_jst(run["published_at"].as_i64().unwrap());
        let sessions: Vec<_> = calendar
            .iter()
            .copied()
            .filter(|d| *d > published)
            .take(253)
            .collect();
        let members = &candidates[&id];
        let mut outcomes = Vec::new();
        for horizon in HORIZONS {
            let mut results = HashMap::new();
            for code in members
                .iter()
                .map(|r| r["code"].as_str().unwrap())
                .chain(std::iter::once("1306"))
            {
                let bars = market.get(code).map(Vec::as_slice).unwrap_or(&[]);
                let splits = saved_splits.get(code).map(Vec::as_slice).unwrap_or(&[]);
                let result = result_json(&evaluate(bars, splits, &sessions, horizon));
                outcomes.push((code.to_string(), horizon, result.clone()));
                results.insert(code.to_string(), result);
            }
            for group in ["__ai10", "__rule10", "__candidates"] {
                let selected: Vec<_> = members
                    .iter()
                    .filter(|r| match group {
                        "__ai10" => r["rank"].as_i64().is_some(),
                        "__rule10" => r["rule_rank"].as_i64().is_some_and(|rank| rank <= 10),
                        _ => true,
                    })
                    .map(|r| &results[r["code"].as_str().unwrap()])
                    .collect();
                outcomes.push((group.into(), horizon, aggregate(&selected)));
            }
        }
        if run["is_primary"].as_i64() == Some(1) {
            let summary: Vec<_> = HORIZONS
                .iter()
                .filter(|h| **h > 0)
                .map(|h| {
                    let find = |code: &str| {
                        outcomes
                            .iter()
                            .find(|(c, period, _)| c == code && period == h)
                            .map(|(_, _, v)| v.clone())
                            .unwrap_or(Value::Null)
                    };
                    json!({"horizon":h,"ai":find("__ai10"),"rule":find("__rule10")})
                })
                .collect();
            run["summary"] = json!(summary);
            run["selected_codes"] = json!(members
                .iter()
                .filter(|c| c["rank"].as_i64().is_some())
                .map(|c| c["code"].clone())
                .collect::<Vec<_>>());
            report_cohorts.push(run.clone());
        }
        // JSONが変わった評価だけ追記する。同じ日の再実行や満期後に行数を増やさない。
        let old=store.query(&stmt(
            "SELECT o.code,o.horizon,o.result_json FROM recommendation_outcomes o \
             WHERE o.run_id=? AND o.batch_id=(SELECT MAX(x.batch_id) FROM recommendation_outcomes x \
             JOIN evaluation_batches b ON b.id=x.batch_id AND b.status='complete' \
             WHERE x.run_id=o.run_id AND x.code=o.code AND x.horizon=o.horizon)",vec![Val::Int(id)])).await?;
        let previous: HashMap<_, _> = old
            .iter()
            .map(|r| {
                (
                    (
                        r["code"].as_str().unwrap().to_string(),
                        r["horizon"].as_u64().unwrap() as usize,
                    ),
                    r["result_json"].as_str().unwrap(),
                )
            })
            .collect();
        let changed: Vec<_> = outcomes
            .into_iter()
            .filter(|(code, horizon, result)| {
                previous.get(&(code.clone(), *horizon)).copied()
                    != Some(result.to_string().as_str())
            })
            .collect();
        for chunk in changed.chunks(20) {
            let mut params = Vec::new();
            for (code, horizon, result) in chunk {
                params.extend([
                    Val::Int(id),
                    text(code),
                    Val::Int(*horizon as i64),
                    Val::Int(batch_id),
                    text(result.to_string()),
                ]);
            }
            store.exec(&stmt(&format!("INSERT OR IGNORE INTO recommendation_outcomes(run_id,code,horizon,batch_id,result_json) VALUES {}",
                vec!["(?,?,?,?,?)";chunk.len()].join(",")),params)).await?;
            count += chunk.len();
        }
    }
    store
        .exec(&stmt(
            "UPDATE evaluation_batches SET status='complete',report_json=? WHERE id=?",
            vec![
                text(crate::recommendation_report::build(&report_cohorts).to_string()),
                Val::Int(batch_id),
            ],
        ))
        .await?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_requires_full_cohort_and_same_dates() {
        let a = json!({"status":"complete","price_return":0.1,"entry_date":"2026-10-26","end_date":"2026-11-26"});
        let b = json!({"status":"complete","price_return":-0.2,"entry_date":"2026-10-26","end_date":"2026-11-26"});
        assert!((aggregate(&[&a, &b])["price_return"].as_f64().unwrap() + 0.05).abs() < 1e-12);
        assert_eq!(
            aggregate(&[&a, &json!({"status":"data_missing"})])["price_return"],
            Value::Null
        );
        assert_eq!(
            aggregate(&[&a, &json!({"status":"complete","price_return":0.5})])["price_return"],
            Value::Null
        );
        assert_eq!(aggregate(&[&a, &b])["max_drawdown"], Value::Null);
    }
    #[test]
    fn market_writes_are_bounded() {
        let rows = vec![("7203".into(), Date::new(2026, 10, 26), 100.0, 1.0); 41];
        let stmts = market_insert(&rows, 1);
        assert_eq!(stmts.len(), 3);
        assert!(stmts.iter().all(|s| s.params.len() <= 100));
    }
}
