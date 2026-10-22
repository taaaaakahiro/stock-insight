//! 財務の取り込み（J-Quants の財務情報サマリー → financials_annual / financials_quarterly / dividend_forecasts）。
//! 指標の計算（daily）は、ここで保存した財務を D1 から読んで使う。

use crate::PortError;
use domain::analysis::CumulativePeriod;
use domain::{AnnualFinancial, Date, StockCode};

/// 本決算の1件（年次の指標と配当に使う）
#[derive(Debug, Clone, PartialEq)]
pub struct AnnualReport {
    pub code: StockCode,
    /// 開示番号（同じ期の訂正などで上書きする）
    pub doc_id: String,
    pub disclosed: Option<Date>,
    pub financial: AnnualFinancial,
}

/// 決算短信の1行（1Q/2Q/3Q/FY の累計値。四半期の赤字判定と詳細ページに使う）
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodReport {
    pub code: StockCode,
    pub doc_id: String,
    pub disclosed: Option<Date>,
    pub period: CumulativePeriod,
}

/// 開示1件の会社予想の年間配当（決算短信・配当予想の修正）
#[derive(Debug, Clone, PartialEq)]
pub struct ForecastReport {
    pub code: StockCode,
    pub doc_id: String,
    pub disclosed: Date,
    /// 予想の対象の事業年度の終了日
    pub fiscal_year_end: Option<Date>,
    /// None は「予想なし（未定・非開示）」
    pub dps: Option<f64>,
}

pub trait FinancialStore {
    async fn save_annual_reports(
        &self,
        reports: &[AnnualReport],
        fetched_at: i64,
    ) -> Result<(), PortError>;
    async fn save_period_reports(
        &self,
        reports: &[PeriodReport],
        fetched_at: i64,
    ) -> Result<(), PortError>;
    async fn save_forecasts(
        &self,
        reports: &[ForecastReport],
        fetched_at: i64,
    ) -> Result<(), PortError>;
}

/// from〜to（両端を含む）の平日。祝日は開示がないだけなので除かない
pub fn weekdays(from: Date, to: Date) -> Vec<Date> {
    (from.to_days()..=to.to_days())
        .map(Date::from_days)
        .filter(|d| {
            let w = domain::calendar::weekday(*d);
            w != 0 && w != 6
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekdays_skip_weekends() {
        let d = |s| Date::parse(s).unwrap();
        let days = weekdays(d("2026-10-16"), d("2026-10-20"));
        let s: Vec<String> = days.iter().map(|x| x.to_string()).collect();
        assert_eq!(s, ["2026-10-16", "2026-10-19", "2026-10-20"]);
    }
}
