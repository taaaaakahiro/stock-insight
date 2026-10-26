//! Read only published RAG snapshots; raw inputs and failed output are never served.
use serde::Deserialize;
use serde_json::{json, Value};
use worker::D1Database;

#[derive(Deserialize)]
struct Row {
    body: String,
}
#[derive(Deserialize)]
struct Publication {
    run_id: String,
    cutoff: i64,
    price_date: String,
}

#[derive(Deserialize)]
struct Attempt {
    value: String,
    updated_at: i64,
}

pub async fn read(db: &D1Database, code: Option<&str>) -> worker::Result<Value> {
    let attempt = db
        .prepare("SELECT value,updated_at FROM sync_state WHERE key='reit_rag_attempt_status'")
        .first::<Attempt>(None)
        .await?;
    let update_status = attempt
        .as_ref()
        .map(|a| a.value.as_str())
        .unwrap_or("not_started");
    let attempted_at = attempt.as_ref().map(|a| a.updated_at);
    let publication=db.prepare("SELECT p.run_id,p.cutoff,r.price_date FROM reit_rag_publication p JOIN reit_rag_runs r ON r.id=p.run_id WHERE p.singleton=1 AND r.status='ready'").first::<Publication>(None).await?;
    let Some(p) = publication else {
        return Ok(
            json!({"status":"not_generated","update_status":update_status,"attempted_at":attempted_at,"items":[]}),
        );
    };
    let query=match code {
        Some(code)=>db.prepare("SELECT body FROM reit_rag_results WHERE run_id=? AND kind='stock' AND subject=?").bind(&[p.run_id.clone().into(),code.into()])?,
        None=>db.prepare("SELECT body FROM reit_rag_results WHERE run_id=? AND kind='sector' ORDER BY subject").bind(&[p.run_id.clone().into()])?,
    };
    let rows = query.all().await?.results::<Row>()?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            serde_json::from_str(&r.body)
                .map_err(|_| worker::Error::RustError("invalid saved RAG JSON".into()))
        })
        .collect::<worker::Result<_>>()?;
    Ok(
        json!({"status":"ready","update_status":update_status,"attempted_at":attempted_at,"run_id":p.run_id,"generated_at":p.cutoff,"price_date":p.price_date,"items":items}),
    )
}
