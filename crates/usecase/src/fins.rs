//! 年次の財務の取り込み（J-Quants の財務情報サマリー → financials_annual）。
//! 指標の計算（ingest）は、ここで保存した財務を D1 から読んで使う。

use crate::PortError;
use domain::{AnnualFinancial, Date, StockCode};

/// 本決算の1件
#[derive(Debug, Clone, PartialEq)]
pub struct AnnualReport {
    pub code: StockCode,
    /// 開示番号（同じ期の訂正などで上書きする）
    pub doc_id: String,
    pub disclosed: Option<Date>,
    pub financial: AnnualFinancial,
}

pub trait FinancialSummarySource {
    /// 指定日に開示された本決算（全銘柄）
    async fn annual_reports_on(&self, date: Date) -> Result<Vec<AnnualReport>, PortError>;
}

pub trait FinancialStore {
    async fn save_annual_reports(
        &self,
        reports: &[AnnualReport],
        fetched_at: i64,
    ) -> Result<(), PortError>;
}

/// 平日の開示日を順に取り込み、保存した本決算の件数を返す
pub async fn sync_fins<S, T>(
    source: &S,
    store: &T,
    dates: &[Date],
    fetched_at: i64,
    mut on_day: impl FnMut(Date, usize),
) -> Result<usize, PortError>
where
    S: FinancialSummarySource,
    T: FinancialStore,
{
    let mut total = 0;
    for &date in dates {
        let reports = source.annual_reports_on(date).await?;
        if !reports.is_empty() {
            store.save_annual_reports(&reports, fetched_at).await?;
        }
        total += reports.len();
        on_day(date, reports.len());
    }
    Ok(total)
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
