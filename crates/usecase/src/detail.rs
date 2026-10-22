//! 銘柄の詳細（F3）。1銘柄の5年分の推移を、保存済みのデータから組み立てる。

use crate::daily::{build_inputs, StoredAnnual, StoredForecast};
use crate::screening::StockRow;
use crate::PortError;
use domain::analysis::{quarter_growth, CumulativePeriod, QuarterGrowth};
use domain::split::Split;
use domain::{yield_series, AnnualFinancial, Date, PriceBar, StockCode};

/// 詳細ページに出す1銘柄分
#[derive(Debug, Clone, PartialEq)]
pub struct StockDetail {
    /// 一覧と同じ最新の指標（名称・種類・業種・規模を含む）
    pub latest: StockRow,
    /// 年次の財務（新しい期が先頭、最大5期）
    pub annual: Vec<AnnualFinancial>,
    /// 四半期単独の損益と前年の同じ四半期からの伸び率（古い順、最大20四半期）
    pub quarters: Vec<QuarterGrowth>,
    /// 配当利回りの推移（週1点、古い順）
    pub yield_series: Vec<(Date, f64)>,
    /// 終値の推移（週1点、古い順）
    pub price_series: Vec<PriceBar>,
    /// 取り込み済みの決算短信（年次）のうち最新の開示日（F8: 財務の鮮度）
    pub latest_disclosed: Option<Date>,
}

pub trait StockDetailQuery {
    async fn latest(&self, code: &StockCode) -> Result<Option<StockRow>, PortError>;
    async fn price_history(&self, code: &StockCode) -> Result<Vec<PriceBar>, PortError>;
    async fn annuals(&self, code: &StockCode) -> Result<Vec<StoredAnnual>, PortError>;
    async fn quarterlies(&self, code: &StockCode) -> Result<Vec<CumulativePeriod>, PortError>;
    /// 会社予想の配当（開示1件1行）
    async fn forecasts(&self, code: &StockCode) -> Result<Vec<StoredForecast>, PortError>;
    /// 株式分割・併合（日付の古い順）
    async fn splits(&self, code: &StockCode) -> Result<Vec<Split>, PortError>;
}

const MAX_ANNUAL: usize = 5;
const MAX_QUARTERS: usize = 20;
/// 株価のグラフも利回りと同じ間隔（5営業日ごと）
const PRICE_STEP: usize = 5;

/// 銘柄がなければ None
pub async fn stock_detail<Q: StockDetailQuery>(
    repo: &Q,
    code: &StockCode,
) -> Result<Option<StockDetail>, PortError> {
    let Some(latest) = repo.latest(code).await? else {
        return Ok(None);
    };
    let bars = repo.price_history(code).await?;
    let annuals = repo.annuals(code).await?;
    let quarterlies = repo.quarterlies(code).await?;
    let splits = repo.splits(code).await?;
    let forecasts = repo.forecasts(code).await?;
    let latest_disclosed = annuals.iter().filter_map(|a| a.disclosed).max();
    let (market, fin, quarters, _) =
        build_inputs(bars, &annuals, &forecasts, &quarterlies, &splits);
    let mut ys = yield_series(&market);
    ys.reverse();
    // 最も古い年度の伸び率も出せるよう、絞り込む前に前年と比べる
    let quarters = quarter_growth(&quarters);
    let n = quarters.len();
    Ok(Some(StockDetail {
        latest,
        annual: fin
            .map(|f| f.years.into_iter().take(MAX_ANNUAL).collect())
            .unwrap_or_default(),
        quarters: quarters[n.saturating_sub(MAX_QUARTERS)..].to_vec(),
        yield_series: ys,
        price_series: market
            .bars
            .iter()
            .rev()
            .step_by(PRICE_STEP)
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
        latest_disclosed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use domain::analysis::PeriodType;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    struct Fake {
        found: bool,
    }
    impl StockDetailQuery for Fake {
        async fn latest(&self, code: &StockCode) -> Result<Option<StockRow>, PortError> {
            Ok(self.found.then(|| StockRow {
                code: code.as_str().into(),
                price: 1000.0,
                price_date: "2026-10-21".into(),
                dividend: Some(44.0),
                dividend_basis: Some("actual".into()),
                dividend_yield: Some(0.044),
                yield_percentile: None,
                pbr: None,
                per: None,
                payout_ratio: None,
                doe: None,
                consecutive_increases: None,
                operating_margin: None,
                no_loss_years: None,
                profit_uptrend: None,
                valuation: "unknown".into(),
                fetched_at: 0,
                name: Some("テスト".into()),
                kind: "stock".into(),
                market: None,
                sector33: None,
                size_class: None,
                dividend_every_year: None,
                no_quarterly_loss: None,
                is_value: None,
                data_years: None,
                sales_growth: None,
                op_growth: None,
                progressive_dividend: None,
                no_dividend_cut: None,
                reit: None,
            }))
        }
        async fn price_history(&self, _: &StockCode) -> Result<Vec<PriceBar>, PortError> {
            // 2025-06-02 から平日 60 本（12週）
            let mut out = vec![];
            let mut day = d("2025-06-02");
            while out.len() < 60 {
                if (1..=5).contains(&domain::calendar::weekday(day)) {
                    out.push(PriceBar {
                        date: day,
                        close: 1000.0,
                    });
                }
                day = day.add_days(1);
            }
            Ok(out)
        }
        async fn annuals(&self, _: &StockCode) -> Result<Vec<StoredAnnual>, PortError> {
            let mk = |pe: &str, disc: &str, dps: f64| StoredAnnual {
                doc_id: pe.into(),
                disclosed: Some(d(disc)),
                financial: AnnualFinancial {
                    period_end: Some(d(pe)),
                    dps: Some(dps),
                    net_income: Some(100.0),
                    ..Default::default()
                },
            };
            Ok(vec![
                mk("2025-03-31", "2025-05-10", 44.0),
                mk("2024-03-31", "2024-05-10", 40.0),
            ])
        }
        async fn quarterlies(&self, _: &StockCode) -> Result<Vec<CumulativePeriod>, PortError> {
            Ok(vec![CumulativePeriod {
                fiscal_year_end: d("2025-03-31"),
                period_end: d("2024-06-30"),
                period_type: PeriodType::Q1,
                net_sales: None,
                operating_income: None,
                ordinary_income: None,
                net_income: Some(30.0),
            }])
        }
        async fn forecasts(&self, _: &StockCode) -> Result<Vec<StoredForecast>, PortError> {
            Ok(vec![])
        }
        async fn splits(&self, _: &StockCode) -> Result<Vec<Split>, PortError> {
            Ok(vec![])
        }
    }

    #[test]
    fn builds_detail_from_stored_data() {
        let code = StockCode::parse("7203").unwrap();
        let det = block_on(stock_detail(&Fake { found: true }, &code))
            .unwrap()
            .unwrap();
        assert_eq!(det.annual.len(), 2);
        assert_eq!(det.annual[0].dps, Some(44.0), "新しい期が先頭");
        // FY2024（Q4 は 3Q がないので不明）、2025年度の 1Q、FY2025（同じく不明）の3件、古い順
        assert_eq!(det.quarters.len(), 3);
        let q1 = det.quarters.iter().find(|q| q.result.quarter == 1).unwrap();
        assert_eq!(q1.result.net_income, Some(30.0));
        assert_eq!(q1.net_growth, None, "前年の 1Q がない");
        assert!(det
            .quarters
            .iter()
            .filter(|q| q.result.quarter == 4)
            .all(|q| q.result.net_income.is_none()));
        assert_eq!(det.yield_series.len(), 12, "60営業日 → 週1点で12点");
        assert!(det.yield_series[0].0 < det.yield_series[11].0, "古い順");
        assert!((det.yield_series[11].1 - 0.044).abs() < 1e-9);
        assert_eq!(det.price_series.len(), 12);
    }

    #[test]
    fn unknown_code_is_none() {
        let code = StockCode::parse("9999").unwrap();
        assert_eq!(
            block_on(stock_detail(&Fake { found: false }, &code)).unwrap(),
            None
        );
    }
}
