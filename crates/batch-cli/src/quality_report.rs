//! `quality-report`: 財務品質スコアの導入計測（docs/19 適用手順4）。
//! D1 を読むだけで書き込まない。業種別の財務JSON取得率と、新旧ルール
//! （quality_score あり/なし）の順位差分を出し、配点調整の判断材料にする。

use adapter_batch::{d1, sql::Stmt};
use adapter_cf::query;
use domain::recommend::{select_candidates, Candidate, PICK_COUNT};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use usecase::recommend::candidates_of;
use usecase::screening::StockRow;

/// indicators_latest × stocks の1行。recommend_candidates が返す列だけ。
/// adapter-cf::d1 の Row と同じ対応（worker 側の型は wasm 専用なのでここで組み直す。
/// SQLite の真偽値は 0/1 の整数）。
#[derive(Deserialize)]
struct Row {
    financial_quality_json: Option<String>,
    code: String,
    price: f64,
    price_date: String,
    dividend_ttm: f64,
    dividend_basis: Option<String>,
    dividend_yield: Option<f64>,
    dividend_total: Option<f64>,
    dividend_special: Option<f64>,
    dividend_commemorative: Option<f64>,
    dividend_source: Option<String>,
    yield_percentile: Option<f64>,
    pbr: Option<f64>,
    per: Option<f64>,
    payout_ratio: Option<f64>,
    doe: Option<f64>,
    consecutive_increases: Option<i64>,
    operating_margin: Option<f64>,
    no_loss_years: Option<i64>,
    profit_uptrend: Option<i64>,
    valuation: String,
    fetched_at: i64,
    name: Option<String>,
    kind: String,
    market: Option<String>,
    sector33: Option<String>,
    size_class: Option<String>,
    dividend_every_year: Option<i64>,
    no_quarterly_loss: Option<i64>,
    is_value: Option<i64>,
    data_years: Option<i64>,
    sales_growth: Option<f64>,
    op_growth: Option<f64>,
    dividend_cut: Option<i64>,
    progressive_dividend: Option<i64>,
    no_dividend_cut: Option<i64>,
    doe_floor: Option<f64>,
    doe_goal: Option<f64>,
    payout_floor: Option<f64>,
    payout_goal: Option<f64>,
    op_growth_q: Option<f64>,
    net_growth_q: Option<f64>,
    op_growth_ttm: Option<f64>,
    profit_one_off: Option<i64>,
}

/// adapter-cf::d1 の From<Row> と同じ変換（reit 列は候補クエリにないので None）。
fn to_stock_row(r: Row) -> StockRow {
    let forecast = r.dividend_basis.as_deref() == Some("forecast");
    let doe_forecast = forecast
        .then(|| {
            domain::indicators::doe_from_yield(
                r.dividend_total
                    .filter(|_| r.price > 0.0)
                    .map(|d| d / r.price),
                r.pbr,
            )
        })
        .flatten();
    let payout_forecast = forecast
        .then(|| {
            domain::indicators::payout_from_yield(
                r.dividend_total
                    .filter(|_| r.price > 0.0)
                    .map(|d| d / r.price),
                r.per,
            )
        })
        .flatten();
    StockRow {
        financial_quality: r
            .financial_quality_json
            .as_deref()
            .and_then(adapter_cf::recommend::parse_financial_quality),
        code: r.code,
        price: r.price,
        price_date: r.price_date,
        dividend: r.dividend_yield.map(|_| r.dividend_ttm),
        dividend_total: r.dividend_total,
        dividend_special: r.dividend_special,
        dividend_commemorative: r.dividend_commemorative,
        dividend_source: r.dividend_source,
        dividend_basis: r.dividend_basis,
        dividend_yield: r.dividend_yield,
        yield_percentile: r.yield_percentile,
        pbr: r.pbr,
        per: r.per,
        payout_ratio: r.payout_ratio,
        doe: r.doe,
        consecutive_increases: r.consecutive_increases,
        operating_margin: r.operating_margin,
        no_loss_years: r.no_loss_years.map(|v| v != 0),
        profit_uptrend: r.profit_uptrend.map(|v| v != 0),
        valuation: r.valuation,
        fetched_at: r.fetched_at,
        name: r.name,
        kind: r.kind,
        market: r.market,
        sector33: r.sector33,
        size_class: r.size_class,
        dividend_every_year: r.dividend_every_year.map(|v| v != 0),
        no_quarterly_loss: r.no_quarterly_loss.map(|v| v != 0),
        is_value: r.is_value.map(|v| v != 0),
        data_years: r.data_years,
        sales_growth: r.sales_growth,
        op_growth: r.op_growth,
        dividend_cut: r.dividend_cut.map(|v| v != 0),
        progressive_dividend: r.progressive_dividend.map(|v| v != 0),
        no_dividend_cut: r.no_dividend_cut.map(|v| v != 0),
        doe_floor: r.doe_floor,
        doe_goal: r.doe_goal,
        doe_forecast,
        payout_floor: r.payout_floor,
        payout_goal: r.payout_goal,
        payout_forecast,
        op_growth_q: r.op_growth_q,
        net_growth_q: r.net_growth_q,
        op_growth_ttm: r.op_growth_ttm,
        profit_one_off: r.profit_one_off.map(|v| v != 0),
        reit: None,
    }
}

/// 全上場株式の財務JSON取得率（業種別）。鮮度の条件は推薦クエリと同じ
/// （price_date を基準に提出済み・18ヶ月以内の年次書類）。
const COVERAGE_SQL: &str =
    "SELECT COALESCE(s.sector33, '(不明)') AS sector, COUNT(*) AS stocks, \
     SUM(CASE WHEN EXISTS (SELECT 1 FROM edinet_docs e WHERE e.code = i.code AND e.doc_kind = 'annual' \
       AND e.submitted <= i.price_date AND e.period_end <= i.price_date \
       AND e.period_end >= date(i.price_date, '-18 months') \
       AND e.financial_quality_json IS NOT NULL) THEN 1 ELSE 0 END) AS covered \
     FROM indicators_latest i LEFT JOIN stocks s ON s.code = i.code \
     WHERE COALESCE(s.kind, 'stock') = 'stock' \
     GROUP BY sector ORDER BY stocks DESC, sector";

fn i64_of(v: &Value, key: &str) -> i64 {
    v[key].as_f64().unwrap_or(0.0) as i64
}

fn pct(n: i64, d: i64) -> String {
    if d == 0 {
        "-".into()
    } else {
        format!("{:.1}%", n as f64 * 100.0 / d as f64)
    }
}

fn label_of(c: &Candidate, meta: &HashMap<&str, (Option<String>, Option<String>)>) -> String {
    let (name, sector) = meta.get(c.code.as_str()).cloned().unwrap_or((None, None));
    format!(
        "{} {} [{}] grade={} score={} q={}",
        c.code,
        name.unwrap_or_else(|| "-".into()),
        sector.unwrap_or_else(|| "-".into()),
        c.grade().as_str(),
        c.score(),
        c.quality_score
    )
}

pub async fn run(store: &d1::D1Store, out: Option<&str>) -> Result<(), String> {
    let mut lines: Vec<String> = vec![
        "財務品質スコア計測レポート（ルール版 graded-dividend-financial-v3）".into(),
        "".into(),
        "■ 財務JSON取得率（上場株式・業種別）".into(),
        "業種\t銘柄数\t取得\t率".into(),
    ];

    let cov = store
        .query(&Stmt {
            sql: COVERAGE_SQL.into(),
            params: vec![],
        })
        .await
        .map_err(|e| e.0)?;
    let (mut all_stocks, mut all_covered) = (0i64, 0i64);
    for r in &cov {
        let (stocks, covered) = (i64_of(r, "stocks"), i64_of(r, "covered"));
        all_stocks += stocks;
        all_covered += covered;
        lines.push(format!(
            "{}\t{}\t{}\t{}",
            r["sector"].as_str().unwrap_or("(不明)"),
            stocks,
            covered,
            pct(covered, stocks)
        ));
    }
    lines.push(format!(
        "合計\t{}\t{}\t{}",
        all_stocks,
        all_covered,
        pct(all_covered, all_stocks)
    ));

    let theme_rows = store
        .query(&Stmt {
            sql: query::STOCK_THEME_NAMES.into(),
            params: vec![],
        })
        .await
        .map_err(|e| e.0)?;
    let themes: Vec<(String, String)> = theme_rows
        .iter()
        .filter_map(|r| {
            Some((
                r["code"].as_str()?.to_string(),
                r["name"].as_str()?.to_string(),
            ))
        })
        .collect();

    let raw = store
        .query(&Stmt {
            sql: query::recommend_candidates(),
            params: vec![],
        })
        .await
        .map_err(|e| e.0)?;
    let mut rows = Vec::with_capacity(raw.len());
    for v in raw {
        let row: Row = serde_json::from_value(v).map_err(|e| format!("candidate row: {e}"))?;
        rows.push(to_stock_row(row));
    }

    let meta: HashMap<&str, (Option<String>, Option<String>)> = rows
        .iter()
        .map(|r| (r.code.as_str(), (r.name.clone(), r.sector33.clone())))
        .collect();

    // 新ルールの順位（candidates_of の結果）と、quality_score を 0 にした旧ルールの順位
    let new = candidates_of(&rows, &themes);
    let old = {
        let mut c = new.clone();
        for x in &mut c {
            x.quality_score = 0;
        }
        select_candidates(&c)
    };

    let with_fin = new
        .iter()
        .filter(|c| {
            rows.iter()
                .any(|r| r.code == c.code && r.financial_quality.is_some())
        })
        .count();
    lines.push("".into());
    lines.push(format!(
        "■ 候補（eligible {}件、うち財務JSON取得済み {}件 = {}）",
        new.len(),
        with_fin,
        pct(with_fin as i64, new.len() as i64)
    ));
    let mut hist = BTreeMap::new();
    for c in &new {
        *hist.entry(c.quality_score).or_insert(0usize) += 1;
    }
    lines.push(format!(
        "スコア分布: {}",
        hist.iter()
            .map(|(s, n)| format!("{s}:{n}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let mut by_sector: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for c in &new {
        let has_fin = rows
            .iter()
            .any(|r| r.code == c.code && r.financial_quality.is_some());
        let sector = meta
            .get(c.code.as_str())
            .and_then(|(_, s)| s.clone())
            .unwrap_or_else(|| "(不明)".into());
        let e = by_sector.entry(sector).or_default();
        e.0 += 1;
        e.1 += has_fin as i64;
    }
    lines.push("業種\t候補\t取得\t率".into());
    for (sector, (n, covered)) in &by_sector {
        lines.push(format!("{sector}\t{n}\t{covered}\t{}", pct(*covered, *n)));
    }

    lines.push("".into());
    lines.push(format!("■ 順位差分（上位{PICK_COUNT}件）"));
    lines.push("旧（財務スコアなし）:".into());
    for (i, c) in old.iter().take(PICK_COUNT).enumerate() {
        lines.push(format!("  {}. {}", i + 1, label_of(c, &meta)));
    }
    lines.push("新（財務スコアあり）:".into());
    for (i, c) in new.iter().take(PICK_COUNT).enumerate() {
        lines.push(format!("  {}. {}", i + 1, label_of(c, &meta)));
    }
    let old_top: Vec<&str> = old
        .iter()
        .take(PICK_COUNT)
        .map(|c| c.code.as_str())
        .collect();
    let new_top: Vec<&str> = new
        .iter()
        .take(PICK_COUNT)
        .map(|c| c.code.as_str())
        .collect();
    let entered: Vec<&str> = new_top
        .iter()
        .copied()
        .filter(|c| !old_top.contains(c))
        .collect();
    let exited: Vec<&str> = old_top
        .iter()
        .copied()
        .filter(|c| !new_top.contains(c))
        .collect();
    lines.push(format!(
        "ランクイン: {}",
        if entered.is_empty() {
            "なし".into()
        } else {
            entered.join(" ")
        }
    ));
    lines.push(format!(
        "圏外: {}",
        if exited.is_empty() {
            "なし".into()
        } else {
            exited.join(" ")
        }
    ));

    let text = lines.join("\n") + "\n";
    match out {
        Some(p) => std::fs::write(p, &text).map_err(|e| format!("write {p}: {e}")),
        None => {
            print!("{text}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROW_JSON: &str = r#"{
        "financial_quality_json": null,
        "code": "1234", "price": 1000.0, "price_date": "2026-10-29",
        "dividend_ttm": 30.0, "dividend_basis": null, "dividend_yield": 0.03,
        "dividend_total": null, "dividend_special": null, "dividend_commemorative": null,
        "dividend_source": null, "yield_percentile": null, "pbr": 1.0, "per": 12.0,
        "payout_ratio": 0.3, "doe": null, "consecutive_increases": 2,
        "operating_margin": null, "no_loss_years": 1, "profit_uptrend": 0,
        "valuation": "cheap", "fetched_at": 1, "name": "テスト商事",
        "kind": "stock", "market": "プライム", "sector33": "卸売業",
        "size_class": "large", "dividend_every_year": 1, "no_quarterly_loss": 1,
        "is_value": 1, "data_years": 5, "sales_growth": null, "op_growth": null,
        "dividend_cut": 0, "progressive_dividend": 1, "no_dividend_cut": 0,
        "doe_floor": null, "doe_goal": null, "payout_floor": null, "payout_goal": null,
        "op_growth_q": null, "net_growth_q": null, "op_growth_ttm": null,
        "profit_one_off": 0
    }"#;

    fn row_json(extra: Value) -> Value {
        let mut base: Value = serde_json::from_str(ROW_JSON).unwrap();
        if let (Value::Object(b), Value::Object(e)) = (&mut base, extra) {
            b.extend(e);
        }
        base
    }

    #[test]
    fn maps_row_to_stock_row() {
        let r: Row = serde_json::from_value(row_json(json!({}))).unwrap();
        let s = to_stock_row(r);
        assert_eq!(s.code, "1234");
        assert_eq!(s.dividend, Some(30.0));
        assert_eq!(s.no_loss_years, Some(true));
        assert_eq!(s.profit_uptrend, Some(false));
        assert_eq!(s.dividend_cut, Some(false));
        assert_eq!(s.sector33.as_deref(), Some("卸売業"));
        assert!(s.financial_quality.is_none());
        assert!(s.reit.is_none());
    }

    #[test]
    fn maps_financial_quality_json() {
        let fq = json!({
            "version": 1, "source": "EDINET", "doc_id": "S1",
            "period_end": "2026-03-31", "scope": "連結",
            "cash": 100.0, "debt": 10.0, "liabilities": 50.0, "assets": 200.0,
            "equity": 150.0, "operating_cf": 20.0, "positive_operating_cf_3y": true,
            "investing_cf": -5.0, "dividends_paid": 8.0
        });
        let r: Row =
            serde_json::from_value(row_json(json!({"financial_quality_json": fq.to_string()})))
                .unwrap();
        let s = to_stock_row(r);
        let f = s.financial_quality.unwrap();
        assert_eq!(f.cash, Some(100.0));
        assert_eq!(f.financial_score(), 3);
    }
}
