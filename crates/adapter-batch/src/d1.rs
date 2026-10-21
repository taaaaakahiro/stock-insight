//! スナップショットの保存先。
//! - D1Store: Cloudflare D1 の HTTP API に書き込む（本番）
//! - SqlFileStore: 同じSQLをファイルに書き出す（ドライラン・ローカル検証）

use crate::sql::{run_stmt, snapshot_stmts, stocks_stmts, Stmt};
use domain::stock_master::ListedStock;
use std::cell::RefCell;
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
        Ok(())
    }

    /// マイグレーション用: セミコロン区切りのSQLを1文ずつ実行する
    pub async fn exec_script(&self, script: &str) -> Result<usize, PortError> {
        let stmts = split_sql(script);
        for s in &stmts {
            self.exec(&Stmt {
                sql: s.clone(),
                params: vec![],
            })
            .await?;
        }
        Ok(stmts.len())
    }
}

impl SnapshotStore for D1Store {
    async fn save_run(&self, run: &BatchRun) -> Result<(), PortError> {
        self.exec(&run_stmt(run)).await
    }
    async fn save_snapshot(&self, snap: &Snapshot) -> Result<(), PortError> {
        for s in snapshot_stmts(snap) {
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
    async fn save_snapshot(&self, snap: &Snapshot) -> Result<(), PortError> {
        snapshot_stmts(snap).iter().for_each(|s| self.push(s));
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
