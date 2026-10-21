//! オフラインの通し試験: 取得（フィクスチャ）→ 指標計算 → SQL 出力。
//! 出力した SQL は scripts/verify_sql.py で SQLite に流してスキーマとの整合を確かめる。
#![allow(async_fn_in_trait)]

use adapter_batch::{d1::SqlFileStore, edinet, yahoo};
use domain::{Date, FinancialData, MarketData, StockCode};
use usecase::{
    ingest_one, BatchRun, Clock, FinancialSource, Outcome, PortError, PriceSource, SnapshotStore,
};

struct FixturePrices;
impl PriceSource for FixturePrices {
    async fn fetch_market(&self, _: &StockCode) -> Result<MarketData, PortError> {
        yahoo::parse_chart(include_bytes!("../../../fixtures/yahoo_chart_sample.json"))
    }
}

struct FixtureFins;
impl FinancialSource for FixtureFins {
    async fn fetch_financials(&self, code: &StockCode) -> Result<Option<FinancialData>, PortError> {
        if code.as_str() == "9998" {
            return Ok(None); // 有報が見つからない銘柄
        }
        let text = include_str!("../../../fixtures/edinet_xbrl_sample.tsv");
        let rows = edinet::parse_xbrl_csv(text);
        Ok(Some(FinancialData {
            doc_id: "S100TEST".into(),
            submitted: Date::parse("2026-06-20"),
            years: edinet::extract_years(&rows, Date::parse("2026-03-31")),
        }))
    }
}

struct Fixed;
impl Clock for Fixed {
    fn now_unix(&self) -> i64 {
        1_792_500_000
    }
}

#[tokio::test(flavor = "current_thread")]
async fn offline_pipeline_writes_sql() {
    let store = SqlFileStore::default();
    let mut run = BatchRun {
        run_id: "offline".into(),
        started_at: 1,
        finished_at: None,
        target: 2,
        succeeded: 0,
        failed: 0,
        status: "running",
    };
    store.save_run(&run).await.unwrap();
    for c in ["9999", "9998"] {
        let code = StockCode::parse(c).unwrap();
        match ingest_one(
            &code,
            "offline",
            &FixturePrices,
            &FixtureFins,
            &store,
            &Fixed,
        )
        .await
        {
            Outcome::Saved { .. } => run.succeeded += 1,
            Outcome::Failed(e) => panic!("{c}: {e}"),
        }
    }
    run.finished_at = Some(2);
    run.status = "succeeded";
    store.save_run(&run).await.unwrap();

    let sql = store.into_string();
    assert!(sql.contains("INSERT INTO indicators_latest"));
    assert_eq!(sql.matches("INSERT INTO financials_annual").count(), 5);
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/poc_offline.sql");
    std::fs::write(out, sql).unwrap();
}
