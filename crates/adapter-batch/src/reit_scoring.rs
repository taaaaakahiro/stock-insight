//! Point-in-time evidence and D1 publication for the J-REIT score.
mod edinet_income;
mod inputs;
pub use edinet_income::METHOD as EDINET_INCOME_METHOD;

pub use inputs::edinet_observations;
pub(crate) use inputs::{jq_actual, pdf_forecast};

use crate::d1::D1Store;
use crate::sql::{Stmt, Val};
use domain::reit_scoring::{self as rules, CATEGORIES, METRICS, VERSION};
use domain::{Date, StockCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use usecase::PortError;

const UNIVERSE: &str = "JPX Listed Issues (REITs), excluding future listings";
const TIE_BREAK: &str = "unrounded score DESC, coverage DESC, ticker ASC";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub ticker: String,
    pub metric_key: String,
    pub metric_value: Option<f64>,
    pub unit: String,
    pub period_start: Option<String>,
    pub period_end: String,
    pub fiscal_period_end: String,
    /// Unit basis for raw per-unit observations, otherwise the measurement date.
    pub as_of_date: String,
    pub source_url: String,
    pub source_document_id: Option<String>,
    pub source_page: Option<u32>,
    pub disclosure_published_at: String,
    pub retrieved_at: i64,
    pub data_kind: String,
    pub method_version: String,
    #[serde(default)]
    pub normalization_flags: Vec<String>,
    #[serde(default)]
    pub quality_flags: Vec<String>,
    pub revision_id: String,
}

pub fn strict_date(s: &str) -> Option<Date> {
    let d = Date::parse(s)?;
    (d.y >= 1900 && d.to_string() == s && Date::from_days(d.to_days()) == d).then_some(d)
}

fn publication_key(s: &str) -> Option<(&str, &str)> {
    let day = s.get(..10)?;
    strict_date(day)?;
    if s.len() == 10 {
        return Some((day, ""));
    }
    let time = s.get(11..19)?;
    if s.len() != 19
        || s.as_bytes()[10] != b'T'
        || time.as_bytes()[2] != b':'
        || time.as_bytes()[5] != b':'
    {
        return None;
    }
    let n = |i: usize| {
        time.get(i..i + 2)
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))?
            .parse::<u32>()
            .ok()
    };
    (n(0)? < 24 && n(3)? < 60 && n(6)? < 60).then_some((day, time))
}

impl Observation {
    fn end(&self) -> Date {
        strict_date(&self.fiscal_period_end).expect("validated period")
    }

    fn disclosed(&self) -> Date {
        Date::parse(&self.disclosure_published_at).expect("validated disclosure")
    }

    fn order(&self) -> (&str, &str) {
        (&self.disclosure_published_at, &self.revision_id)
    }

    fn trusted(&self) -> bool {
        self.data_kind != "estimated" && self.quality_flags.is_empty()
    }

    fn validate(&self) -> bool {
        let (Some(start), Some(end), Some(fiscal), Some(basis), Some((published, _))) = (
            self.period_start
                .as_deref()
                .map_or(Some(None), |d| strict_date(d).map(Some)),
            strict_date(&self.period_end),
            strict_date(&self.fiscal_period_end),
            strict_date(&self.as_of_date),
            publication_key(&self.disclosure_published_at),
        ) else {
            return false;
        };
        let source = reqwest::Url::parse(&self.source_url).ok();
        StockCode::parse(&self.ticker).is_some_and(|c| c.as_str() == self.ticker)
            && source.is_some_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some_and(|host| host.contains('.'))
                    && url.username().is_empty()
                    && url.password().is_none()
                    && !self.source_url.chars().any(char::is_whitespace)
            })
            && inputs::unit(&self.metric_key) == Some(self.unit.as_str())
            && !self.method_version.trim().is_empty()
            && !self.revision_id.trim().is_empty()
            && self.source_page.is_none_or(|p| p > 0)
            && self
                .metric_value
                .is_none_or(|v| inputs::valid_raw_value(&self.metric_key, v))
            && end == fiscal
            && start.is_none_or(|start| start <= end)
            && (!matches!(
                self.metric_key.as_str(),
                "period_noi" | "period_interest" | "period_ebitda"
            ) || self.metric_value.is_none()
                || start.is_some())
            && basis <= strict_date(published).unwrap()
            && matches!(
                self.data_kind.as_str(),
                "actual" | "forecast" | "derived" | "estimated"
            )
            && if self.metric_key == "forecast_dpu" {
                self.data_kind == "forecast"
            } else {
                self.data_kind != "forecast" && end <= strict_date(published).unwrap()
            }
    }
}

fn stmt(sql: &str, params: Vec<Val>) -> Stmt {
    Stmt {
        sql: sql.into(),
        params,
    }
}
fn t(s: impl Into<String>) -> Val {
    Val::Text(s.into())
}
fn serialization(e: serde_json::Error) -> PortError {
    PortError(e.to_string())
}

fn observation_stmt(rows: &[Observation]) -> Result<Stmt, PortError> {
    Ok(stmt(
        "INSERT INTO reit_metric_observations (ticker,metric_key,metric_value,unit,period_start,period_end,\
         fiscal_period_end,as_of_date,source_url,source_document_id,source_page,disclosure_published_at,\
         retrieved_at,data_kind,method_version,normalization_flags,quality_flags,revision_id) \
         SELECT value->>'ticker',value->>'metric_key',value->>'metric_value',value->>'unit',\
         value->>'period_start',value->>'period_end',value->>'fiscal_period_end',value->>'as_of_date',\
         value->>'source_url',value->>'source_document_id',value->>'source_page',value->>'disclosure_published_at',\
         value->>'retrieved_at',value->>'data_kind',value->>'method_version',value->>'normalization_flags',\
         value->>'quality_flags',value->>'revision_id' FROM json_each(?) WHERE true \
         ON CONFLICT(ticker,metric_key,fiscal_period_end,revision_id) DO UPDATE SET \
         metric_value=excluded.metric_value,unit=excluded.unit,period_start=excluded.period_start,\
         as_of_date=excluded.as_of_date,source_url=excluded.source_url,source_document_id=excluded.source_document_id,\
         source_page=excluded.source_page,disclosure_published_at=excluded.disclosure_published_at,\
         data_kind=excluded.data_kind,method_version=excluded.method_version,\
         normalization_flags=excluded.normalization_flags,quality_flags=excluded.quality_flags",
        vec![t(serde_json::to_string(rows).map_err(serialization)?)],
    ))
}

pub async fn import(store: &D1Store, rows: &[Observation], day: Date) -> Result<(), PortError> {
    let listed = inputs::listed_reits(day).await?;
    for (i, row) in rows.iter().enumerate() {
        if !row.validate() || !listed.contains_key(&row.ticker) {
            return Err(PortError(format!(
                "invalid J-REIT observation at record {}",
                i + 1
            )));
        }
    }
    save_observations(store, rows).await
}

pub async fn save_observations(store: &D1Store, rows: &[Observation]) -> Result<(), PortError> {
    for chunk in rows.chunks(100) {
        store.exec(&observation_stmt(chunk)?).await?;
    }
    Ok(())
}

pub async fn save_disclosure_issues(
    store: &D1Store,
    events: &[Value],
    day: Date,
    now: i64,
) -> Result<(), PortError> {
    for event in events.iter().filter(|event| {
        event["docTypeCode"] == "130"
            || ["withdrawalStatus", "disclosureStatus", "docInfoEditStatus"]
                .iter()
                .any(|&key| event[key].as_str().is_some_and(|s| s != "0"))
    }) {
        let Some(id) = event["docID"].as_str() else {
            continue;
        };
        let body = event.to_string();
        store.exec(&stmt(
            "INSERT OR IGNORE INTO reit_rag_document_events(id,doc_id,parent_doc_id,observed_at,listed_on,body) VALUES(?,?,?,?,?,?)",
            vec![t(hash(body.as_bytes())),t(id),event["parentDocID"].as_str().map_or(Val::Null,t),Val::Int(now),t(day.to_string()),t(body)],
        )).await?;
    }
    Ok(())
}

struct Issuer {
    ticker: String,
    name: String,
    sector: String,
    values: BTreeMap<&'static str, inputs::ValueWithSource>,
    provisional: Vec<&'static str>,
    history: Vec<Value>,
    forecast: Value,
}

struct SnapshotRow {
    summary: Value,
    detail: Value,
}

fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn profile_ranks(rows: &mut [SnapshotRow]) {
    for profile in [
        "total",
        "distribution",
        "profitability",
        "finance",
        "nav",
        "valuation",
    ] {
        let mut order: Vec<_> = rows
            .iter()
            .enumerate()
            .filter_map(|(i, row)| {
                let s = &row.summary;
                (s["rank_eligible"] == true).then_some(())?;
                Some((
                    i,
                    s["profiles"][profile].as_f64()?,
                    s["coverage_pct"].as_u64()? as u32,
                    s["ticker"].as_str()?,
                ))
            })
            .collect();
        order.sort_by(|a, b| rules::rank_cmp((a.1, a.2, a.3), (b.1, b.2, b.3)));
        let indexes: Vec<_> = order.into_iter().map(|(i, ..)| i).collect();
        let mut by_sector = BTreeMap::<String, usize>::new();
        for (i, idx) in indexes.into_iter().enumerate() {
            let summary = &mut rows[idx].summary;
            summary["ranks"][profile] = json!(i + 1);
            let sector = summary["sector"].as_str().unwrap_or("unknown");
            if sector != "unknown" {
                let rank = by_sector.entry(sector.to_owned()).or_default();
                *rank += 1;
                summary["sector_ranks"][profile] = json!(*rank);
            }
        }
    }
}

fn snapshot(issuers: Vec<Issuer>) -> Vec<SnapshotRow> {
    let inputs: Vec<_> = issuers
        .iter()
        .map(|i| {
            i.values
                .iter()
                .filter(|(_, v)| v.valid)
                .map(|(k, v)| (*k, v.value))
                .collect()
        })
        .collect();
    let mut rows: Vec<_> = issuers.into_iter().zip(rules::score(&inputs)).map(|(i, score)| {
        let lookup = |k: &str| i.values.get(k).filter(|v| v.valid).map(|v| v.value);
        let mut warnings = Vec::new();
        for (key, condition, message) in [
            ("ltv", lookup("ltv").is_some_and(|v| v > 0.55), "LTVが55%超"),
            ("floating_ratio", lookup("floating_ratio").is_some_and(|v| v > 0.50), "実質変動金利比率が50%超"),
            ("interest_cover", lookup("interest_cover").is_some_and(|v| v < 2.0), "利払い余力が2倍未満"),
            ("stress_100", lookup("stress_100").is_some_and(|v| v > 0.10), "金利+1.0%ptの負担が正常化分配額の10%超"),
        ] {
            if condition {
                warnings.push(json!({"metric_key":key,"message":message}));
            }
        }
        let mut reasons = i.provisional;
        if score.coverage < rules::MIN_COVERAGE {
            reasons.push("low_coverage");
        }
        let mut profiles = BTreeMap::new();
        profiles.insert("total", score.total);
        let categories: Vec<_> = CATEGORIES.iter().zip(&score.categories).map(|(&(key, label), c)| {
            profiles.insert(key, c.rate());
            json!({"key":key,"label":label,"points":c.points,"available":c.available,"maximum":c.maximum})
        }).collect();
        let components: Vec<_> = score.components.iter().map(|c| {
            let m = c.metric;
            let v = i.values.get(m.key);
            json!({"metric_key":m.key,"label":m.label,"formula":m.formula,"unit":m.unit,"weight":m.weight,
                "metric_value":v.map(|v|v.value),"percentile":c.percentile,"weighted_points":c.percentile.map(|p|p*f64::from(m.weight)/100.0),
                "status":c.status.key(),"benchmark_size":c.benchmark_size,
                "source_ref":v.map(|v|&v.sources)})
        }).collect();
        let mut drivers: Vec<_> = score.components.iter().filter_map(|c| {
            Some((c.metric, c.percentile?))
        }).collect();
        drivers.sort_by(|(a, ap), (b, bp)| {
            let impact = |m: &rules::Metric, p: f64| (p - 50.0) * f64::from(m.weight);
            impact(b, *bp).total_cmp(&impact(a, *ap)).then(a.key.cmp(b.key))
        });
        let explain = |m: &rules::Metric, p: f64| json!({"metric_key":m.key,"label":m.label,"percentile":p});
        let positives: Vec<_> = drivers.iter().filter(|(_,p)| *p > 50.0).take(3).map(|(m,p)|explain(m,*p)).collect();
        let weaknesses: Vec<_> = drivers.iter().rev().filter(|(_,p)| *p < 50.0).take(3).map(|(m,p)|explain(m,*p)).collect();
        let summary = json!({"ticker":i.ticker,"name":i.name,"sector":i.sector,"total_score":score.total,
            "coverage_pct":score.coverage,"rank_eligible":reasons.is_empty(),"provisional_reasons":reasons,
            "price_date":i.values.get("price").and_then(|v|v.sources.first()).map(|s| &s.period_end),
            "actual_yield":lookup("actual_yield"),"forecast_yield":lookup("forecast_yield"),
            "forecast":i.forecast,"ltv":lookup("ltv"),"floating_ratio":lookup("floating_ratio"),
            "risks":warnings,"positives":positives,"weaknesses":weaknesses,
            "profiles":profiles,"ranks":{},"sector_ranks":{}});
        let detail = json!({"categories":categories,"components":components,"values":i.values,"history":i.history});
        SnapshotRow { summary, detail }
    }).collect();
    profile_ranks(&mut rows);
    rows
}

pub async fn compute(store: &D1Store, day: Date, now: i64) -> Result<(), PortError> {
    let started = std::time::Instant::now();
    let issuers = inputs::load(store, day).await?;
    if issuers.is_empty() {
        return Err(PortError("no REITs in the JPX stock master".into()));
    }
    let rows = snapshot(issuers);
    let payload: Vec<_> = rows.iter().map(|r| (&r.summary, &r.detail)).collect();
    // Hash the exact frozen evidence (including excluded observations), not the clock.
    let watermark = hash(&serde_json::to_vec(&payload).map_err(serialization)?);
    let id = format!("{VERSION}-{day}-{watermark}");
    store.exec(&stmt(
        "INSERT INTO reit_scoring_runs VALUES (?,?,?,?,?,NULL,'staging',?) ON CONFLICT(run_id) DO NOTHING",
        vec![t(&id),t(day.to_string()),t(VERSION),Val::Int(rows.len() as i64),t(UNIVERSE),t(&watermark)]
    )).await?;
    for row in &rows {
        store
            .exec(&stmt(
                include_str!("reit_scoring/insert_score.sql"),
                vec![
                    t(&id),
                    t(row.summary.to_string()),
                    t(row.detail.to_string()),
                ],
            ))
            .await?;
        store
            .exec(&stmt(
                include_str!("reit_scoring/insert_components.sql"),
                vec![
                    t(&id),
                    t(row.summary["ticker"].as_str().unwrap_or_default()),
                ],
            ))
            .await?;
    }
    // Only this guarded single statement makes a run visible. A failed or interrupted
    // run never changes indicators_latest or any previous published snapshot.
    store
        .exec(&stmt(
            include_str!("reit_scoring/publish.sql"),
            vec![
                Val::Int(now + started.elapsed().as_secs() as i64),
                t(&id),
                Val::Int(METRICS.len() as i64),
            ],
        ))
        .await?;
    let published = store
        .query(&stmt(
            "SELECT status FROM reit_scoring_runs WHERE run_id=?",
            vec![t(&id)],
        ))
        .await?;
    if published.first().is_none_or(|r| r["status"] != "ready") {
        return Err(PortError("J-REIT score snapshot incomplete".into()));
    }
    let eligible = rows
        .iter()
        .filter(|r| r.summary["rank_eligible"] == true)
        .count();
    let mut coverage: Vec<_> = rows
        .iter()
        .filter_map(|row| row.summary["coverage_pct"].as_u64())
        .collect();
    coverage.sort_unstable();
    println!(
        "reit-scoring: {id} universe={} eligible={eligible} coverage_min_median_max={}/{}/{} tie_break={TIE_BREAK}",
        rows.len(),coverage[0],coverage[coverage.len()/2],coverage[coverage.len()-1]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisional_issuer_can_benchmark_but_never_receives_a_rank() {
        let issuers: Vec<_> = (0..8)
            .map(|i| {
                let ticker = format!("89{i:02}");
                let mut provisional = vec![];
                if i == 0 {
                    provisional.push("missing_or_stale_price");
                }
                let values = METRICS
                    .iter()
                    .filter(|m| m.enabled)
                    .map(|m| {
                        (
                            m.key,
                            inputs::ValueWithSource {
                                value: 1.0,
                                valid: true,
                                sources: vec![],
                            },
                        )
                    })
                    .collect();
                Issuer {
                    ticker,
                    name: String::new(),
                    sector: if i <= 2 { "office" } else { "retail" }.into(),
                    values,
                    provisional,
                    history: vec![],
                    forecast: Value::Null,
                }
            })
            .collect();
        let rows = snapshot(issuers);
        assert_eq!(rows[0].summary["coverage_pct"], 92);
        assert_eq!(rows[0].summary["rank_eligible"], false);
        assert!(rows[0].summary["ranks"]["total"].is_null());
        assert_eq!(rows[1].summary["ranks"]["total"], 1);
        assert_eq!(rows[2].summary["sector_ranks"]["total"], 2);
        assert_eq!(rows[3].summary["ranks"]["total"], 3);
        assert_eq!(rows[3].summary["sector_ranks"]["total"], 1);
        assert_eq!(rows[3].summary["ranks"]["distribution"], 3);
    }
}
