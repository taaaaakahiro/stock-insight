//! スナップショットの保存先。
//! - D1Store: Cloudflare D1 の HTTP API に書き込む（本番）
//! - SqlFileStore: 同じSQLをファイルに書き出す（ドライラン・ローカル検証）

use crate::sql::{
    annual_reports_stmts, delete_edinet_doc_stmt, edinet_docs_stmts, forecasts_stmts,
    period_reports_stmts, price_history_stmts, run_stmt, set_state_stmt, snapshots_stmts,
    splits_stmts, stocks_stmts, Stmt, Val,
};
use domain::analysis::{CumulativePeriod, PeriodType};
use domain::disclosure::DocKind;
use domain::split::Split;
use domain::stock_master::ListedStock;
use domain::{AnnualFinancial, Date, PriceBar};
use std::cell::RefCell;
use std::collections::HashMap;
use usecase::daily::{StoredAnnual, StoredForecast};
use usecase::edinet::{DocKey, DocRef, EdinetDoc};
use usecase::fins::{AnnualReport, FinancialStore, ForecastReport, PeriodReport};
use usecase::master::StockMasterStore;
use usecase::{BatchRun, PortError, Snapshot, SnapshotStore};

pub struct D1Store {
    client: reqwest::Client,
    url: String,
    token: String,
}

impl D1Store {
    pub fn new(
        client: reqwest::Client,
        account_id: &str,
        database_id: &str,
        token: String,
    ) -> Self {
        Self {
            client,
            url: format!("https://api.cloudflare.com/client/v4/accounts/{account_id}/d1/database/{database_id}/query"),
            token,
        }
    }

    pub async fn exec(&self, stmt: &Stmt) -> Result<(), PortError> {
        self.query(stmt).await.map(|_| ())
    }

    /// 1文を実行して結果の行（JSON のオブジェクト）を返す
    pub async fn query(&self, stmt: &Stmt) -> Result<Vec<serde_json::Value>, PortError> {
        let params: Vec<serde_json::Value> = stmt.params.iter().map(|v| v.to_json()).collect();
        let body = serde_json::json!({ "sql": stmt.sql, "params": params });
        let res = self
            .client
            .post(&self.url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| PortError(format!("d1: {e}")))?;
        let status = res.status();
        let v: serde_json::Value = res
            .json()
            .await
            .map_err(|e| PortError(format!("d1: {e}")))?;
        if !status.is_success() || v["success"].as_bool() != Some(true) {
            return Err(PortError(format!("d1 HTTP {status}: {}", v["errors"])));
        }
        Ok(v["result"][0]["results"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// 銘柄マスタにある全銘柄（指定の種類だけ）のコード
    pub async fn listed_codes(&self, kinds: &[&str]) -> Result<Vec<String>, PortError> {
        let placeholders = vec!["?"; kinds.len()].join(", ");
        let rows = self
            .query(&Stmt {
                sql: format!(
                    "SELECT code FROM stocks WHERE kind IN ({placeholders}) ORDER BY code"
                ),
                params: kinds.iter().map(|k| Val::Text(k.to_string())).collect(),
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| r["code"].as_str().map(str::to_string))
            .collect())
    }

    /// いま上場している銘柄（最新の JPX 一覧にある銘柄）のコード。上場廃止の銘柄はマスタに古い as_of で残る
    pub async fn current_codes(&self, kinds: &[&str]) -> Result<Vec<String>, PortError> {
        let placeholders = vec!["?"; kinds.len()].join(", ");
        let rows = self
            .query(&Stmt {
                sql: format!(
                    "SELECT code FROM stocks WHERE kind IN ({placeholders}) \
                     AND as_of = (SELECT MAX(as_of) FROM stocks) ORDER BY code"
                ),
                params: kinds.iter().map(|k| Val::Text(k.to_string())).collect(),
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| r["code"].as_str().map(str::to_string))
            .collect())
    }

    /// 保存済みの EDINET の書類（銘柄・種類ごとに1件）
    pub async fn load_edinet_docs(&self) -> Result<HashMap<DocKey, DocRef>, PortError> {
        let rows = self
            .query(&Stmt {
                sql: "SELECT code, doc_kind, doc_id, submitted, period_end FROM edinet_docs".into(),
                params: vec![],
            })
            .await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let doc = DocRef {
                    code: r["code"].as_str()?.to_string(),
                    kind: DocKind::parse(r["doc_kind"].as_str()?)?,
                    doc_id: r["doc_id"].as_str()?.to_string(),
                    submitted: Date::parse(r["submitted"].as_str()?)?,
                    period_end: r["period_end"].as_str().and_then(Date::parse),
                };
                Some(((doc.code.clone(), doc.kind), doc))
            })
            .collect())
    }

    pub async fn save_edinet_docs(&self, docs: &[EdinetDoc], now: i64) -> Result<(), PortError> {
        for s in edinet_docs_stmts(docs, now) {
            self.exec(&s).await?;
        }
        Ok(())
    }

    pub async fn delete_edinet_docs(&self, keys: &[DocKey]) -> Result<(), PortError> {
        for k in keys {
            self.exec(&delete_edinet_doc_stmt(k)).await?;
        }
        Ok(())
    }

    /// 今日（日本時間）の、指定ラベルのバッチで成功したものがあるか
    pub async fn succeeded_since(&self, label: &str, since: i64) -> Result<bool, PortError> {
        let rows = self
            .query(&Stmt {
                sql: "SELECT COUNT(*) AS n FROM batch_runs WHERE status = 'succeeded' AND run_id LIKE ? AND started_at >= ?"
                    .into(),
                params: vec![
                    Val::Text(format!("{label}-%")),
                    Val::Int(since),
                ],
            })
            .await?;
        Ok(rows.first().and_then(|r| r["n"].as_i64()).unwrap_or(0) > 0)
    }

    /// マイグレーション用: セミコロン区切りのSQLを1文ずつ実行する。
    /// ALTER TABLE ... ADD COLUMN は2回目以降「列がすでにある」で失敗するので、それだけは読み飛ばす
    pub async fn exec_script(&self, script: &str) -> Result<usize, PortError> {
        let stmts = split_sql(script);
        for s in &stmts {
            let r = self
                .exec(&Stmt {
                    sql: s.clone(),
                    params: vec![],
                })
                .await;
            match r {
                Err(e) if is_add_column(s) && e.0.contains("duplicate column") => {}
                other => other?,
            }
        }
        Ok(stmts.len())
    }

    pub async fn get_state(&self, key: &str) -> Result<Option<String>, PortError> {
        let rows = self
            .query(&Stmt {
                sql: "SELECT value FROM sync_state WHERE key = ?".into(),
                params: vec![Val::Text(key.into())],
            })
            .await?;
        Ok(rows
            .first()
            .and_then(|r| r["value"].as_str().map(str::to_string)))
    }

    pub async fn set_state(&self, key: &str, value: &str, now: i64) -> Result<(), PortError> {
        self.exec(&set_state_stmt(key, value, now)).await
    }

    /// 全銘柄の株価の履歴（コード順に少しずつ読む）
    pub async fn load_price_histories(&self) -> Result<HashMap<String, Vec<PriceBar>>, PortError> {
        let mut out = HashMap::new();
        let mut after = String::new();
        loop {
            let rows = self
                .query(&Stmt {
                    sql: "SELECT code, closes FROM price_history WHERE code > ? ORDER BY code LIMIT 200"
                        .into(),
                    params: vec![Val::Text(after.clone())],
                })
                .await?;
            let Some(last) = rows.last().and_then(|r| r["code"].as_str()) else {
                return Ok(out);
            };
            after = last.to_string();
            for r in &rows {
                if let (Some(code), Some(closes)) = (r["code"].as_str(), r["closes"].as_str()) {
                    out.insert(code.to_string(), domain::price_history::decode(closes));
                }
            }
        }
    }

    pub async fn save_price_histories(
        &self,
        rows: &[(String, String, String)],
        now: i64,
    ) -> Result<(), PortError> {
        for s in price_history_stmts(rows, now) {
            self.exec(&s).await?;
        }
        Ok(())
    }

    pub async fn save_splits(&self, rows: &[(String, Split)], now: i64) -> Result<(), PortError> {
        for s in splits_stmts(rows, now) {
            self.exec(&s).await?;
        }
        Ok(())
    }

    /// 全銘柄の株式分割・併合（銘柄ごとに日付の古い順）
    pub async fn load_splits(&self) -> Result<HashMap<String, Vec<Split>>, PortError> {
        let mut out: HashMap<String, Vec<Split>> = HashMap::new();
        let mut after = 0i64;
        loop {
            let rows = self
                .query(&Stmt {
                    sql: "SELECT rowid AS rid, code, date, factor FROM splits                           WHERE rowid > ? ORDER BY rowid LIMIT 2000"
                        .into(),
                    params: vec![Val::Int(after)],
                })
                .await?;
            let Some(last) = rows.last().and_then(|r| r["rid"].as_i64()) else {
                break;
            };
            after = last;
            for r in &rows {
                let split = r["date"]
                    .as_str()
                    .and_then(Date::parse)
                    .zip(r["factor"].as_f64())
                    .and_then(|(d, f)| Split::new(d, f));
                if let (Some(code), Some(s)) = (r["code"].as_str(), split) {
                    out.entry(code.to_string()).or_default().push(s);
                }
            }
        }
        for v in out.values_mut() {
            v.sort_by_key(|s| s.date);
        }
        Ok(out)
    }

    /// 全銘柄の年次財務（rowid 順に少しずつ読む）
    pub async fn load_annuals(&self) -> Result<HashMap<String, Vec<StoredAnnual>>, PortError> {
        let mut out: HashMap<String, Vec<StoredAnnual>> = HashMap::new();
        let mut after = 0i64;
        loop {
            let rows = self
                .query(&Stmt {
                    sql: "SELECT rowid AS rid, code, period_end, doc_id, disclosed, net_sales, operating_income, \
                          ordinary_income, net_income, equity, eps, bps, dps, operating_cf \
                          FROM financials_annual WHERE rowid > ? ORDER BY rowid LIMIT 2000"
                        .into(),
                    params: vec![Val::Int(after)],
                })
                .await?;
            let Some(last) = rows.last().and_then(|r| r["rid"].as_i64()) else {
                return Ok(out);
            };
            after = last;
            for r in &rows {
                let Some(code) = r["code"].as_str() else {
                    continue;
                };
                let date = |k: &str| r[k].as_str().and_then(Date::parse);
                let f = |k: &str| r[k].as_f64();
                out.entry(code.to_string()).or_default().push(StoredAnnual {
                    doc_id: r["doc_id"].as_str().unwrap_or_default().to_string(),
                    disclosed: date("disclosed"),
                    financial: AnnualFinancial {
                        period_end: date("period_end"),
                        net_sales: f("net_sales"),
                        operating_income: f("operating_income"),
                        ordinary_income: f("ordinary_income"),
                        net_income: f("net_income"),
                        equity: f("equity"),
                        eps: f("eps"),
                        bps: f("bps"),
                        dps: f("dps"),
                        operating_cf: f("operating_cf"),
                    },
                });
            }
        }
    }
}

impl D1Store {
    /// 全銘柄の四半期の累計値（rowid 順に少しずつ読む）
    pub async fn load_quarterlies(
        &self,
    ) -> Result<HashMap<String, Vec<CumulativePeriod>>, PortError> {
        let mut out: HashMap<String, Vec<CumulativePeriod>> = HashMap::new();
        let mut after = 0i64;
        loop {
            let rows = self
                .query(&Stmt {
                    sql: "SELECT rowid AS rid, code, fiscal_year_end, period_type, period_end, net_sales, \
                          operating_income, ordinary_income, net_income \
                          FROM financials_quarterly WHERE rowid > ? ORDER BY rowid LIMIT 2000"
                        .into(),
                    params: vec![Val::Int(after)],
                })
                .await?;
            let Some(last) = rows.last().and_then(|r| r["rid"].as_i64()) else {
                return Ok(out);
            };
            after = last;
            for r in &rows {
                let (Some(code), Some(fy), Some(pt), Some(pe)) = (
                    r["code"].as_str(),
                    r["fiscal_year_end"].as_str().and_then(Date::parse),
                    r["period_type"].as_str().and_then(PeriodType::from_jquants),
                    r["period_end"].as_str().and_then(Date::parse),
                ) else {
                    continue;
                };
                let f = |k: &str| r[k].as_f64();
                out.entry(code.to_string())
                    .or_default()
                    .push(CumulativePeriod {
                        fiscal_year_end: fy,
                        period_end: pe,
                        period_type: pt,
                        net_sales: f("net_sales"),
                        operating_income: f("operating_income"),
                        ordinary_income: f("ordinary_income"),
                        net_income: f("net_income"),
                    });
            }
        }
    }
}

impl D1Store {
    /// 全銘柄の会社予想の配当（rowid 順に少しずつ読む）
    pub async fn load_forecasts(&self) -> Result<HashMap<String, Vec<StoredForecast>>, PortError> {
        let mut out: HashMap<String, Vec<StoredForecast>> = HashMap::new();
        let mut after = 0i64;
        loop {
            let rows = self
                .query(&Stmt {
                    sql: "SELECT rowid AS rid, code, doc_id, disclosed, dps \
                          FROM dividend_forecasts WHERE rowid > ? ORDER BY rowid LIMIT 2000"
                        .into(),
                    params: vec![Val::Int(after)],
                })
                .await?;
            let Some(last) = rows.last().and_then(|r| r["rid"].as_i64()) else {
                return Ok(out);
            };
            after = last;
            for r in &rows {
                let (Some(code), Some(disclosed)) = (
                    r["code"].as_str(),
                    r["disclosed"].as_str().and_then(Date::parse),
                ) else {
                    continue;
                };
                out.entry(code.to_string())
                    .or_default()
                    .push(StoredForecast {
                        doc_id: r["doc_id"].as_str().unwrap_or_default().to_string(),
                        disclosed,
                        dps: r["dps"].as_f64(),
                    });
            }
        }
    }
}

fn is_add_column(sql: &str) -> bool {
    let s = sql.to_ascii_uppercase();
    s.starts_with("ALTER TABLE") && s.contains("ADD COLUMN")
}

impl FinancialStore for D1Store {
    async fn save_annual_reports(
        &self,
        reports: &[AnnualReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        for s in annual_reports_stmts(reports, fetched_at) {
            self.exec(&s).await?;
        }
        Ok(())
    }
    async fn save_period_reports(
        &self,
        reports: &[PeriodReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        for s in period_reports_stmts(reports, fetched_at) {
            self.exec(&s).await?;
        }
        Ok(())
    }
    async fn save_forecasts(
        &self,
        reports: &[ForecastReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        for s in forecasts_stmts(reports, fetched_at) {
            self.exec(&s).await?;
        }
        Ok(())
    }
}

impl SnapshotStore for D1Store {
    async fn save_run(&self, run: &BatchRun) -> Result<(), PortError> {
        self.exec(&run_stmt(run)).await
    }
    async fn save_snapshots(&self, snaps: &[Snapshot]) -> Result<(), PortError> {
        for s in snapshots_stmts(snaps) {
            self.exec(&s).await?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SqlFileStore {
    buf: RefCell<String>,
}

impl SqlFileStore {
    pub fn into_string(self) -> String {
        self.buf.into_inner()
    }
    fn push(&self, s: &Stmt) {
        let mut b = self.buf.borrow_mut();
        b.push_str(&s.render());
        b.push('\n');
    }
}

impl SnapshotStore for SqlFileStore {
    async fn save_run(&self, run: &BatchRun) -> Result<(), PortError> {
        self.push(&run_stmt(run));
        Ok(())
    }
    async fn save_snapshots(&self, snaps: &[Snapshot]) -> Result<(), PortError> {
        snapshots_stmts(snaps).iter().for_each(|s| self.push(s));
        Ok(())
    }
}

impl FinancialStore for SqlFileStore {
    async fn save_annual_reports(
        &self,
        reports: &[AnnualReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        annual_reports_stmts(reports, fetched_at)
            .iter()
            .for_each(|s| self.push(s));
        Ok(())
    }
    async fn save_period_reports(
        &self,
        reports: &[PeriodReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        period_reports_stmts(reports, fetched_at)
            .iter()
            .for_each(|s| self.push(s));
        Ok(())
    }
    async fn save_forecasts(
        &self,
        reports: &[ForecastReport],
        fetched_at: i64,
    ) -> Result<(), PortError> {
        forecasts_stmts(reports, fetched_at)
            .iter()
            .for_each(|s| self.push(s));
        Ok(())
    }
}

impl StockMasterStore for D1Store {
    async fn save_stocks(
        &self,
        as_of: &str,
        stocks: &[ListedStock],
        now: i64,
    ) -> Result<(), PortError> {
        for s in stocks_stmts(as_of, stocks, now) {
            self.exec(&s).await?;
        }
        Ok(())
    }
}

impl StockMasterStore for SqlFileStore {
    async fn save_stocks(
        &self,
        as_of: &str,
        stocks: &[ListedStock],
        now: i64,
    ) -> Result<(), PortError> {
        stocks_stmts(as_of, stocks, now)
            .iter()
            .for_each(|s| self.push(s));
        Ok(())
    }
}

/// コメント行を除き、セミコロンで文を分割する（マイグレーションSQLは文字列内にセミコロンを含めない前提）
pub fn split_sql(script: &str) -> Vec<String> {
    let cleaned: String = script
        .lines()
        .filter(|l| !l.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
        .split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
