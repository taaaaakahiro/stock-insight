//! 日次の指標計算。保存済みの株価の履歴と財務から、全銘柄の指標を作る（取得はしない）。

use crate::Snapshot;
use domain::analysis::{
    standalone_quarters, yoy_growth, CumulativePeriod, PeriodType, QuarterResult,
};
use domain::split::{adjust_bars, factor_after, Split};
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

/// 本決算の行を、四半期の差分計算に使う累計値（FY）の形にする
fn annual_as_period(a: &StoredAnnual) -> Option<CumulativePeriod> {
    let end = a.financial.period_end?;
    Some(CumulativePeriod {
        fiscal_year_end: end,
        period_end: end,
        period_type: PeriodType::FY,
        net_sales: a.financial.net_sales,
        operating_income: a.financial.operating_income,
        ordinary_income: a.financial.ordinary_income,
        net_income: a.financial.net_income,
    })
}

/// 1銘柄分の入力を組み立てる: 株価と配当、年次財務（新しい期が先頭）、四半期単独の損益（古い順）。
/// 株式分割があれば、終値と1株当たりの値（配当・EPS・BPS）を最新の株数の基準にそろえる
/// （1株当たりの値は、開示した日より後の分割の分だけ直す）
pub fn build_inputs(
    mut bars: Vec<PriceBar>,
    annuals: &[StoredAnnual],
    quarterlies: &[CumulativePeriod],
    splits: &[Split],
) -> (
    MarketData,
    Option<FinancialData>,
    Vec<QuarterResult>,
    Option<usize>,
) {
    adjust_bars(&mut bars, splits);
    let adjusted: Vec<StoredAnnual> = annuals
        .iter()
        .map(|a| {
            let f = effective_date(a).map_or(1.0, |e| factor_after(splits, e));
            let mut a = a.clone();
            if f != 1.0 {
                let per_share = |v: Option<f64>| v.map(|x| x * f);
                a.financial.eps = per_share(a.financial.eps);
                a.financial.bps = per_share(a.financial.bps);
                a.financial.dps = per_share(a.financial.dps);
            }
            a
        })
        .collect();
    let mut sorted: Vec<(usize, &StoredAnnual)> = adjusted
        .iter()
        .enumerate()
        .filter(|(_, a)| a.financial.period_end.is_some())
        .collect();
    // 新しい期が先頭
    sorted.sort_by_key(|(_, a)| std::cmp::Reverse(a.financial.period_end));
    let dividends = sorted
        .iter()
        .filter_map(|(_, a)| {
            Some(Dividend {
                effective: effective_date(a)?,
                annual: a.financial.dps?,
            })
        })
        .collect();
    let latest = sorted.first().map(|(i, _)| *i);
    let fin = sorted.first().map(|(_, l)| FinancialData {
        doc_id: l.doc_id.clone(),
        submitted: l.disclosed,
        years: sorted
            .iter()
            .take(MAX_YEARS)
            .map(|(_, a)| a.financial.clone())
            .collect(),
    });
    let mut periods: Vec<CumulativePeriod> = quarterlies.to_vec();
    periods.extend(sorted.iter().filter_map(|(_, a)| annual_as_period(a)));
    let quarters = standalone_quarters(&periods);
    (MarketData { bars, dividends }, fin, quarters, latest)
}

/// 全銘柄の指標を計算する。株価の履歴がない銘柄は飛ばす
pub fn compute_all(
    codes: &[StockCode],
    mut histories: HashMap<String, Vec<PriceBar>>,
    annuals: &HashMap<String, Vec<StoredAnnual>>,
    quarterlies: &HashMap<String, Vec<CumulativePeriod>>,
    splits: &HashMap<String, Vec<Split>>,
    run_id: &str,
    now: i64,
) -> (Vec<Snapshot>, usize) {
    let mut snaps = vec![];
    let mut skipped = 0;
    let no_annuals = vec![];
    let no_quarters = vec![];
    let no_splits = vec![];
    for code in codes {
        let bars = histories.remove(code.as_str()).unwrap_or_default();
        let anns = annuals.get(code.as_str()).unwrap_or(&no_annuals);
        let (market, fin, quarters, latest) = build_inputs(
            bars,
            anns,
            quarterlies.get(code.as_str()).unwrap_or(&no_quarters),
            splits.get(code.as_str()).unwrap_or(&no_splits),
        );
        let latest = latest.map(|i| &anns[i]);
        match compute(&market, fin.as_ref(), &quarters) {
            Ok(mut indicators) => {
                let mut periods = quarterlies.get(code.as_str()).cloned().unwrap_or_default();
                periods.extend(anns.iter().filter_map(annual_as_period));
                (indicators.sales_growth, indicators.op_growth) = yoy_growth(&periods);
                snaps.push(Snapshot {
                    code: code.clone(),
                    run_id: run_id.to_string(),
                    fetched_at: now,
                    indicators,
                    fin_doc_id: latest.map(|l| l.doc_id.clone()),
                    fin_submitted: latest.and_then(|l| l.disclosed),
                })
            }
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
                net_income: Some(100.0),
                ..Default::default()
            },
        }
    }

    fn quarter(fy: &str, pe: &str, t: PeriodType, net: f64) -> CumulativePeriod {
        CumulativePeriod {
            fiscal_year_end: d(fy),
            period_end: d(pe),
            period_type: t,
            net_sales: None,
            operating_income: None,
            ordinary_income: None,
            net_income: Some(net),
        }
    }

    #[test]
    fn dividends_start_from_disclosure() {
        let annuals = [
            annual("2025-03-31", Some("2025-05-10"), 40.0, 100.0),
            annual("2026-03-31", None, 44.0, 110.0),
        ];
        let (m, fin, _, latest) = build_inputs(vec![], &annuals, &[], &[]);
        assert_eq!(m.dividends.len(), 2);
        assert_eq!(
            m.dividends[0].effective,
            d("2026-05-15"),
            "開示日がなければ期末+45日"
        );
        assert_eq!(m.dividends[1].effective, d("2025-05-10"));
        assert_eq!(fin.unwrap().years[0].dps, Some(44.0), "新しい期が先頭");
        assert_eq!(latest, Some(1));
    }

    #[test]
    fn fourth_quarter_comes_from_annual_minus_3q() {
        let annuals = [annual("2026-03-31", Some("2026-05-10"), 44.0, 110.0)];
        let qs = [
            quarter("2026-03-31", "2025-06-30", PeriodType::Q1, 20.0),
            quarter("2026-03-31", "2025-09-30", PeriodType::Q2, 50.0),
            quarter("2026-03-31", "2025-12-31", PeriodType::Q3, 70.0),
        ];
        let (_, _, quarters, _) = build_inputs(vec![], &annuals, &qs, &[]);
        let nets: Vec<Option<f64>> = quarters.iter().map(|q| q.net_income).collect();
        assert_eq!(nets, vec![Some(20.0), Some(30.0), Some(20.0), Some(30.0)]);
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
        let (snaps, skipped) = compute_all(
            &[code, missing],
            hist,
            &ann,
            &HashMap::new(),
            &HashMap::new(),
            "r1",
            0,
        );
        assert_eq!(skipped, 1, "株価の履歴がない銘柄は飛ばす");
        let i = &snaps[0].indicators;
        assert!((i.dividend_yield.unwrap() - 0.04).abs() < 1e-9);
        assert!((i.payout_ratio.unwrap() - 0.4).abs() < 1e-9);
        assert_eq!(snaps[0].fin_doc_id.as_deref(), Some("doc-2026-03-31"));
    }

    #[test]
    fn split_after_disclosure_does_not_inflate_the_yield() {
        // 2026-05-10 に1株配当40円・EPS100円を開示、2026-09-29 に1:2の分割。株価は 1000円 → 500円
        let bars: Vec<PriceBar> = ["2026-09-25", "2026-09-28", "2026-09-29", "2026-09-30"]
            .iter()
            .zip([1000.0, 1000.0, 500.0, 500.0])
            .map(|(s, close)| PriceBar { date: d(s), close })
            .collect();
        let code = StockCode::parse("8766").unwrap();
        let hist = HashMap::from([("8766".to_string(), bars.clone())]);
        let ann = HashMap::from([(
            "8766".to_string(),
            vec![annual("2026-03-31", Some("2026-05-10"), 40.0, 100.0)],
        )]);
        let splits = HashMap::from([(
            "8766".to_string(),
            vec![Split::new(d("2026-09-29"), 0.5).unwrap()],
        )]);
        let (snaps, _) = compute_all(
            std::slice::from_ref(&code),
            hist.clone(),
            &ann,
            &HashMap::new(),
            &splits,
            "r1",
            0,
        );
        let i = &snaps[0].indicators;
        assert_eq!(i.price, 500.0, "最新の終値はそのまま");
        assert!(
            (i.dividend_yield.unwrap() - 0.04).abs() < 1e-9,
            "20円 ÷ 500円"
        );
        assert!((i.per.unwrap() - 10.0).abs() < 1e-9, "500円 ÷ EPS 50円");
        assert!((i.payout_ratio.unwrap() - 0.4).abs() < 1e-9);

        let (unadjusted, _) = compute_all(
            &[code],
            hist,
            &ann,
            &HashMap::new(),
            &HashMap::new(),
            "r1",
            0,
        );
        assert!(
            (unadjusted[0].indicators.dividend_yield.unwrap() - 0.08).abs() < 1e-9,
            "分割を知らなければ 40円 ÷ 500円 で2倍に出る"
        );
    }
}
