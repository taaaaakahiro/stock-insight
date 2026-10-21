//! D1 から一覧と鮮度を読む（IndicatorQuery の実装）。ポートフォリオの読み書きもここ。

use crate::query::{self, Arg, Query};
use domain::analysis::{CumulativePeriod, PeriodType};
use domain::screening::ScreeningFilter;
use domain::{AnnualFinancial, Date, PriceBar, StockCode};
use serde::Deserialize;
use usecase::daily::StoredAnnual;
use usecase::detail::StockDetailQuery;
use usecase::portfolio::PortfolioRepository;
use usecase::screening::{DataStatus, IndicatorQuery, StockRow};
use usecase::PortError;
use worker::wasm_bindgen::JsValue;
use worker::{D1Database, D1PreparedStatement};

pub struct D1Indicators {
    db: D1Database,
}

impl D1Indicators {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }

    fn prepare(&self, q: &Query) -> Result<D1PreparedStatement, PortError> {
        let args: Vec<JsValue> = q
            .args
            .iter()
            .map(|a| match a {
                Arg::Real(x) => JsValue::from_f64(*x),
                Arg::Int(i) => JsValue::from_f64(*i as f64),
                Arg::Text(s) => JsValue::from_str(s),
            })
            .collect();
        self.db.prepare(q.sql.as_str()).bind(&args).map_err(err)
    }
}

fn err(e: worker::Error) -> PortError {
    PortError(format!("d1: {e}"))
}

/// indicators_latest の行。SQLite の真偽値は 0/1 の整数
#[derive(Deserialize)]
pub(crate) struct Row {
    code: String,
    price: f64,
    price_date: String,
    dividend_yield: Option<f64>,
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
    /// 条件に合う件数（一覧の SQL だけが返す）
    #[serde(default)]
    total: Option<i64>,
}

impl From<Row> for StockRow {
    fn from(r: Row) -> Self {
        StockRow {
            code: r.code,
            price: r.price,
            price_date: r.price_date,
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
        }
    }
}

#[derive(Deserialize)]
struct VersionRow {
    run_id: String,
    started_at: i64,
    finished_at: Option<i64>,
    edinet_at: Option<i64>,
}

/// これより長く「実行中」のままのバッチは止まったとみなし、キャッシュを再開する（秒）
const STUCK_RUN_SECS: i64 = 3 * 3600;

impl D1Indicators {
    /// データの版（サーバー側キャッシュのキーに使う）。バッチが未実行・実行中なら None（キャッシュしない）
    pub async fn data_version(&self, now: i64) -> Result<Option<String>, PortError> {
        let row: Option<VersionRow> = self
            .db
            .prepare(query::DATA_VERSION)
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.and_then(|r| {
            let finished = match r.finished_at {
                Some(t) => t.to_string(),
                None if now - r.started_at > STUCK_RUN_SECS => "stuck".into(),
                None => return None,
            };
            Some(format!(
                "{}.{finished}.{}",
                r.run_id,
                r.edinet_at.unwrap_or(0)
            ))
        }))
    }
}

#[derive(Deserialize)]
struct AnnualRow {
    period_end: Option<String>,
    doc_id: String,
    disclosed: Option<String>,
    net_sales: Option<f64>,
    operating_income: Option<f64>,
    ordinary_income: Option<f64>,
    net_income: Option<f64>,
    equity: Option<f64>,
    eps: Option<f64>,
    bps: Option<f64>,
    dps: Option<f64>,
    operating_cf: Option<f64>,
}

#[derive(Deserialize)]
struct QuarterRow {
    fiscal_year_end: String,
    period_type: String,
    period_end: String,
    net_sales: Option<f64>,
    operating_income: Option<f64>,
    ordinary_income: Option<f64>,
    net_income: Option<f64>,
}

#[derive(Deserialize)]
struct ClosesRow {
    closes: String,
}

fn code_arg(code: &StockCode) -> [JsValue; 1] {
    [JsValue::from_str(code.as_str())]
}

impl StockDetailQuery for D1Indicators {
    async fn latest(&self, code: &StockCode) -> Result<Option<StockRow>, PortError> {
        let row: Option<Row> = self
            .prepare(&query::latest_by_code(code.as_str()))?
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.map(StockRow::from))
    }

    async fn price_history(&self, code: &StockCode) -> Result<Vec<PriceBar>, PortError> {
        let row: Option<ClosesRow> = self
            .db
            .prepare(query::PRICE_HISTORY_BY_CODE)
            .bind(&code_arg(code))
            .map_err(err)?
            .first(None)
            .await
            .map_err(err)?;
        Ok(row
            .map(|r| domain::price_history::decode(&r.closes))
            .unwrap_or_default())
    }

    async fn annuals(&self, code: &StockCode) -> Result<Vec<StoredAnnual>, PortError> {
        let rows: Vec<AnnualRow> = self
            .db
            .prepare(query::ANNUALS_BY_CODE)
            .bind(&code_arg(code))
            .map_err(err)?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows
            .into_iter()
            .map(|r| StoredAnnual {
                doc_id: r.doc_id,
                disclosed: r.disclosed.as_deref().and_then(Date::parse),
                financial: AnnualFinancial {
                    period_end: r.period_end.as_deref().and_then(Date::parse),
                    net_sales: r.net_sales,
                    operating_income: r.operating_income,
                    ordinary_income: r.ordinary_income,
                    net_income: r.net_income,
                    equity: r.equity,
                    eps: r.eps,
                    bps: r.bps,
                    dps: r.dps,
                    operating_cf: r.operating_cf,
                },
            })
            .collect())
    }

    async fn quarterlies(&self, code: &StockCode) -> Result<Vec<CumulativePeriod>, PortError> {
        let rows: Vec<QuarterRow> = self
            .db
            .prepare(query::QUARTERLIES_BY_CODE)
            .bind(&code_arg(code))
            .map_err(err)?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                Some(CumulativePeriod {
                    fiscal_year_end: Date::parse(&r.fiscal_year_end)?,
                    period_end: Date::parse(&r.period_end)?,
                    period_type: PeriodType::from_jquants(&r.period_type)?,
                    net_sales: r.net_sales,
                    operating_income: r.operating_income,
                    ordinary_income: r.ordinary_income,
                    net_income: r.net_income,
                })
            })
            .collect())
    }
}

#[derive(Deserialize)]
struct RunRow {
    run_id: String,
    started_at: i64,
    finished_at: Option<i64>,
    status: String,
    latest_price_date: Option<String>,
}

impl IndicatorQuery for D1Indicators {
    async fn search(&self, filter: &ScreeningFilter) -> Result<(Vec<StockRow>, u64), PortError> {
        let rows: Vec<Row> = self
            .prepare(&query::list(filter))?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        let total = match rows.first() {
            Some(r) => r.total.unwrap_or(0).max(0) as u64,
            // 1ページ目が空なら0件。最後のページより先を開いたときだけ数え直す
            None if filter.page <= 1 => 0,
            None => {
                let n: Option<f64> = self
                    .prepare(&query::count(filter))?
                    .first(Some("n"))
                    .await
                    .map_err(err)?;
                n.unwrap_or(0.0) as u64
            }
        };
        Ok((rows.into_iter().map(StockRow::from).collect(), total))
    }

    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError> {
        let row: Option<RunRow> = self
            .db
            .prepare(query::LATEST_RUN)
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.map(|r| DataStatus {
            run_id: r.run_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            status: r.status,
            latest_price_date: r.latest_price_date,
        }))
    }
}

/// ポートフォリオの読み書き（PortfolioRepository の実装）
pub struct D1Portfolio {
    db: D1Database,
}

impl D1Portfolio {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }

    fn prepare(&self, sql: &str, args: &[JsValue]) -> Result<D1PreparedStatement, PortError> {
        self.db.prepare(sql).bind(args).map_err(err)
    }
}

#[derive(Deserialize)]
struct CodeRow {
    code: String,
}

impl PortfolioRepository for D1Portfolio {
    async fn codes(&self, user_id: &str) -> Result<Vec<String>, PortError> {
        let rows: Vec<CodeRow> = self
            .prepare(query::PORTFOLIO_CODES, &[user_id.into()])?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows.into_iter().map(|r| r.code).collect())
    }

    async fn rows(&self, user_id: &str) -> Result<Vec<StockRow>, PortError> {
        let rows: Vec<Row> = self
            .prepare(&query::portfolio_rows(), &[user_id.into()])?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows.into_iter().map(StockRow::from).collect())
    }

    async fn add(&self, user_id: &str, code: &str, now: i64) -> Result<(), PortError> {
        self.prepare(
            query::PORTFOLIO_ADD,
            &[user_id.into(), code.into(), JsValue::from_f64(now as f64)],
        )?
        .run()
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn remove(&self, user_id: &str, code: &str) -> Result<(), PortError> {
        self.prepare(query::PORTFOLIO_REMOVE, &[user_id.into(), code.into()])?
            .run()
            .await
            .map_err(err)?;
        Ok(())
    }
}
