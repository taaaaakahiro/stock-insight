//! オフラインの通し試験: J-Quants の応答（フィクスチャ）→ 年次財務・株価の履歴 → 指標計算 → SQL 出力。
//! 出力した SQL は scripts/verify_sql.py で SQLite に流してスキーマとの整合を確かめる。
#![allow(async_fn_in_trait)]

use adapter_batch::{d1::SqlFileStore, jquants};
use domain::{Date, PriceBar, StockCode};
use std::collections::HashMap;
use usecase::daily::{compute_all, StoredAnnual};
use usecase::fins::FinancialStore;
use usecase::{BatchRun, SnapshotStore};

#[tokio::test(flavor = "current_thread")]
async fn offline_pipeline_writes_sql() {
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/jquants_fins_summary_sample.json"
    ))
    .unwrap();
    let reports: Vec<_> = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(jquants::annual_report)
        .collect();
    assert_eq!(reports.len(), 2);

    let store = SqlFileStore::default();
    store.save_annual_reports(&reports, 1).await.unwrap();

    // 2年分の平日の終値（一定）。配当は 2025-04-30 の開示から有効
    let mut bars = vec![];
    let mut day = Date::parse("2024-07-01").unwrap();
    while day <= Date::parse("2026-07-01").unwrap() {
        if (1..=5).contains(&domain::calendar::weekday(day)) {
            bars.push(PriceBar {
                date: day,
                close: 2000.0,
            });
        }
        day = day.add_days(1);
    }
    let mut histories = HashMap::new();
    histories.insert("8697".to_string(), bars);
    let mut annuals: HashMap<String, Vec<StoredAnnual>> = HashMap::new();
    for r in &reports {
        annuals
            .entry(r.code.as_str().to_string())
            .or_default()
            .push(StoredAnnual {
                doc_id: r.doc_id.clone(),
                disclosed: r.disclosed,
                financial: r.financial.clone(),
            });
    }
    let codes: Vec<StockCode> = ["8697", "8951"]
        .iter()
        .map(|c| StockCode::parse(c).unwrap())
        .collect();
    let periods = jquants::period_reports(v["data"].as_array().unwrap());
    store.save_period_reports(&periods, 1).await.unwrap();
    let (snaps, skipped) = compute_all(
        &codes,
        histories,
        &annuals,
        &HashMap::new(),
        "offline",
        1_792_500_000,
    );
    assert_eq!(snaps.len(), 1);
    assert_eq!(skipped, 1, "8951 は株価の履歴がない");
    let i = &snaps[0].indicators;
    assert!((i.dividend_yield.unwrap() - 62.0 / 2000.0).abs() < 1e-9);

    store.save_snapshots(&snaps).await.unwrap();
    store
        .save_run(&BatchRun {
            run_id: "offline".into(),
            started_at: 1,
            finished_at: Some(2),
            target: 2,
            succeeded: 1,
            failed: 1,
            status: "partial",
        })
        .await
        .unwrap();

    let sql = store.into_string();
    assert!(sql.contains("INSERT INTO indicators_latest"));
    assert!(sql.contains("INSERT INTO financials_annual"));
    assert!(
        sql.contains("INSERT INTO financials_quarterly"),
        "3Q の行が入る"
    );
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/poc_offline.sql");
    std::fs::write(out, sql).unwrap();
}
