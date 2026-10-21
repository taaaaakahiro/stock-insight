//! Yahoo Finance（非公式）から日次終値と配当履歴を取得する。

use crate::http;
use domain::{Date, Dividend, MarketData, PriceBar, StockCode};
use serde_json::Value;
use usecase::{PortError, PriceSource};

pub struct YahooClient {
    client: reqwest::Client,
    base: String,
}

impl YahooClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            base: "https://query1.finance.yahoo.com".into(),
        }
    }
}

impl PriceSource for YahooClient {
    async fn fetch_market(&self, code: &StockCode) -> Result<MarketData, PortError> {
        let url = format!(
            "{}/v8/finance/chart/{}.T?range=10y&interval=1d&events=div%7Csplit",
            self.base,
            code.as_str()
        );
        let body = http::get_bytes(&self.client, &url).await?;
        parse_chart(&body)
    }
}

/// chart API の JSON を解釈する
pub fn parse_chart(body: &[u8]) -> Result<MarketData, PortError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| PortError(format!("json: {e}")))?;
    let chart = &v["chart"];
    if !chart["error"].is_null() {
        return Err(PortError(format!("yahoo error: {}", chart["error"])));
    }
    let r = &chart["result"][0];
    let ts = r["timestamp"]
        .as_array()
        .ok_or_else(|| PortError("no timestamp".into()))?;
    let closes = r["indicators"]["quote"][0]["close"]
        .as_array()
        .ok_or_else(|| PortError("no close".into()))?;

    let mut bars: Vec<PriceBar> = ts
        .iter()
        .zip(closes)
        .filter_map(|(t, c)| {
            Some(PriceBar {
                date: Date::from_unix_jst(t.as_i64()?),
                close: c.as_f64()?,
            })
        })
        .collect();
    bars.sort_by_key(|b| b.date);
    bars.dedup_by_key(|b| b.date);

    let mut dividends: Vec<Dividend> = r["events"]["dividends"]
        .as_object()
        .map(|m| {
            m.values()
                .filter_map(|d| {
                    Some(Dividend {
                        ex_date: Date::from_unix_jst(d["date"].as_i64()?),
                        amount: d["amount"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    dividends.sort_by_key(|d| d.ex_date);

    if bars.is_empty() {
        return Err(PortError("no price bars".into()));
    }
    Ok(MarketData { bars, dividends })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let body = include_bytes!("../../../fixtures/yahoo_chart_sample.json");
        let m = parse_chart(body).unwrap();
        assert_eq!(m.bars.len(), 3); // null の終値は捨てる
        assert_eq!(m.bars[0].date.to_string(), "2026-10-16");
        assert_eq!(m.dividends.len(), 2);
        assert!(m.dividends[0].ex_date < m.dividends[1].ex_date);
    }

    #[test]
    fn reports_api_error() {
        let body = br#"{"chart":{"result":null,"error":{"code":"Not Found","description":"No data found"}}}"#;
        assert!(parse_chart(body).is_err());
    }
}
