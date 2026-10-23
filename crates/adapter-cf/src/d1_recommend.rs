//! recommendations の読み書きと、AIレコメンドの候補の読み取り（RecommendRepository の実装）。

use crate::d1::Row;
use crate::query;
use domain::recommend::{Grade, Pick};
use serde::Deserialize;
use usecase::recommend::{RecommendRepository, RecommendationRow, Recommendations};
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

    /// 1日分を1回の batch で書く（D1 への要求と書き込み行数を最小にする）
    async fn save(
        &self,
        date: &str,
        model: &str,
        picks: &[Pick],
        now: i64,
    ) -> Result<(), PortError> {
        let stmts = picks
            .iter()
            .map(|p| {
                self.db
                    .prepare(query::RECOMMENDATION_INSERT)
                    .bind(&[
                        JsValue::from_str(date),
                        JsValue::from_f64(p.rank as f64),
                        JsValue::from_str(&p.code),
                        JsValue::from_str(&p.reason),
                        JsValue::from_str(model),
                        JsValue::from_f64(now as f64),
                        JsValue::from_str(p.grade.as_str()),
                    ])
                    .map_err(err)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.db.batch(stmts).await.map_err(err)?;
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
        let day = Recommendations {
            date: first.date.clone(),
            model: first.model.clone(),
            created_at: first.created_at,
            rows: meta
                .iter()
                .zip(stocks)
                .map(|(m, s)| RecommendationRow {
                    rank: m.rank as u32,
                    grade: m.grade.as_deref().and_then(Grade::parse),
                    reason: m.reason.clone(),
                    stock: StockRow::from(s),
                })
                .collect(),
        };
        Ok(Some(day))
    }
}
