//! スクリーニング一覧（F2）とデータ鮮度（F8）の読み取りユースケース。

use crate::PortError;
use domain::screening::{ScreeningFilter, PAGE_SIZE};

/// 一覧の1行（indicators_latest の1行）。比率は小数（0.035 = 3.5%）
#[derive(Debug, Clone, PartialEq)]
pub struct StockRow {
    pub code: String,
    pub price: f64,
    pub price_date: String,
    /// 利回りに使った1株配当（円）と、それが会社予想（forecast）か実績（actual）か。配当の開示がなければ None
    pub dividend: Option<f64>,
    pub dividend_basis: Option<String>,
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
    /// 5年分の判定（期数が足りなければ None）
    pub dividend_every_year: Option<bool>,
    pub no_quarterly_loss: Option<bool>,
    pub is_value: Option<bool>,
    pub data_years: Option<i64>,
    /// 売上高・営業利益の前年同期比（J-Quants の決算短信から）
    pub sales_growth: Option<f64>,
    pub op_growth: Option<f64>,
    /// 有報・半期報告書に累進配当・非減配の記述がある（EDINET。書類がなければ None）
    pub progressive_dividend: Option<bool>,
    pub no_dividend_cut: Option<bool>,
    /// REIT の金利・稼働率（reit_profile。REIT 以外や未取り込みなら None）
    pub reit: Option<ReitListFields>,
}

/// 一覧に出す REIT の要約。比率は小数
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReitListFields {
    /// office / logistics / residential / hotel / retail / healthcare / diversified / infra
    pub sector: Option<String>,
    pub period_end: Option<String>,
    pub floating_ratio: Option<f64>,
    pub unknown_ratio: Option<f64>,
    pub reprice_ratio: Option<f64>,
    pub reprice_max_ratio: Option<f64>,
    pub avg_rate: Option<f64>,
    pub occupancy: Option<f64>,
}

/// REIT の最新の有報の要約（reit_profile）。金額は円、面積は㎡、avg_rate・occupancy は %
#[derive(Debug, Clone, PartialEq)]
pub struct ReitProfile {
    pub doc_id: String,
    pub submitted: String,
    pub period_end: String,
    pub sector: Option<String>,
    pub short_loans: i64,
    pub long_loans: i64,
    pub bonds: i64,
    pub rate_fixed: i64,
    pub rate_swap_fixed: i64,
    pub rate_swap_amount: i64,
    pub rate_floating: i64,
    pub rate_unknown: i64,
    pub swap_notional: Option<i64>,
    pub avg_rate: Option<f64>,
    pub avg_years: Option<f64>,
    pub reprice_1y: i64,
    pub reprice_1y_max: i64,
    pub property_count: i64,
    pub leasable_m2: Option<f64>,
    pub leased_m2: Option<f64>,
    pub occupancy: Option<f64>,
    /// 読み取りの確認（JSON の文字列）
    pub checks: String,
}

/// REIT の決算ごとの要約（reit_history）
#[derive(Debug, Clone, PartialEq)]
pub struct ReitHistoryRow {
    pub period_end: String,
    pub loans_total: i64,
    pub bonds: i64,
    pub rate_floating: i64,
    pub rate_unknown: i64,
    pub swap_notional: Option<i64>,
    pub avg_rate: Option<f64>,
    pub property_count: i64,
    pub leasable_m2: Option<f64>,
    pub occupancy: Option<f64>,
}

/// 1銘柄の REIT の要約・明細（JSON の文字列のまま）・推移
#[derive(Debug, Clone, PartialEq)]
pub struct ReitDetail {
    pub profile: ReitProfile,
    pub body: Option<String>,
    pub history: Vec<ReitHistoryRow>,
}

/// 1銘柄の EDINET の書類（有報・半期報告書の最新1通ずつ）と配当方針の手がかり
#[derive(Debug, Clone, PartialEq)]
pub struct DisclosureRow {
    /// annual / semiannual
    pub doc_kind: String,
    pub doc_id: String,
    pub submitted: String,
    pub period_end: Option<String>,
    pub progressive_dividend: bool,
    pub no_dividend_cut: bool,
    pub dividend_raise: bool,
    pub doe_target: bool,
    pub payout_target: Option<f64>,
    /// 手がかりの言葉を含む文
    pub phrases: Vec<String>,
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
    /// 取り込み済みの決算短信のうち最新の開示日
    pub latest_fin_date: Option<String>,
}

pub trait IndicatorQuery {
    async fn search(&self, filter: &ScreeningFilter) -> Result<(Vec<StockRow>, u64), PortError>;
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError>;
    async fn disclosures(&self, code: &str) -> Result<Vec<DisclosureRow>, PortError>;
    /// まだ取り込んでいない銘柄は None
    async fn reit_detail(&self, code: &str) -> Result<Option<ReitDetail>, PortError>;
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

/// 銘柄コードを確かめてから、その銘柄の EDINET の書類を返す（コードが不正なら None）
pub async fn disclosures<Q: IndicatorQuery>(
    repo: &Q,
    code: &str,
) -> Result<Option<Vec<DisclosureRow>>, PortError> {
    let Some(code) = domain::StockCode::parse(code) else {
        return Ok(None);
    };
    repo.disclosures(code.as_str()).await.map(Some)
}

pub async fn data_status<Q: IndicatorQuery>(repo: &Q) -> Result<Option<DataStatus>, PortError> {
    repo.latest_run().await
}

/// 銘柄コードを確かめてから、その銘柄の REIT の要約・明細・推移を返す（コードが不正なら外側が None、未取り込みなら内側が None）
pub async fn reit_detail<Q: IndicatorQuery>(
    repo: &Q,
    code: &str,
) -> Result<Option<Option<ReitDetail>>, PortError> {
    let Some(code) = domain::StockCode::parse(code) else {
        return Ok(None);
    };
    repo.reit_detail(code.as_str()).await.map(Some)
}
