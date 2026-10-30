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
    pub breakdown: Option<domain::DividendBreakdown>,
}

/// 会社予想の配当の開示1件（dps が None は「予想なし」）
#[derive(Debug, Clone, PartialEq)]
pub struct StoredForecast {
    pub doc_id: String,
    pub disclosed: Date,
    /// 予想の対象の事業年度の終了日（分からなければ None）
    pub fiscal_year_end: Option<Date>,
    pub dps: Option<f64>,
    pub breakdown: Option<domain::DividendBreakdown>,
}

/// 開示日が分からないときは、決算期末から45日後（決算短信の目安）に分かったとみなす
const ASSUMED_DISCLOSURE_LAG_DAYS: i64 = 45;
/// 指標に使う年次財務の期数
const MAX_YEARS: usize = 5;
/// 期末の配当の権利落ち日は期末の数営業日前。期末の1週間前より後に権利落ちした分割は、その期の配当に入っていない
const RECORD_DATE_MARGIN_DAYS: i64 = 7;

/// 事業年度の配当が、どの日の株数の基準か。配当は期末の株主に払うので、期末の権利落ち日の株数
/// （期末の翌日に効力が出る分割は、権利落ちが期末より前でも配当には入っていない）。
/// 期の途中で分割すると中間配当は分割前・期末配当は分割後になるが、そこは分割後とみなす
fn dividend_basis(fiscal_year_end: Date) -> Date {
    fiscal_year_end.add_days(-RECORD_DATE_MARGIN_DAYS)
}

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

/// 利回りに使う配当の移り変わり（新しい順）。実績・予想のどちらかの開示があった日ごとに、
/// 会社予想の年間配当総額を優先する。予想未定なら最新実績を使う
fn dividend_series(actuals: &[Dividend], forecasts: &[StoredForecast]) -> Vec<Dividend> {
    let mut days: Vec<Date> = actuals
        .iter()
        .map(|a| a.effective)
        .chain(
            actuals
                .iter()
                .filter_map(|a| a.breakdown.as_ref().and_then(|b| b.known_since)),
        )
        .chain(forecasts.iter().map(|f| f.disclosed))
        .collect();
    days.sort();
    days.dedup();
    days.into_iter()
        .rev()
        .filter_map(|day| {
            // 過年度の訂正開示で今期予想を上書きしない。同じ対象年度では最新開示を使う。
            // 最新年度が未定の場合も、過年度の予想に戻らず実績に切り替える。
            let forecast = forecasts
                .iter()
                .filter(|f| f.disclosed <= day)
                .max_by_key(|f| (f.fiscal_year_end, f.disclosed, &f.doc_id))
                .filter(|f| f.dps.is_some());
            let actual = domain::dividend_at(actuals, day);
            match forecast {
                Some(f) => Some(Dividend {
                    effective: day,
                    annual: f.dps.unwrap(),
                    breakdown: f.breakdown.clone(),
                    forecast: true,
                }),
                None => actual.map(|a| Dividend {
                    effective: day,
                    ..a
                }),
            }
        })
        .collect()
}

/// 1銘柄分の入力を組み立てる: 株価と配当、年次財務（新しい期が先頭）、四半期単独の損益（古い順）。
/// 株式分割があれば、終値と1株当たりの値（配当・EPS・BPS）を最新の株数の基準にそろえる。
/// EPS・BPS は開示までの分割を織り込んで開示されるので、開示した日より後の分割の分だけ直す。
/// 配当実績は期末の株数の基準なので、期末（の権利落ち日）より後の分割の分だけ直す
pub fn build_inputs(
    mut bars: Vec<PriceBar>,
    annuals: &[StoredAnnual],
    forecasts: &[StoredForecast],
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
            let fd = a
                .financial
                .period_end
                .map_or(f, |end| factor_after(splits, dividend_basis(end)));
            let mut a = a.clone();
            a.financial.eps = a.financial.eps.map(|x| x * f);
            a.financial.bps = a.financial.bps.map(|x| x * f);
            a.financial.dps = a.financial.dps.map(|x| x * fd);
            a.breakdown = a.breakdown.as_ref().map(|b| b.adjusted(fd));
            a
        })
        .collect();
    // 数字のない行（PR #29 より前に取り込んだ予想の修正など）を最新の本決算にしない
    let mut sorted: Vec<(usize, &StoredAnnual)> = adjusted
        .iter()
        .enumerate()
        .filter(|(_, a)| a.financial.period_end.is_some() && !a.financial.has_no_figures())
        .collect();
    // 新しい期が先頭
    sorted.sort_by_key(|(_, a)| std::cmp::Reverse(a.financial.period_end));
    let actuals: Vec<Dividend> = sorted
        .iter()
        .filter_map(|(_, a)| {
            Some(Dividend {
                effective: effective_date(a)?,
                annual: a.financial.dps?,
                breakdown: a.breakdown.clone(),
                forecast: false,
            })
        })
        .collect();
    // 予想は開示した時点の株数の基準。ただし対象の期末より後の分割は入っていない
    let forecasts: Vec<StoredForecast> = forecasts
        .iter()
        .map(|f| {
            let basis = f
                .fiscal_year_end
                .map_or(f.disclosed, |end| f.disclosed.min(dividend_basis(end)));
            StoredForecast {
                dps: f.dps.map(|x| x * factor_after(splits, basis)),
                breakdown: f
                    .breakdown
                    .as_ref()
                    .map(|b| b.adjusted(factor_after(splits, basis))),
                ..f.clone()
            }
        })
        .collect();
    let dividends = dividend_series(&actuals, &forecasts);
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

/// D1 から読んだ全銘柄の財務（キーは銘柄コード）
#[derive(Debug, Default)]
pub struct StoredFinancials {
    pub annuals: HashMap<String, Vec<StoredAnnual>>,
    pub forecasts: HashMap<String, Vec<StoredForecast>>,
    pub quarterlies: HashMap<String, Vec<CumulativePeriod>>,
    pub splits: HashMap<String, Vec<Split>>,
}

/// 全銘柄の指標を計算する。株価の履歴がない銘柄は飛ばす
pub fn compute_all(
    codes: &[StockCode],
    mut histories: HashMap<String, Vec<PriceBar>>,
    stored: &StoredFinancials,
    run_id: &str,
    now: i64,
) -> (Vec<Snapshot>, usize) {
    let StoredFinancials {
        annuals,
        forecasts,
        quarterlies,
        splits,
    } = stored;
    let mut snaps = vec![];
    let mut skipped = 0;
    let no_annuals = vec![];
    let no_forecasts = vec![];
    let no_quarters = vec![];
    let no_splits = vec![];
    for code in codes {
        let bars = histories.remove(code.as_str()).unwrap_or_default();
        let anns = annuals.get(code.as_str()).unwrap_or(&no_annuals);
        let (market, fin, quarters, latest) = build_inputs(
            bars,
            anns,
            forecasts.get(code.as_str()).unwrap_or(&no_forecasts),
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
            breakdown: Some(Default::default()),
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
        let (m, fin, _, latest) = build_inputs(vec![], &annuals, &[], &[], &[]);
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

    /// 配当予想の修正（DividendForecastRevision）が本決算の形で残った行: 来期の期末だけで数字がない
    fn revision_row(period_end: &str, disclosed: &str) -> StoredAnnual {
        StoredAnnual {
            breakdown: Some(Default::default()),
            doc_id: format!("rev-{period_end}"),
            disclosed: Some(d(disclosed)),
            financial: AnnualFinancial {
                period_end: Some(d(period_end)),
                ..Default::default()
            },
        }
    }

    #[test]
    fn row_without_figures_is_not_the_latest_annual() {
        let annuals = [
            annual("2026-03-31", Some("2026-05-10"), 44.0, 110.0),
            revision_row("2027-03-31", "2026-08-25"),
        ];
        let qs = [
            quarter("2027-03-31", "2026-06-30", PeriodType::Q1, 30.0),
            quarter("2026-03-31", "2025-12-31", PeriodType::Q3, 70.0),
        ];
        let (m, fin, quarters, latest) = build_inputs(vec![], &annuals, &[], &qs, &[]);
        let fin = fin.unwrap();
        assert_eq!(latest, Some(0), "数字のある 2026/03 期が最新");
        assert_eq!(fin.doc_id, "doc-2026-03-31");
        assert_eq!(fin.years.len(), 1, "空の行は5期の表にも入れない");
        assert_eq!(m.dividends.len(), 1);
        assert!(
            quarters
                .iter()
                .all(|q| q.fiscal_year_end != d("2027-03-31") || q.quarter != 4),
            "空の行から 2027/03 期の Q4 を作らない"
        );
    }

    #[test]
    fn fourth_quarter_comes_from_annual_minus_3q() {
        let annuals = [annual("2026-03-31", Some("2026-05-10"), 44.0, 110.0)];
        let qs = [
            quarter("2026-03-31", "2025-06-30", PeriodType::Q1, 20.0),
            quarter("2026-03-31", "2025-09-30", PeriodType::Q2, 50.0),
            quarter("2026-03-31", "2025-12-31", PeriodType::Q3, 70.0),
        ];
        let (_, _, quarters, _) = build_inputs(vec![], &annuals, &[], &qs, &[]);
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
            &[code.clone(), missing],
            hist,
            &StoredFinancials {
                annuals: ann.clone(),
                ..Default::default()
            },
            "r1",
            0,
        );
        assert_eq!(skipped, 1, "株価の履歴がない銘柄は飛ばす");
        let i = &snaps[0].indicators;
        assert!((i.dividend_yield.unwrap() - 0.04).abs() < 1e-9);
        assert_eq!(i.dividend_is_forecast, Some(false), "予想がなければ実績");
        assert!((i.payout_ratio.unwrap() - 0.4).abs() < 1e-9);
        assert_eq!(snaps[0].fin_doc_id.as_deref(), Some("doc-2026-03-31"));

        // 来期の空の行が後から入っても、PER・配当性向は 2026/03 期の数字で出る
        ann.get_mut("7203")
            .unwrap()
            .push(revision_row("2027-03-31", "2026-08-25"));
        let bars: Vec<PriceBar> = (0..10)
            .map(|i| PriceBar {
                date: d("2026-09-01").add_days(i),
                close: 1000.0,
            })
            .collect();
        let mut hist = HashMap::new();
        hist.insert("7203".to_string(), bars);
        let (snaps, _) = compute_all(
            &[code],
            hist,
            &StoredFinancials {
                annuals: ann,
                ..Default::default()
            },
            "r2",
            0,
        );
        let i = &snaps[0].indicators;
        assert!((i.per.unwrap() - 10.0).abs() < 1e-9);
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
            &StoredFinancials {
                annuals: ann.clone(),
                splits: splits.clone(),
                ..Default::default()
            },
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
            &StoredFinancials {
                annuals: ann.clone(),
                ..Default::default()
            },
            "r1",
            0,
        );
        assert!(
            (unadjusted[0].indicators.dividend_yield.unwrap() - 0.08).abs() < 1e-9,
            "分割を知らなければ 40円 ÷ 500円 で2倍に出る"
        );
    }

    fn forecast(disclosed: &str, dps: Option<f64>) -> StoredForecast {
        StoredForecast {
            breakdown: Some(Default::default()),
            doc_id: format!("f-{disclosed}"),
            disclosed: d(disclosed),
            fiscal_year_end: None,
            dps,
        }
    }

    #[test]
    fn yield_uses_latest_forecast_and_falls_back_to_actual() {
        let annuals = [
            annual("2025-03-31", Some("2025-05-10"), 40.0, 100.0),
            annual("2026-03-31", Some("2026-05-12"), 44.0, 110.0),
        ];
        let forecasts = [
            // 本決算と同じ日に来期の予想
            forecast("2025-05-10", Some(42.0)),
            // 第2四半期で増額
            forecast("2025-11-10", Some(46.0)),
            // 次の本決算では来期の予想が未定
            forecast("2026-05-12", None),
            // 第1四半期で予想を出した
            forecast("2026-08-05", Some(50.0)),
        ];
        let (m, _, _, _) = build_inputs(vec![], &annuals, &forecasts, &[], &[]);
        let at = |s| domain::dividend_at(&m.dividends, d(s)).map(|x| (x.annual, x.forecast));
        assert_eq!(at("2025-05-09"), None);
        assert_eq!(at("2025-05-10"), Some((42.0, true)));
        assert_eq!(at("2025-12-01"), Some((46.0, true)));
        assert_eq!(at("2026-06-01"), Some((44.0, false)), "予想が未定なら実績");
        assert_eq!(at("2026-08-05"), Some((50.0, true)));
    }

    #[test]
    fn actual_only_when_no_forecast_was_ever_disclosed() {
        let annuals = [annual("2025-03-31", Some("2025-05-10"), 40.0, 100.0)];
        let (m, _, _, _) = build_inputs(vec![], &annuals, &[], &[], &[]);
        assert_eq!(m.dividends.len(), 1);
        assert!(!m.dividends[0].forecast);
        // 実績がなく予想だけ（上場直後など）でも利回りは出る
        let (m, _, _, _) =
            build_inputs(vec![], &[], &[forecast("2025-08-01", Some(30.0))], &[], &[]);
        assert_eq!(m.dividends[0].annual, 30.0);
    }

    #[test]
    fn forecast_disclosed_before_a_split_is_adjusted() {
        // 2026-08-05 に予想60円、2026-09-29 に1:2の分割 → 30円として扱う
        let forecasts = [forecast("2026-08-05", Some(60.0))];
        let splits = [Split::new(d("2026-09-29"), 0.5).unwrap()];
        let (m, _, _, _) = build_inputs(vec![], &[], &forecasts, &[], &splits);
        assert_eq!(m.dividends[0].annual, 30.0);
        assert!(m.dividends[0].forecast);
    }

    #[test]
    fn march_company_uses_2027_forecast_despite_later_prior_year_correction() {
        let annuals = [annual("2026-03-31", Some("2026-05-08"), 60.0, 100.0)];
        let mut forecasts = vec![
            StoredForecast {
                fiscal_year_end: Some(d("2027-03-31")),
                ..forecast("2026-05-08", Some(80.0))
            },
            StoredForecast {
                fiscal_year_end: Some(d("2027-03-31")),
                ..forecast("2026-08-10", Some(90.0))
            },
            StoredForecast {
                fiscal_year_end: Some(d("2026-03-31")),
                ..forecast("2026-10-01", Some(65.0))
            },
        ];
        let bars = vec![PriceBar {
            date: d("2026-10-29"),
            close: 2000.0,
        }];
        let (m, _, _, _) = build_inputs(bars.clone(), &annuals, &forecasts, &[], &[]);
        let i = compute(&m, None, &[]).unwrap();
        assert_eq!(i.dividend_yield, Some(90.0 / 2000.0));
        assert_eq!(i.dividend_is_forecast, Some(true));
        assert_eq!(
            domain::annual_dividend_at(&m.dividends, d("2026-07-01")),
            Some(80.0)
        );

        forecasts.push(StoredForecast {
            fiscal_year_end: Some(d("2027-03-31")),
            ..forecast("2026-10-15", None)
        });
        let (m, _, _, _) = build_inputs(bars, &annuals, &forecasts, &[], &[]);
        let i = compute(&m, None, &[]).unwrap();
        assert_eq!(i.dividend_ttm, 60.0);
        assert_eq!(i.dividend_is_forecast, Some(false));
    }

    #[test]
    fn dividend_of_the_year_before_a_split_is_adjusted() {
        // SPK（7466）: 2026-03-30 に1:2の分割（効力は 4/1）。2026/03 期の配当73円は分割前の株に払い、
        // 2026-05-08 の決算短信では EPS だけ分割後に直して開示している
        let bars: Vec<PriceBar> = ["2026-03-27", "2026-03-30", "2026-10-21"]
            .iter()
            .zip([2616.0, 1269.0, 1405.0])
            .map(|(s, close)| PriceBar { date: d(s), close })
            .collect();
        let annuals = [
            annual("2025-03-31", Some("2025-05-09"), 60.0, 247.94),
            annual("2026-03-31", Some("2026-05-08"), 73.0, 133.35),
        ];
        let forecasts = [
            // 第3四半期: 2026/03 期の予想（分割前の株）
            StoredForecast {
                fiscal_year_end: Some(d("2026-03-31")),
                ..forecast("2026-02-09", Some(73.0))
            },
            // 本決算: 2027/03 期の予想（分割後の株）
            StoredForecast {
                fiscal_year_end: Some(d("2027-03-31")),
                ..forecast("2026-05-08", Some(40.0))
            },
        ];
        let splits = [Split::new(d("2026-03-30"), 0.5).unwrap()];
        let (m, fin, _, _) = build_inputs(bars.clone(), &annuals, &forecasts, &[], &splits);
        let closes: Vec<f64> = m.bars.iter().map(|b| b.close).collect();
        assert_eq!(closes, vec![1308.0, 1269.0, 1405.0]);
        let at = |s| domain::dividend_at(&m.dividends, d(s)).map(|x| x.annual);
        assert_eq!(at("2025-06-01"), Some(30.0));
        assert_eq!(at("2026-03-01"), Some(36.5), "分割前に開示した今期の予想");
        assert_eq!(
            at("2026-06-01"),
            Some(40.0),
            "分割後に開示した来期の予想はそのまま"
        );
        let years = fin.unwrap().years;
        assert_eq!(
            (years[0].dps, years[0].eps),
            (Some(36.5), Some(133.35)),
            "配当は期末の株数の基準で直し、EPS は開示の時点で分割後"
        );
        assert_eq!((years[1].dps, years[1].eps), (Some(30.0), Some(123.97)));

        // 予想が未定でも、実績の配当で利回り・配当性向が2倍にならない
        let (m, fin, q, _) = build_inputs(bars, &annuals, &[], &[], &splits);
        let i = compute(&m, fin.as_ref(), &q).unwrap();
        assert!((i.dividend_yield.unwrap() - 36.5 / 1405.0).abs() < 1e-9);
        assert!((i.payout_ratio.unwrap() - 36.5 / 133.35).abs() < 1e-9);
    }
    #[test]
    fn forecast_yield_uses_total_including_special_and_adjusts_splits() {
        let mut f = forecast("2026-08-13", Some(475.0));
        f.breakdown = Some(domain::DividendBreakdown {
            special: 415.0,
            commemorative: 0.0,
            source: "https://example.com/disclosure".into(),
            known_since: None,
        });
        let bars = vec![PriceBar {
            date: d("2026-10-22"),
            close: 1407.0,
        }];
        let split = [Split::new(d("2026-09-29"), 0.5).unwrap()];
        let (m, fin, q, _) = build_inputs(bars, &[], &[f], &[], &split);
        let i = compute(&m, fin.as_ref(), &q).unwrap();
        assert_eq!(i.dividend_total, Some(237.5));
        assert_eq!(i.dividend_special, Some(207.5));
        assert_eq!(i.dividend_commemorative, Some(0.0));
        assert_eq!(i.dividend_ttm, 237.5);
        assert!((i.dividend_yield.unwrap() - 475.0 / 2814.0).abs() < 1e-10);
        assert_eq!(
            i.dividend_source.as_deref(),
            Some("https://example.com/disclosure")
        );
    }

    #[test]
    fn unknown_forecast_breakdown_still_uses_forecast() {
        let a = annual("2026-03-31", Some("2026-05-10"), 40.0, 100.0);
        let mut f = forecast("2026-08-13", Some(475.0));
        f.breakdown = None;
        let bars = vec![PriceBar {
            date: d("2026-10-22"),
            close: 2814.0,
        }];
        let (m, fin, q, _) = build_inputs(
            bars.clone(),
            std::slice::from_ref(&a),
            std::slice::from_ref(&f),
            &[],
            &[],
        );
        let i = compute(&m, fin.as_ref(), &q).unwrap();
        assert_eq!(i.dividend_yield, Some(475.0 / 2814.0));
        assert_eq!(i.dividend_total, Some(475.0));
        assert_eq!(i.dividend_is_forecast, Some(true));
        assert!(!domain::yield_series(&m).is_empty());
        // 予想自体が未定なら実績の通常配当に戻す。
        f.dps = None;
        let (m, fin, q, _) = build_inputs(bars, &[a], &[f], &[], &[]);
        let i = compute(&m, fin.as_ref(), &q).unwrap();
        assert_eq!(i.dividend_ttm, 40.0);
        assert_eq!(i.dividend_is_forecast, Some(false));
    }

    #[test]
    fn actual_breakdown_is_adjusted_from_fiscal_year_and_zero_is_known() {
        let mut a = annual("2026-03-31", Some("2026-05-10"), 100.0, 100.0);
        a.breakdown = Some(domain::DividendBreakdown {
            special: 40.0,
            commemorative: 10.0,
            source: String::new(),
            known_since: None,
        });
        let splits = [Split::new(d("2026-03-30"), 0.5).unwrap()];
        let bars = vec![PriceBar {
            date: d("2026-10-22"),
            close: 500.0,
        }];
        let (m, fin, q, _) = build_inputs(bars.clone(), &[a], &[], &[], &splits);
        let i = compute(&m, fin.as_ref(), &q).unwrap();
        assert_eq!(i.dividend_total, Some(50.0));
        assert_eq!(i.dividend_ttm, 25.0);
        assert_eq!(i.dividend_yield, Some(0.05));
        assert_eq!(
            fin.unwrap().years[0].dps,
            Some(50.0),
            "配当性向などは合計配当を維持"
        );
        let mut f = forecast("2026-08-13", Some(0.0));
        f.breakdown = None;
        let (m, _, _, _) = build_inputs(bars, &[], &[f], &[], &[]);
        assert_eq!(compute(&m, None, &[]).unwrap().dividend_yield, Some(0.0));
    }
    #[test]
    fn edinet_breakdown_is_used_only_after_submission_and_unverified_actual_remains_visible() {
        let mut a = annual("2026-03-31", Some("2026-05-10"), 100.0, 100.0);
        a.breakdown = Some(domain::DividendBreakdown {
            special: 40.0,
            commemorative: 0.0,
            source: "https://example.com/edinet".into(),
            known_since: Some(d("2026-06-20")),
        });
        let mut f = forecast("2026-05-10", None);
        f.breakdown = None;
        let bars = vec![PriceBar {
            date: d("2026-07-01"),
            close: 1000.0,
        }];
        let (m, _, _, _) = build_inputs(bars, &[a], &[f], &[], &[]);
        assert_eq!(
            domain::annual_dividend_at(&m.dividends, d("2026-06-19")),
            Some(100.0)
        );
        assert_eq!(
            domain::annual_dividend_at(&m.dividends, d("2026-06-20")),
            Some(60.0)
        );
        let i = compute(&m, None, &[]).unwrap();
        assert_eq!(i.dividend_yield, Some(0.06));
        assert_eq!(i.dividend_is_forecast, Some(false));
        assert_eq!(i.dividend_special, Some(40.0));
    }

    #[test]
    fn reference_total_is_available_without_any_breakdown_or_without_actuals() {
        let mut a = annual("2026-03-31", Some("2026-05-10"), 40.0, 100.0);
        a.breakdown = None;
        let mut f = forecast("2026-05-10", Some(50.0));
        f.breakdown = None;
        let bars = vec![PriceBar {
            date: d("2026-07-01"),
            close: 1000.0,
        }];
        let (m, _, _, _) = build_inputs(bars.clone(), &[a], std::slice::from_ref(&f), &[], &[]);
        let i = compute(&m, None, &[]).unwrap();
        assert_eq!(i.dividend_yield, Some(0.05));
        assert_eq!(i.dividend_special, None);
        assert_eq!(i.dividend_is_forecast, Some(true));
        let (m, _, _, _) = build_inputs(bars, &[], &[f], &[], &[]);
        let i = compute(&m, None, &[]).unwrap();
        assert_eq!(i.dividend_yield, Some(0.05));
        assert_eq!(i.dividend_is_forecast, Some(true));
    }
}
