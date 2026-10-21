//! D1 から一覧と鮮度を読む（IndicatorQuery の実装）。

use crate::query::{self, Arg, Query};
use domain::screening::ScreeningFilter;
use serde::Deserialize;
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
struct Row {
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
        let total: Option<f64> = self
            .prepare(&query::count(filter))?
            .first(Some("n"))
            .await
            .map_err(err)?;
        Ok((
            rows.into_iter().map(StockRow::from).collect(),
            total.unwrap_or(0.0) as u64,
        ))
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
