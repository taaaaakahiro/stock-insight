//! J-Quants API（V2）: 株価四本値（/equities/bars/daily）と財務情報サマリー（/fins/summary）。
//!
//! どちらも日付を指定すると全上場銘柄分をまとめて返す（ページ分割あり）。
//! 呼び出しの上限はプランごとに違う（Free 5件/分、Light 60件/分）ので、間隔は設定で渡す。

use crate::http;
use domain::{AnnualFinancial, Date, PriceBar, StockCode};
use serde_json::Value;
use std::cell::Cell;
use std::time::{Duration, Instant};
use usecase::fins::{AnnualReport, FinancialSummarySource};
use usecase::PortError;

const BASE: &str = "https://api.jquants.com/v2";

pub struct JQuantsClient {
    client: reqwest::Client,
    key: String,
    /// 呼び出しの最小間隔（Light は1分60件なので約1.5秒、Free は1分5件なので12.5秒）
    min_interval: Duration,
    last_call: Cell<Option<Instant>>,
}

impl JQuantsClient {
    pub fn new(client: reqwest::Client, key: String, calls_per_minute: u32) -> Self {
        let per_min = calls_per_minute.max(1) as u64;
        Self {
            client,
            key,
            // 境界ちょうどで弾かれないよう、少しだけ余裕を持たせる
            min_interval: Duration::from_millis(60_000 / per_min + 500),
            last_call: Cell::new(None),
        }
    }

    async fn pace(&self) {
        if let Some(last) = self.last_call.get() {
            let elapsed = last.elapsed();
            if elapsed < self.min_interval {
                tokio::time::sleep(self.min_interval - elapsed).await;
            }
        }
        self.last_call.set(Some(Instant::now()));
    }

    async fn get(&self, url: &str) -> Result<Vec<u8>, PortError> {
        // 制限に当たったら（429）1分待って、あと2回まで試す
        for attempt in 0..3 {
            self.pace().await;
            match http::get_bytes_with_headers(&self.client, url, &[("x-api-key", &self.key)]).await
            {
                Err(e) if e.0.contains("HTTP 429") && attempt < 2 => {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                other => return other,
            }
        }
        unreachable!()
    }

    /// ページ分割をたどって data をすべて集める。`path` はクエリ付き（例: "/fins/summary?date=2026-10-21"）
    async fn fetch_all(&self, path: &str) -> Result<Vec<Value>, PortError> {
        let mut rows = vec![];
        let mut key: Option<String> = None;
        loop {
            let mut url = format!("{BASE}{path}");
            if let Some(k) = &key {
                url.push_str("&pagination_key=");
                url.push_str(&percent_encode(k));
            }
            let body = self.get(&url).await?;
            let v: Value =
                serde_json::from_slice(&body).map_err(|e| PortError(format!("jquants: {e}")))?;
            if let Some(data) = v["data"].as_array() {
                rows.extend(data.iter().cloned());
            }
            match v["pagination_key"].as_str() {
                Some(k) if !k.is_empty() => key = Some(k.to_string()),
                _ => return Ok(rows),
            }
        }
    }

    /// 指定日に開示された全銘柄の財務情報サマリー
    pub async fn summaries_on(&self, date: Date) -> Result<Vec<Value>, PortError> {
        self.fetch_all(&format!("/fins/summary?date={date}")).await
    }

    /// 指定日の全上場銘柄の終値（分割調整前）。休場日は空
    pub async fn closes_on(&self, date: Date) -> Result<Vec<(StockCode, PriceBar)>, PortError> {
        Ok(self
            .fetch_all(&format!("/equities/bars/daily?date={date}"))
            .await?
            .iter()
            .filter_map(close_bar)
            .collect())
    }
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 日足の1行 → (銘柄, 終値)。約定がない日（終値が null）は None
pub fn close_bar(r: &Value) -> Option<(StockCode, PriceBar)> {
    let code = code_from_jquants(r["Code"].as_str()?)?;
    let date = Date::parse(r["Date"].as_str()?)?;
    // 利回りの計算では、開示された（分割調整前の）配当と合わせるため調整前の終値を使う
    let close = r["C"].as_f64().filter(|c| c.is_finite() && *c > 0.0)?;
    Some((code, PriceBar { date, close }))
}

impl FinancialSummarySource for JQuantsClient {
    async fn annual_reports_on(&self, date: Date) -> Result<Vec<AnnualReport>, PortError> {
        Ok(self
            .summaries_on(date)
            .await?
            .iter()
            .filter_map(annual_report)
            .collect())
    }
}

/// J-Quants の5桁コード（末尾が 0）→ 4桁。英字入りのコード（例: 130A0）も同じ
pub fn code_from_jquants(code: &str) -> Option<StockCode> {
    let c = code.trim();
    let four = if c.len() == 5 {
        c.strip_suffix('0')?
    } else {
        c
    };
    StockCode::parse(four)
}

/// 数値の文字列（空文字・"-" は値なし）
fn num(v: &Value) -> Option<f64> {
    let s = v.as_str()?.trim();
    if s.is_empty() || s == "-" {
        return None;
    }
    s.replace(',', "")
        .parse()
        .ok()
        .filter(|x: &f64| x.is_finite())
}

/// 本決算（CurPerType = FY）の行だけを年次の財務にする。四半期の行は使わない
pub fn annual_report(r: &Value) -> Option<AnnualReport> {
    if r["CurPerType"].as_str()? != "FY" {
        return None;
    }
    let code = code_from_jquants(r["Code"].as_str()?)?;
    let period_end = Date::parse(r["CurFYEn"].as_str().or(r["CurPerEn"].as_str())?)?;
    let first = |keys: &[&str]| keys.iter().find_map(|k| num(&r[*k]));
    Some(AnnualReport {
        code,
        doc_id: r["DiscNo"].as_str().unwrap_or_default().to_string(),
        disclosed: r["DiscDate"].as_str().and_then(Date::parse),
        financial: AnnualFinancial {
            period_end: Some(period_end),
            net_sales: first(&["Sales", "NCSales"]),
            operating_income: first(&["OP", "NCOP"]),
            ordinary_income: first(&["OdP", "NCOdP"]),
            net_income: first(&["NP", "NCNP"]),
            // 自己資本（なければ純資産）
            equity: first(&["ShEq", "Eq", "NCShEq", "NCEq"]),
            eps: first(&["EPS", "NCEPS"]),
            bps: first(&["BPS", "NCBPS"]),
            // REIT・ETF は1口当たり分配金
            dps: first(&["DivAnn", "DivUnit"]),
            operating_cf: num(&r["CFO"]),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Value {
        serde_json::from_str(include_str!(
            "../../../fixtures/jquants_fins_summary_sample.json"
        ))
        .unwrap()
    }

    #[test]
    fn keeps_only_full_year_reports() {
        let v = rows();
        let reports: Vec<_> = v["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(annual_report)
            .collect();
        assert_eq!(reports.len(), 2, "四半期（3Q）は除く");
        let a = &reports[0];
        assert_eq!(a.code.as_str(), "8697");
        assert_eq!(a.doc_id, "20250430500001");
        assert_eq!(a.financial.period_end.unwrap().to_string(), "2025-03-31");
        assert_eq!(a.financial.net_sales, Some(162_000_000_000.0));
        assert_eq!(a.financial.eps, Some(70.5));
        assert_eq!(a.financial.dps, Some(62.0));
        assert_eq!(
            a.financial.equity,
            Some(380_000_000_000.0),
            "自己資本を優先"
        );
        assert_eq!(a.financial.operating_cf, None, "空文字は値なし");
    }

    #[test]
    fn reit_uses_distribution_per_unit() {
        let v = rows();
        let reports: Vec<_> = v["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(annual_report)
            .collect();
        let reit = &reports[1];
        assert_eq!(reit.code.as_str(), "8951");
        assert_eq!(reit.financial.dps, Some(18_500.0));
    }

    #[test]
    fn converts_codes() {
        assert_eq!(code_from_jquants("86970").unwrap().as_str(), "8697");
        assert_eq!(code_from_jquants("130A0").unwrap().as_str(), "130A");
        assert_eq!(code_from_jquants("8697").unwrap().as_str(), "8697");
        assert!(
            code_from_jquants("25935").is_none(),
            "優先株（末尾が0でない）は対象外"
        );
    }

    #[test]
    fn parses_daily_bars() {
        let v: Value = serde_json::from_str(
            r#"{"data":[
              {"Date":"2026-10-21","Code":"72030","O":3080,"H":3100,"L":3070,"C":3094.0,"AdjC":3094.0},
              {"Date":"2026-10-21","Code":"130A0","C":512},
              {"Date":"2026-10-21","Code":"99990","C":null}
            ]}"#,
        )
        .unwrap();
        let bars: Vec<_> = v["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(close_bar)
            .collect();
        assert_eq!(bars.len(), 2, "約定なし（null）は除く");
        assert_eq!(bars[0].0.as_str(), "7203");
        assert_eq!(bars[0].1.close, 3094.0);
        assert_eq!(bars[1].0.as_str(), "130A");
        assert_eq!(percent_encode("a b/+="), "a%20b%2F%2B%3D");
    }

    #[test]
    fn pacing_interval_for_free_plan() {
        let c = JQuantsClient::new(reqwest::Client::new(), String::new(), 5);
        assert_eq!(c.min_interval, Duration::from_millis(12_500));
    }
}
