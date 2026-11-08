//! Readers bind to a single completed snapshot for the whole response.
use serde::Deserialize;
use serde_json::{json, Value};
use worker::{D1Database, Error, Result};

#[derive(Deserialize)]
pub struct Run {
    pub run_id: String,
    as_of_date: String,
    score_version: String,
    universe_count: u32,
    universe_definition: String,
    completed_at: i64,
    source_watermark: String,
}

impl Run {
    pub fn json(&self) -> Value {
        json!({
            "run_id":self.run_id,"as_of_date":self.as_of_date,"score_version":self.score_version,
            "universe_count":self.universe_count,"universe_definition":self.universe_definition,
            "completed_at":self.completed_at,"source_watermark":self.source_watermark,
            "tie_break":"unrounded score DESC, coverage DESC, ticker ASC"
        })
    }
}

pub async fn run(db: &D1Database, as_of: Option<&str>) -> Result<Option<Run>> {
    db.prepare(
        "SELECT run_id,as_of_date,score_version,universe_count,universe_definition,completed_at,source_watermark \
         FROM reit_scoring_runs WHERE score_version=?1 AND status='ready' AND as_of_date<=?2 \
         ORDER BY as_of_date DESC,completed_at DESC,run_id DESC LIMIT 1",
    ).bind(&[domain::reit_scoring::VERSION.into(),as_of.unwrap_or("9999-12-31").into()])?.first(None).await
}

pub async fn run_by_id(db: &D1Database, id: &str) -> Result<Option<Run>> {
    db.prepare(
        "SELECT run_id,as_of_date,score_version,universe_count,universe_definition,completed_at,source_watermark \
         FROM reit_scoring_runs WHERE run_id=?1 AND score_version=?2 AND status='ready'",
    ).bind(&[id.into(),domain::reit_scoring::VERSION.into()])?.first(None).await
}

#[derive(Deserialize)]
struct Row {
    summary_json: String,
    detail_json: Option<String>,
}

fn parse(s: &str) -> Result<Value> {
    serde_json::from_str(s).map_err(|_| Error::RustError("invalid saved J-REIT score JSON".into()))
}

pub async fn rankings(db: &D1Database, run: &Run) -> Result<Vec<Value>> {
    let rows = db.prepare("SELECT summary_json,NULL AS detail_json FROM reit_scores WHERE run_id=? ORDER BY ticker")
        .bind(&[run.run_id.as_str().into()])?.all().await?.results::<Row>()?;
    rows.iter().map(|r| parse(&r.summary_json)).collect()
}

pub async fn details(db: &D1Database, run: &Run, codes: &[String]) -> Result<Vec<Value>> {
    let mut items = Vec::with_capacity(codes.len());
    // Querying with the same immutable run_id keeps detail and comparisons consistent.
    for code in codes {
        let row = db
            .prepare("SELECT summary_json,detail_json FROM reit_scores WHERE run_id=? AND ticker=?")
            .bind(&[run.run_id.as_str().into(), code.as_str().into()])?
            .first::<Row>(None)
            .await?;
        if let Some(row) = row {
            items.push(json!({"summary":parse(&row.summary_json)?,"detail":parse(row.detail_json.as_deref().unwrap_or("null"))?}));
        }
    }
    Ok(items)
}

pub async fn listed(db: &D1Database, codes: &[String]) -> Result<bool> {
    for code in codes {
        if db
            .prepare("SELECT code FROM stocks WHERE code=? AND kind='reit'")
            .bind(&[code.as_str().into()])?
            .first::<String>(Some("code"))
            .await?
            .is_none()
        {
            return Ok(false);
        }
    }
    Ok(true)
}
