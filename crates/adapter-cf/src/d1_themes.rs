//! themes / stock_themes の読み書き（ThemeRepository の実装）。照合は D1 の instr で行う

use crate::query;
use domain::theme::ThemeInput;
use serde::Deserialize;
use usecase::themes::{Theme, ThemeRepository, ThemeStock};
use usecase::PortError;
use worker::wasm_bindgen::JsValue;
use worker::{D1Database, D1PreparedStatement};

pub struct D1Themes {
    db: D1Database,
}

impl D1Themes {
    pub fn new(db: D1Database) -> Self {
        Self { db }
    }

    fn prepare(&self, sql: &str, args: &[JsValue]) -> Result<D1PreparedStatement, PortError> {
        self.db.prepare(sql).bind(args).map_err(err)
    }
}

fn err(e: worker::Error) -> PortError {
    PortError(format!("d1: {e}"))
}

fn num(i: i64) -> JsValue {
    JsValue::from_f64(i as f64)
}

/// themes の行。words は JSON の文字列配列
#[derive(Deserialize)]
struct ThemeRow {
    id: i64,
    name: String,
    words: String,
    count: i64,
    updated_at: i64,
}

impl From<ThemeRow> for Theme {
    fn from(r: ThemeRow) -> Self {
        Theme {
            id: r.id,
            name: r.name,
            words: serde_json::from_str(&r.words).unwrap_or_default(),
            count: r.count,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Deserialize)]
struct StockThemeRow {
    code: String,
    name: Option<String>,
    matched_word: String,
}

impl ThemeRepository for D1Themes {
    async fn list(&self) -> Result<Vec<Theme>, PortError> {
        let rows: Vec<ThemeRow> = self
            .db
            .prepare(query::THEMES)
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows.into_iter().map(Theme::from).collect())
    }

    async fn get(&self, id: i64) -> Result<Option<Theme>, PortError> {
        let row: Option<ThemeRow> = self
            .prepare(query::THEME, &[num(id)])?
            .first(None)
            .await
            .map_err(err)?;
        Ok(row.map(Theme::from))
    }

    async fn insert(&self, input: &ThemeInput, now: i64) -> Result<i64, PortError> {
        let words = serde_json::to_string(&input.words).unwrap_or_else(|_| "[]".into());
        let id: Option<f64> = self
            .prepare(
                query::THEME_INSERT,
                &[
                    JsValue::from_str(&input.name),
                    JsValue::from_str(&words),
                    num(now),
                    num(now),
                ],
            )?
            .first(Some("id"))
            .await
            .map_err(err)?;
        id.map(|v| v as i64)
            .ok_or_else(|| PortError("d1: insert returned no id".into()))
    }

    async fn update(&self, id: i64, input: &ThemeInput, now: i64) -> Result<bool, PortError> {
        let words = serde_json::to_string(&input.words).unwrap_or_else(|_| "[]".into());
        let result = self
            .prepare(
                query::THEME_UPDATE,
                &[
                    JsValue::from_str(&input.name),
                    JsValue::from_str(&words),
                    num(now),
                    num(id),
                ],
            )?
            .run()
            .await
            .map_err(err)?;
        Ok(result
            .meta()
            .ok()
            .flatten()
            .and_then(|m| m.changes)
            .unwrap_or(0)
            > 0)
    }

    /// テーマと、その照合結果を1回の batch で消す
    async fn delete(&self, id: i64) -> Result<(), PortError> {
        self.db
            .batch(vec![
                self.prepare(query::STOCK_THEMES_DELETE, &[num(id)])?,
                self.prepare(query::THEME_DELETE, &[num(id)])?,
            ])
            .await
            .map_err(err)?;
        Ok(())
    }

    /// 消して入れ直すまでを1回の batch で（途中で切れて空にならないように）。言葉ごとに1文
    async fn rematch(&self, id: i64, words: &[String]) -> Result<(), PortError> {
        let mut stmts = vec![self.prepare(query::STOCK_THEMES_DELETE, &[num(id)])?];
        for w in words {
            stmts.push(self.prepare(
                query::STOCK_THEMES_MATCH,
                &[num(id), JsValue::from_str(w), JsValue::from_str(w)],
            )?);
        }
        self.db.batch(stmts).await.map_err(err)?;
        Ok(())
    }

    async fn stocks(&self, id: i64) -> Result<Vec<ThemeStock>, PortError> {
        let rows: Vec<StockThemeRow> = self
            .prepare(query::THEME_STOCKS, &[num(id)])?
            .all()
            .await
            .map_err(err)?
            .results()
            .map_err(err)?;
        Ok(rows
            .into_iter()
            .map(|r| ThemeStock {
                code: r.code,
                name: r.name,
                matched_word: r.matched_word,
            })
            .collect())
    }
}
