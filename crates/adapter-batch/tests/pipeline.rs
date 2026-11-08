//! オフラインの通し試験: J-Quants の応答（フィクスチャ）→ 年次財務・株価の履歴 → 指標計算 → SQL 出力。
//! 出力した SQL は scripts/verify_sql.py で SQLite に流してスキーマとの整合を確かめる。
#![allow(async_fn_in_trait)]

use adapter_batch::{d1::SqlFileStore, jquants};
use domain::{Date, PriceBar, StockCode};
use std::collections::HashMap;
use usecase::daily::{build_inputs, compute_all, StoredAnnual, StoredFinancials, StoredForecast};
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
                breakdown: Some(Default::default()),
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
        &StoredFinancials {
            annuals,
            ..Default::default()
        },
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

#[tokio::test(flavor = "current_thread")]
async fn reit_forecasts_feed_the_snapshot_and_detail_history_on_the_same_basis() {
    let d = |value| Date::parse(value).unwrap();
    let rows = serde_json::json!([
        {
            "Code": "89510", "DocType": "REITDividendForecastRevision", "DiscNo": "1",
            "DiscDate": "2026-04-01", "DiscTime": "15:30:00",
            "CurFYSt": "2026-04-01", "CurFYEn": "2026-09-30", "FDivUnit": "6000"
        },
        {
            "Code": "89510", "DocType": "REITDividendForecastRevision", "DiscNo": "2",
            "DiscDate": "2026-06-01", "DiscTime": "15:30:00",
            "CurFYSt": "2026-04-01", "CurFYEn": "2026-09-30", "FDivUnit": "3000",
            "NxtFYSt": "2026-10-01", "NxtFYEn": "2027-03-31", "NxFDivUnit": "2500"
        }
    ]);
    let reports = jquants::forecast_reports(rows.as_array().unwrap());
    assert_eq!(reports.len(), 2);
    let code = reports[0].code.clone();
    let forecasts: Vec<_> = reports
        .iter()
        .map(|f| StoredForecast {
            doc_id: f.doc_id.clone(),
            disclosed: f.disclosed,
            fiscal_year_end: f.fiscal_year_end,
            dps: f.dps,
            reit: f.reit.clone(),
            breakdown: None,
        })
        .collect();
    let mut bars = vec![PriceBar {
        date: d("2026-04-15"),
        close: 200_000.0,
    }];
    bars.extend((0..5).map(|n| PriceBar {
        date: d("2026-06-15").add_days(n),
        close: 100_000.0,
    }));
    let splits = vec![domain::split::Split::new(d("2026-05-01"), 0.5).unwrap()];
    let (market, _, _, _) = build_inputs(bars.clone(), &[], &forecasts, &[], &splits, true);
    assert!(market
        .reit_distributions
        .as_ref()
        .unwrap()
        .at(bars[0].date)
        .is_none());
    assert_eq!(domain::yield_series(&market), vec![(bars[5].date, 0.055)]);
    let key = code.as_str().to_owned();
    let (snaps, skipped) = compute_all(
        &[code],
        HashMap::from([(key.clone(), bars)]),
        &StoredFinancials {
            forecasts: HashMap::from([(key.clone(), forecasts)]),
            splits: HashMap::from([(key.clone(), splits)]),
            reit_codes: [key].into(),
            ..Default::default()
        },
        "reit-offline",
        1,
    );
    assert_eq!(skipped, 0);
    let i = &snaps[0].indicators;
    assert_eq!(i.dividend_ttm, 5500.0);
    assert_eq!(i.dividend_yield, Some(0.055));
    assert_eq!(i.dividend_is_forecast, Some(true));
    let store = SqlFileStore::default();
    store.save_forecasts(&reports, 1).await.unwrap();
    store.save_snapshots(&snaps).await.unwrap();
    let sql = store.into_string();
    let out = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/reit_yield_offline.sql"
    );
    std::fs::write(out, sql).unwrap();
}
