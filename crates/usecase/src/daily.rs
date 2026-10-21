//! 日次の指標計算。保存済みの株価の履歴と年次財務から、全銘柄の指標を作る（取得はしない）。

use crate::Snapshot;
use domain::{
    compute, AnnualFinancial, Date, Dividend, FinancialData, MarketData, PriceBar, StockCode,
};
use std::collections::HashMap;

/// 年次財務の1件と、その開示日（配当をいつから使えるか）
#[derive(Debug, Clone, PartialEq)]
pub struct StoredAnnual {
    pub doc_id: String,
    pub disclosed: Option<Date>,
    pub financial: AnnualFinancial,
}

/// 開示日が分からないときは、決算期末から45日後（決算短信の目安）に分かったとみなす
const ASSUMED_DISCLOSURE_LAG_DAYS: i64 = 45;
/// 指標に使う年次財務の期数
const MAX_YEARS: usize = 5;

fn effective_date(a: &StoredAnnual) -> Option<Date> {
    a.disclosed.or_else(|| {
        a.financial
            .period_end
            .map(|p| p.add_days(ASSUMED_DISCLOSURE_LAG_DAYS))
    })
}

/// 1銘柄分の入力を組み立てる
pub fn build_inputs(
    bars: Vec<PriceBar>,
    annuals: &[StoredAnnual],
) -> (MarketData, Option<FinancialData>, Option<&StoredAnnual>) {
    let mut sorted: Vec<&StoredAnnual> = annuals
        .iter()
        .filter(|a| a.financial.period_end.is_some())
        .collect();
    // 新しい期が先頭
    sorted.sort_by_key(|a| std::cmp::Reverse(a.financial.period_end));
    let dividends = sorted
        .iter()
        .filter_map(|a| {
            Some(Dividend {
                effective: effective_date(a)?,
                annual: a.financial.dps?,
            })
        })
        .collect();
    let latest = sorted.first().copied();
    let fin = latest.map(|l| FinancialData {
        doc_id: l.doc_id.clone(),
        submitted: l.disclosed,
        years: sorted
            .iter()
            .take(MAX_YEARS)
            .map(|a| a.financial.clone())
            .collect(),
    });
    (MarketData { bars, dividends }, fin, latest)
}

/// 全銘柄の指標を計算する。株価の履歴がない銘柄は飛ばす
pub fn compute_all(
    codes: &[StockCode],
    mut histories: HashMap<String, Vec<PriceBar>>,
    annuals: &HashMap<String, Vec<StoredAnnual>>,
    run_id: &str,
    now: i64,
) -> (Vec<Snapshot>, usize) {
    let mut snaps = vec![];
    let mut skipped = 0;
    let empty = vec![];
    for code in codes {
        let bars = histories.remove(code.as_str()).unwrap_or_default();
        let (market, fin, latest) =
            build_inputs(bars, annuals.get(code.as_str()).unwrap_or(&empty));
        match compute(&market, fin.as_ref()) {
            Ok(indicators) => snaps.push(Snapshot {
                code: code.clone(),
                run_id: run_id.to_string(),
                fetched_at: now,
                indicators,
                fin_doc_id: latest.map(|l| l.doc_id.clone()),
                fin_submitted: latest.and_then(|l| l.disclosed),
            }),
            Err(_) => skipped += 1,
        }
    }
    (snaps, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn annual(period_end: &str, disclosed: Option<&str>, dps: f64, eps: f64) -> StoredAnnual {
        StoredAnnual {
            doc_id: format!("doc-{period_end}"),
            disclosed: disclosed.map(d),
            financial: AnnualFinancial {
                period_end: Some(d(period_end)),
                dps: Some(dps),
                eps: Some(eps),
                ..Default::default()
            },
        }
    }

    #[test]
    fn dividends_start_from_disclosure() {
        let annuals = [
            annual("2025-03-31", Some("2025-05-10"), 40.0, 100.0),
            annual("2026-03-31", None, 44.0, 110.0),
        ];
        let (m, fin, latest) = build_inputs(vec![], &annuals);
        assert_eq!(m.dividends.len(), 2);
        assert_eq!(
            m.dividends[0].effective,
            d("2026-05-15"),
            "開示日がなければ期末+45日"
        );
        assert_eq!(m.dividends[1].effective, d("2025-05-10"));
        assert_eq!(fin.unwrap().years[0].dps, Some(44.0), "新しい期が先頭");
        assert_eq!(latest.unwrap().doc_id, "doc-2026-03-31");
    }

    #[test]
    fn computes_yield_from_disclosed_dividend() {
        let bars: Vec<PriceBar> = (0..10)
            .map(|i| PriceBar {
                date: d("2026-07-01").add_days(i),
                close: 1000.0,
            })
            .collect();
        let code = StockCode::parse("7203").unwrap();
        let mut hist = HashMap::new();
        hist.insert("7203".to_string(), bars);
        let mut ann = HashMap::new();
        ann.insert(
            "7203".to_string(),
            vec![annual("2026-03-31", Some("2026-05-10"), 40.0, 100.0)],
        );
        let missing = StockCode::parse("9999").unwrap();
        let (snaps, skipped) = compute_all(&[code, missing], hist, &ann, "r1", 0);
        assert_eq!(skipped, 1, "株価の履歴がない銘柄は飛ばす");
        let i = &snaps[0].indicators;
        assert!((i.dividend_yield.unwrap() - 0.04).abs() < 1e-9);
        assert!((i.payout_ratio.unwrap() - 0.4).abs() < 1e-9);
        assert_eq!(snaps[0].fin_doc_id.as_deref(), Some("doc-2026-03-31"));
    }
}
