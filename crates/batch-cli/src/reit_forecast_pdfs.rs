use crate::{d1_store, env, Args, SystemClock};
use adapter_batch::reit_scoring::{self, Observation};
use adapter_batch::{d1, http, jquants::JQuantsClient, reit_forecast_pdf as pdf};
use domain::{Date, StockCode};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use usecase::fins::{FinancialStore, ForecastReport};
use usecase::Clock;

const CURSOR: &str = "reit_forecast_pdfs_until";

#[derive(Deserialize)]
struct LocalDisclosure {
    #[serde(flatten)]
    disclosure: pdf::Disclosure,
    pdf: PathBuf,
}

pub async fn run(args: Args) -> Result<(), String> {
    let now = SystemClock.now_unix();
    let today = Date::from_unix_jst(now);
    if let Some(manifest) = &args.from_file {
        let (reports, evidence) = local_reports(Path::new(manifest), today, now)?;
        if let Some(out) = &args.dry_run {
            let store = d1::SqlFileStore::default();
            store.save_forecasts(&reports, now).await.map_err(|e| e.0)?;
            std::fs::write(out, store.into_string()).map_err(|e| e.to_string())?;
            println!(
                "reit forecast PDFs: verified {} disclosures (dry run)",
                reports.len()
            );
            return Ok(());
        }
        let store = d1_store(http::client())?;
        let codes: HashSet<_> = store
            .listed_codes(&["reit"])
            .await
            .map_err(|e| e.0)?
            .into_iter()
            .collect();
        if let Some(report) = reports.iter().find(|r| !codes.contains(r.code.as_str())) {
            return Err(format!("{} is not a listed REIT", report.code.as_str()));
        }
        store.save_forecasts(&reports, now).await.map_err(|e| e.0)?;
        reit_scoring::save_observations(&store, &evidence)
            .await
            .map_err(|e| e.0)?;
        println!("reit forecast PDFs: imported {} disclosures", reports.len());
        return Ok(());
    }
    if args.dry_run.is_some() {
        return Err("sync-reit-forecast-pdfs --dry-run requires --from-file".into());
    }

    let client = http::client();
    let jq = JQuantsClient::new(client.clone(), env("JQUANTS_API_KEY")?, args.rate);
    let store = d1_store(client)?;
    let until = args.until.unwrap_or(today);
    let cursor = store
        .get_state(CURSOR)
        .await
        .map_err(|e| e.0)?
        .as_deref()
        .and_then(Date::parse);
    let from = args.from.unwrap_or_else(|| match cursor {
        Some(day) => day.add_days(1).min(until.add_days(-3)),
        None => until.add_days(-210),
    });
    if until > today || from > until {
        return Err(format!("invalid disclosure date range: {from}..{until}"));
    }
    let codes = store.listed_codes(&["reit"]).await.map_err(|e| e.0)?;
    if codes.is_empty() {
        return Err("no listed REITs in the stock master".into());
    }
    let reit_codes: HashSet<_> = codes.iter().cloned().collect();
    let mut rows = Vec::new();
    // 長期の取り直しでは、開示のない日を全市場に問い合わせる代わりに銘柄ごとに取得する。
    if until.to_days() - from.to_days() + 1 > codes.len() as i64 {
        for code in codes.iter().filter_map(|c| StockCode::parse(c)) {
            rows.extend(
                jq.disclosures_for(&code, from, until)
                    .await
                    .map_err(|e| e.0)?,
            );
        }
    } else {
        for day in from.to_days()..=until.to_days() {
            rows.extend(
                jq.disclosures_on(Date::from_days(day))
                    .await
                    .map_err(|e| e.0)?,
            );
        }
    }
    let deleted: HashSet<_> = rows
        .iter()
        .filter(|r| r["DiscStatus"].as_str() == Some("delete"))
        .filter_map(|r| r["DiscNo"].as_str())
        .collect();
    let disclosures: BTreeMap<_, _> = rows
        .iter()
        .filter_map(|row| pdf::from_tdnet(row, &reit_codes))
        .filter(|doc| !deleted.contains(doc.doc_id.as_str()))
        .map(|doc| (doc.doc_id.clone(), doc))
        .collect();
    let mut reports = Vec::new();
    let mut evidence = Vec::new();
    for disclosure in disclosures.values() {
        let bytes = jq
            .disclosure_pdf(&disclosure.doc_id)
            .await
            .map_err(|e| e.0)?;
        let text = pdf::pdf_text(&bytes)?;
        match pdf::forecast_report(disclosure, &text) {
            Some(report) => {
                evidence.extend(pdf::scoring_observations(&report, &text, now));
                reports.push(report);
            }
            None => eprintln!(
                "reit forecast PDFs: unverified table for {} ({})",
                disclosure.code, disclosure.doc_id
            ),
        }
    }
    store.save_forecasts(&reports, now).await.map_err(|e| e.0)?;
    reit_scoring::save_observations(&store, &evidence)
        .await
        .map_err(|e| e.0)?;
    for id in deleted {
        store.exec(&adapter_batch::sql::Stmt {
            sql: "UPDATE reit_metric_observations SET quality_flags=json_array('withdrawn') WHERE method_version='tdnet-pdf-v1' AND source_document_id=?".into(),
            params: vec![adapter_batch::sql::Val::Text(id.to_owned())],
        }).await.map_err(|e| e.0)?;
    }
    // ネットワークや保存の失敗で中断した場合はカーソルを進めず、次回同じ開示を取り直す。
    if cursor.is_none_or(|day| day < until) && args.from.is_none() && args.until.is_none() {
        store
            .set_state(CURSOR, &until.to_string(), now)
            .await
            .map_err(|e| e.0)?;
    }
    println!(
        "reit forecast PDFs: verified {}/{} disclosures ({from}..{until})",
        reports.len(),
        disclosures.len()
    );
    Ok(())
}

fn local_reports(
    manifest: &Path,
    today: Date,
    now: i64,
) -> Result<(Vec<ForecastReport>, Vec<Observation>), String> {
    let bytes = std::fs::read(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let entries: Vec<LocalDisclosure> =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", manifest.display()))?;
    if entries.is_empty() {
        return Err("no disclosures in manifest".into());
    }
    let dir = manifest.parent().unwrap_or_else(|| Path::new("."));
    let mut ids = HashSet::new();
    let mut evidence = Vec::new();
    let reports = entries
        .into_iter()
        .map(|entry| {
            let path = dir.join(entry.pdf);
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let text = pdf::pdf_text(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            let report = pdf::forecast_report(&entry.disclosure, &text)
                .filter(|r| r.disclosed <= today)
                .ok_or_else(|| {
                    format!(
                        "{}: code, disclosure date or forecast table could not be verified",
                        path.display()
                    )
                })?;
            if !ids.insert((report.code.clone(), report.doc_id.clone())) {
                return Err(format!("{}: duplicate disclosure ID", path.display()));
            }
            evidence.extend(pdf::scoring_observations(&report, &text, now));
            Ok(report)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((reports, evidence))
}
