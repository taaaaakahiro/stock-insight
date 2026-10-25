//! recommendations の読み書きと、AIレコメンドの候補の読み取り（RecommendRepository の実装）。

use crate::d1::Row;
use crate::query;
use crate::stock_json::row_json;
use domain::recommend::{Grade, Pick};
use serde::Deserialize;
use serde_json::{json, Value};
use usecase::recommend::{
    Attempt, RecommendRepository, RecommendationRow, Recommendations, RULE_VERSION,
};
use usecase::screening::{DataStatus, StockRow};
use usecase::PortError;
use worker::wasm_bindgen::JsValue;
use worker::D1Database;

pub struct D1Recommendations {
    db: D1Database,
}

impl D1Recommendations {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }
}

fn err(e: worker::Error) -> PortError {
    PortError(format!("d1: {e}"))
}

#[derive(Deserialize)]
struct RunRow {
    run_id: String,
    started_at: i64,
    finished_at: Option<i64>,
    status: String,
    latest_price_date: Option<String>,
    latest_fin_date: Option<String>,
}

/// レコメンド側の列。指標の列は同じ結果を Row としても読む
#[derive(Deserialize)]
struct RecoRow {
    date: String,
    rank: i64,
    grade: Option<String>,
    reason: String,
    model: String,
    created_at: i64,
    source: Option<String>,
}

impl RecommendRepository for D1Recommendations {
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError> {
        let row: Option<RunRow> = self
            .db
            .prepare(query::LATEST_RUN)
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.map(|r| DataStatus {
            run_id: r.run_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            status: r.status,
            latest_price_date: r.latest_price_date,
            latest_fin_date: r.latest_fin_date,
        }))
    }

    async fn candidates(&self) -> Result<Vec<StockRow>, PortError> {
        let rows: Vec<Row> = self
            .db
            .prepare(query::recommend_candidates())
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows.into_iter().map(StockRow::from).collect())
    }

    /// 配当方針の文。1銘柄に有報と半期報告書の両方があれば両方（新しい順）。同じ文は1つにする
    async fn policy_phrases(&self, codes: &[String]) -> Result<Vec<(String, String)>, PortError> {
        if codes.is_empty() {
            return Ok(vec![]);
        }
        #[derive(Deserialize)]
        struct PhraseRow {
            code: String,
            doe_phrase: Option<String>,
            payout_phrase: Option<String>,
        }
        let args: Vec<JsValue> = codes.iter().map(|c| JsValue::from_str(c)).collect();
        let rows: Vec<PhraseRow> = self
            .db
            .prepare(query::policy_phrases(codes.len()))
            .bind(&args)
            .map_err(err)?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        let mut out: Vec<(String, String)> = vec![];
        for r in rows {
            for p in [r.doe_phrase, r.payout_phrase].into_iter().flatten() {
                let item = (r.code.clone(), p);
                if !out.contains(&item) {
                    out.push(item);
                }
            }
        }
        Ok(out)
    }

    async fn themes(&self) -> Result<Vec<(String, String)>, PortError> {
        #[derive(Deserialize)]
        struct ThemeRow {
            code: String,
            name: String,
        }
        let rows: Vec<ThemeRow> = self
            .db
            .prepare(query::STOCK_THEME_NAMES)
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows.into_iter().map(|r| (r.code, r.name)).collect())
    }

    async fn exists(&self, date: &str) -> Result<bool, PortError> {
        let n: Option<f64> = self
            .db
            .prepare(query::RECOMMENDATION_EXISTS)
            .bind(&[JsValue::from_str(date)])
            .map_err(err)?
            .first(Some("n"))
            .await
            .map_err(err)?;
        Ok(n.unwrap_or(0.0) > 0.0)
    }

    async fn begin(&self, a: &Attempt<'_>) -> Result<i64, PortError> {
        let snapshots: Vec<_> = a
            .candidates
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut stock = row_json(r);
                stock["themes"] = json!(a
                    .themes
                    .iter()
                    .filter(|(code, _)| code == &r.code)
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>());
                json!({"code":r.code,"rule_rank":i+1,"stock":stock})
            })
            .collect();
        // run と候補の保存は同じトランザクション。AI はこの完了後にだけ呼ぶ。
        let insert_run = self.db.prepare(
            "INSERT INTO recommendation_runs (price_date,batch_id,model,prompt_version,rule_version,app_version,request_json,status,trigger_mode,created_at) \
             VALUES (?,?,?,?,?,?,?,'pending',?,?) RETURNING id"
        ).bind(&[
            JsValue::from_str(a.date),JsValue::from_str(a.batch_id),JsValue::from_str(a.model),
            JsValue::from_str(a.prompt_version),JsValue::from_str(RULE_VERSION),
            JsValue::from_str(env!("CARGO_PKG_VERSION")),JsValue::from_str(a.request),
            JsValue::from_str(if a.manual {"manual"} else {"scheduled"}),JsValue::from_f64(a.now as f64),
        ]).map_err(err)?;
        // batch内に別の書き込みは割り込まないため、MAX(id)は直前に挿入したrun。
        let insert_candidates = self.db.prepare(
            "INSERT INTO recommendation_candidates (run_id,code,rule_rank,snapshot_json) \
             SELECT (SELECT MAX(id) FROM recommendation_runs),json_extract(value,'$.code'), \
                    json_extract(value,'$.rule_rank'),json_set(json_extract(value,'$.stock'), \
                    '$.financial_doc_id',(SELECT fin_doc_id FROM indicators_latest WHERE code=json_extract(value,'$.code')), \
                    '$.financial_disclosed',(SELECT fin_submitted FROM indicators_latest WHERE code=json_extract(value,'$.code'))) FROM json_each(?)"
        ).bind(&[JsValue::from_str(&json!(snapshots).to_string())]).map_err(err)?;
        let result = self
            .db
            .batch(vec![insert_run, insert_candidates])
            .await
            .map_err(err)?;
        #[derive(Deserialize)]
        struct Id {
            id: i64,
        }
        let ids: Vec<Id> = result[0].results().map_err(err)?;
        ids.first()
            .map(|r| r.id)
            .ok_or_else(|| PortError("missing recommendation id".into()))
    }

    async fn publish(&self, id: i64, response: &str, picks: &[Pick]) -> Result<(), PortError> {
        let now = (worker::Date::now().as_millis() / 1000) as i64;
        let mut statements = Vec::new();
        for p in picks {
            statements.push(self.db.prepare(
                "UPDATE recommendation_candidates SET rank=?,reason=?,grade=? WHERE run_id=? AND code=? \
                 AND EXISTS (SELECT 1 FROM recommendation_runs WHERE id=? AND status='pending')"
            ).bind(&[JsValue::from_f64(p.rank as f64), JsValue::from_str(&p.reason),
                JsValue::from_str(p.grade.as_str()), JsValue::from_f64(id as f64), JsValue::from_str(&p.code), JsValue::from_f64(id as f64)])
                .map_err(err)?);
        }
        statements.push(self.db.prepare(
            "UPDATE recommendation_runs SET status='published',response_json=?,published_at=?, \
             is_primary=CASE WHEN EXISTS (SELECT 1 FROM recommendation_runs old \
               WHERE old.price_date=recommendation_runs.price_date AND old.is_primary=1) THEN 0 ELSE 1 END \
             WHERE id=? AND status='pending'"
        ).bind(&[JsValue::from_str(response), JsValue::from_f64(now as f64), JsValue::from_f64(id as f64)]).map_err(err)?);
        // 最新表示の互換キャッシュだけを更新する。履歴には DELETE を行わない。
        statements.push(self.db.prepare(
            "DELETE FROM recommendations WHERE date=(SELECT price_date FROM recommendation_runs WHERE id=? AND status='published')"
        ).bind(&[JsValue::from_f64(id as f64)]).map_err(err)?);
        statements.push(
            self.db
                .prepare(
                    "INSERT INTO recommendations(date,rank,code,reason,model,created_at,grade,source) \
             SELECT r.price_date,c.rank,c.code,c.reason,r.model,r.published_at,c.grade,CASE WHEN r.trigger_mode='manual' THEN 'manual' ELSE 'cron' END \
             FROM recommendation_runs r JOIN recommendation_candidates c ON c.run_id=r.id \
             WHERE r.id=? AND r.status='published' AND c.rank IS NOT NULL",
                )
                .bind(&[JsValue::from_f64(id as f64)])
                .map_err(err)?,
        );
        self.db.batch(statements).await.map_err(err)?;
        Ok(())
    }

    async fn fail(&self, id: i64, response: Option<&str>, error: &str) -> Result<(), PortError> {
        self.db.prepare("UPDATE recommendation_runs SET status='failed',response_json=?,error=? WHERE id=? AND status='pending'")
            .bind(&[response.map_or(JsValue::NULL, JsValue::from_str),
                JsValue::from_str(&error.chars().take(2000).collect::<String>()), JsValue::from_f64(id as f64)])
            .map_err(err)?.run().await.map_err(err)?;
        Ok(())
    }

    async fn latest(&self) -> Result<Option<Recommendations>, PortError> {
        let result = self
            .db
            .prepare(query::recommendation_rows())
            .all()
            .await
            .map_err(err)?;
        let meta: Vec<RecoRow> = result.results().map_err(err)?;
        let stocks: Vec<Row> = result.results().map_err(err)?;
        let Some(first) = meta.first() else {
            return Ok(None);
        };
        let themes = self.themes().await?;
        let day = Recommendations {
            date: first.date.clone(),
            model: first.model.clone(),
            created_at: first.created_at,
            manual: first.source.as_deref() == Some("manual"),
            rows: meta
                .iter()
                .zip(stocks)
                .map(|(m, s)| {
                    let stock = StockRow::from(s);
                    RecommendationRow {
                        rank: m.rank as u32,
                        grade: m.grade.as_deref().and_then(Grade::parse),
                        reason: m.reason.clone(),
                        themes: themes
                            .iter()
                            .filter(|(code, _)| *code == stock.code)
                            .map(|(_, name)| name.clone())
                            .collect(),
                        stock,
                    }
                })
                .collect(),
        };
        Ok(Some(day))
    }
}

impl D1Recommendations {
    pub async fn comparison(&self) -> Result<Value, PortError> {
        let batch:Option<Value> = self.db.prepare(
            "SELECT created_at,as_of,version,report_json FROM evaluation_batches WHERE status='complete' AND report_json IS NOT NULL ORDER BY id DESC LIMIT 1"
        ).first(None).await.map_err(err)?;
        let Some(batch) = batch else {
            return Ok(json!({"report":null}));
        };
        let report: Value = serde_json::from_str(batch["report_json"].as_str().unwrap_or("null"))
            .map_err(|e| PortError(e.to_string()))?;
        Ok(
            json!({"report":report,"as_of":batch["as_of"],"evaluated_at":batch["created_at"],"version":batch["version"]}),
        )
    }

    /// キーセットページング。本文・入力全文は一覧には含めない。
    pub async fn history(&self, before: i64) -> Result<Value, PortError> {
        let mut rows: Vec<Value> = self.db.prepare(
            "SELECT id,price_date,model,prompt_version,rule_version,status,is_primary,trigger_mode,created_at,published_at \
             FROM recommendation_runs WHERE id < ? AND status IN ('published','legacy_incomplete') ORDER BY id DESC LIMIT 21"
        ).bind(&[JsValue::from_f64(before as f64)]).map_err(err)?.all().await.map_err(err)?.results().map_err(err)?;
        let more = rows.len() > 20;
        rows.truncate(20);
        let next = if more {
            rows.last().and_then(|r| r["id"].as_i64())
        } else {
            None
        };
        Ok(json!({"runs": rows, "next_before": next}))
    }

    pub async fn history_detail(&self, id: i64) -> Result<Option<Value>, PortError> {
        let run: Option<Value> = self.db.prepare(
            "SELECT id,price_date,model,prompt_version,rule_version,status,is_primary,trigger_mode,created_at,published_at \
             FROM recommendation_runs WHERE id=? AND status IN ('published','legacy_incomplete')"
        ).bind(&[JsValue::from_f64(id as f64)]).map_err(err)?.first(None).await.map_err(err)?;
        let Some(run) = run else {
            return Ok(None);
        };
        let rows: Vec<Value> = self.db.prepare(
            "SELECT code,rule_rank,rank,reason,grade,snapshot_json FROM recommendation_candidates WHERE run_id=? ORDER BY COALESCE(rank,1000),rule_rank,code"
        ).bind(&[JsValue::from_f64(id as f64)]).map_err(err)?.all().await.map_err(err)?.results().map_err(err)?;
        // 未完了の評価バッチは見せない。銘柄・期間ごとの最新の完了版だけを返す。
        let outcomes: Vec<Value> = self.db.prepare(
            "SELECT o.code,o.horizon,o.result_json,b.created_at AS evaluated_at FROM recommendation_outcomes o \
             JOIN evaluation_batches b ON b.id=o.batch_id AND b.status='complete' \
             WHERE o.run_id=? AND o.batch_id=(SELECT MAX(x.batch_id) FROM recommendation_outcomes x \
              JOIN evaluation_batches eb ON eb.id=x.batch_id AND eb.status='complete' \
              WHERE x.run_id=o.run_id AND x.code=o.code AND x.horizon=o.horizon)"
        ).bind(&[JsValue::from_f64(id as f64)]).map_err(err)?.all().await.map_err(err)?.results().map_err(err)?;
        let mut candidates = Vec::new();
        for mut r in rows {
            let snapshot = r.as_object_mut().and_then(|o| o.remove("snapshot_json"));
            r["stock"] = snapshot
                .and_then(|s| s.as_str().map(str::to_owned))
                .map(|s| serde_json::from_str::<Value>(&s))
                .transpose()
                .map_err(|e| PortError(e.to_string()))?
                .unwrap_or(Value::Null);
            candidates.push(r);
        }
        let mut evaluated = Vec::new();
        for mut r in outcomes {
            let result: Value = serde_json::from_str(r["result_json"].as_str().unwrap_or("null"))
                .map_err(|e| PortError(e.to_string()))?;
            r.as_object_mut().unwrap().remove("result_json");
            r["result"] = result;
            evaluated.push(r);
        }
        Ok(Some(
            json!({"run":run,"candidates":candidates,"outcomes":evaluated}),
        ))
    }
}
