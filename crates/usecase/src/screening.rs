//! スクリーニング一覧（F2）とデータ鮮度（F8）の読み取りユースケース。

use crate::PortError;
use domain::screening::{ScreeningFilter, PAGE_SIZE};

/// 一覧の1行（indicators_latest の1行）。比率は小数（0.035 = 3.5%）
#[derive(Debug, Clone, PartialEq)]
pub struct StockRow {
    pub code: String,
    pub price: f64,
    pub price_date: String,
    pub dividend_yield: Option<f64>,
    pub yield_percentile: Option<f64>,
    pub pbr: Option<f64>,
    pub per: Option<f64>,
    pub payout_ratio: Option<f64>,
    pub doe: Option<f64>,
    pub consecutive_increases: Option<i64>,
    pub operating_margin: Option<f64>,
    pub no_loss_years: Option<bool>,
    pub profit_uptrend: Option<bool>,
    pub valuation: String,
    pub fetched_at: i64,
    /// 銘柄マスタ（stocks）から。マスタにまだなければ None
    pub name: Option<String>,
    /// stock / reit / etf
    pub kind: String,
    pub market: Option<String>,
    pub sector33: Option<String>,
    /// large / mid / small / none
    pub size_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StockPage {
    pub rows: Vec<StockRow>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
}

/// 直近のバッチの状態（F8）
#[derive(Debug, Clone, PartialEq)]
pub struct DataStatus {
    pub run_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: String,
    pub latest_price_date: Option<String>,
}

pub trait IndicatorQuery {
    async fn search(&self, filter: &ScreeningFilter) -> Result<(Vec<StockRow>, u64), PortError>;
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError>;
}

pub async fn screen<Q: IndicatorQuery>(
    repo: &Q,
    filter: &ScreeningFilter,
) -> Result<StockPage, PortError> {
    let (rows, total) = repo.search(filter).await?;
    Ok(StockPage {
        rows,
        total,
        page: filter.page,
        page_size: PAGE_SIZE,
    })
}

pub async fn data_status<Q: IndicatorQuery>(repo: &Q) -> Result<Option<DataStatus>, PortError> {
    repo.latest_run().await
}
