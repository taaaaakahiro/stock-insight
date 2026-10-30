//! RecommendRepository の D1 HTTP API 版（Actions のバッチから使う）。
//! Worker 側（adapter-cf::d1_recommend、wasm 専用）と同じクエリと保存手順を、
//! D1Store（REST）で組み直したもの。REST は複数文のトランザクションがないため、
//! Worker 側が batch() でまとめていた文は順に実行する（begin/publish とも
//! 途中で止まった分は status='pending' のまま残り、次回の生成で作り直される）。

use crate::d1::D1Store;
use crate::sql::{Stmt, Val};
use adapter_cf::query;
use adapter_cf::stock_json::row_json;
use domain::recommend::{Grade, Pick};
use serde::Deserialize;
use serde_json::{json, Value};
use usecase::recommend::{
    Attempt, RecommendRepository, RecommendationRow, Recommendations, RULE_VERSION,
};
use usecase::screening::{DataStatus, StockRow};
use usecase::PortError;

/// Worker 側の D1Recommendations と同じ責務を持つ Actions 向け実装。
pub struct D1Recommendations<'a> {
    store: &'a D1Store,
}

impl<'a> D1Recommendations<'a> {
    pub fn new(store: &'a D1Store) -> Self {
        Self { store }
    }
}

fn err(e: serde_json::Error) -> PortError {
    PortError(format!("d1: {e}"))
}

fn int(v: i64) -> Val {
    Val::Int(v)
}

fn text(s: &str) -> Val {
    Val::Text(s.to_string())
}

fn stmt(sql: impl Into<String>) -> Stmt {
    Stmt {
        sql: sql.into(),
        params: vec![],
    }
}

/// indicators_latest × stocks の1行。recommend_candidates が返す列だけ。
/// adapter-cf::d1 の Row と同じ対応（worker 側の型は wasm 専用なのでここで組み直す。
/// SQLite の真偽値は 0/1 の整数）。
#[derive(Deserialize)]
pub struct Row {
    financial_quality_json: Option<String>,
    code: String,
    price: f64,
    price_date: String,
    dividend_ttm: f64,
    dividend_basis: Option<String>,
    dividend_yield: Option<f64>,
    dividend_total: Option<f64>,
    dividend_special: Option<f64>,
    dividend_commemorative: Option<f64>,
    dividend_source: Option<String>,
    yield_percentile: Option<f64>,
    pbr: Option<f64>,
    per: Option<f64>,
    payout_ratio: Option<f64>,
    doe: Option<f64>,
    consecutive_increases: Option<i64>,
    operating_margin: Option<f64>,
    no_loss_years: Option<i64>,
    profit_uptrend: Option<i64>,
    valuation: String,
    fetched_at: i64,
    name: Option<String>,
    kind: String,
    market: Option<String>,
    sector33: Option<String>,
    size_class: Option<String>,
    dividend_every_year: Option<i64>,
    no_quarterly_loss: Option<i64>,
    is_value: Option<i64>,
    data_years: Option<i64>,
    sales_growth: Option<f64>,
    op_growth: Option<f64>,
    dividend_cut: Option<i64>,
    progressive_dividend: Option<i64>,
    no_dividend_cut: Option<i64>,
    doe_floor: Option<f64>,
    doe_goal: Option<f64>,
    payout_floor: Option<f64>,
    payout_goal: Option<f64>,
    op_growth_q: Option<f64>,
    net_growth_q: Option<f64>,
    op_growth_ttm: Option<f64>,
    profit_one_off: Option<i64>,
}

/// adapter-cf::d1 の From<Row> と同じ変換（reit 列は候補クエリにないので None）。
pub fn to_stock_row(r: Row) -> StockRow {
    let forecast = r.dividend_basis.as_deref() == Some("forecast");
    let doe_forecast = forecast
        .then(|| {
            domain::indicators::doe_from_yield(
                r.dividend_total
                    .filter(|_| r.price > 0.0)
                    .map(|d| d / r.price),
                r.pbr,
            )
        })
        .flatten();
    let payout_forecast = forecast
        .then(|| {
            domain::indicators::payout_from_yield(
                r.dividend_total
                    .filter(|_| r.price > 0.0)
                    .map(|d| d / r.price),
                r.per,
            )
        })
        .flatten();
    StockRow {
        financial_quality: r
            .financial_quality_json
            .as_deref()
            .and_then(adapter_cf::recommend::parse_financial_quality),
        code: r.code,
        price: r.price,
        price_date: r.price_date,
        dividend: r.dividend_yield.map(|_| r.dividend_ttm),
        dividend_total: r.dividend_total,
        dividend_special: r.dividend_special,
        dividend_commemorative: r.dividend_commemorative,
        dividend_source: r.dividend_source,
        dividend_basis: r.dividend_basis,
        dividend_yield: r.dividend_yield,
        yield_percentile: r.yield_percentile,
        pbr: r.pbr,
        per: r.per,
        payout_ratio: r.payout_ratio,
        doe: r.doe,
        consecutive_increases: r.consecutive_increases,
        operating_margin: r.operating_margin,
        no_loss_years: r.no_loss_years.map(|v| v != 0),
        profit_uptrend: r.profit_uptrend.map(|v| v != 0),
        valuation: r.valuation,
        fetched_at: r.fetched_at,
        name: r.name,
        kind: r.kind,
        market: r.market,
        sector33: r.sector33,
        size_class: r.size_class,
        dividend_every_year: r.dividend_every_year.map(|v| v != 0),
        no_quarterly_loss: r.no_quarterly_loss.map(|v| v != 0),
        is_value: r.is_value.map(|v| v != 0),
        data_years: r.data_years,
        sales_growth: r.sales_growth,
        op_growth: r.op_growth,
        dividend_cut: r.dividend_cut.map(|v| v != 0),
        progressive_dividend: r.progressive_dividend.map(|v| v != 0),
        no_dividend_cut: r.no_dividend_cut.map(|v| v != 0),
        doe_floor: r.doe_floor,
        doe_goal: r.doe_goal,
        doe_forecast,
        payout_floor: r.payout_floor,
        payout_goal: r.payout_goal,
        payout_forecast,
        op_growth_q: r.op_growth_q,
        net_growth_q: r.net_growth_q,
        op_growth_ttm: r.op_growth_ttm,
        profit_one_off: r.profit_one_off.map(|v| v != 0),
        reit: None,
    }
}

fn decode_rows<T: for<'de> Deserialize<'de>>(values: Vec<Value>) -> Result<Vec<T>, PortError> {
    values
        .into_iter()
        .map(|v| serde_json::from_value(v).map_err(err))
        .collect()
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
    #[serde(default)]
    quality_json: Option<String>,
    date: String,
    rank: i64,
    grade: Option<String>,
    reason: String,
    model: String,
    created_at: i64,
    source: Option<String>,
}

/// publish の1回分の文列（trait 外に置くために inherent impl にする）。
/// 各文は冪等（候補の UPDATE は条件一致、run の UPDATE は status='pending' のときだけ、
/// DELETE と INSERT ... SELECT は同じ結果に収束）なので、途中失敗しても全体を
/// 再試行すれば復旧する
impl D1Recommendations<'_> {
    async fn publish_once(&self, id: i64, response: &str, picks: &[Pick]) -> Result<(), PortError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for p in picks {
            self.store
                .exec(&Stmt {
                    sql: "UPDATE recommendation_candidates SET rank=?,reason=?,grade=? WHERE run_id=? AND code=? \
                          AND EXISTS (SELECT 1 FROM recommendation_runs WHERE id=? AND status='pending')"
                        .to_string(),
                    params: vec![
                        int(p.rank as i64),
                        text(&p.reason),
                        text(p.grade.as_str()),
                        int(id),
                        text(&p.code),
                        int(id),
                    ],
                })
                .await?;
        }
        // scheduled 実行は同日に公開済みの run（手動の作り直しなど）があると公開しない。
        // 後続の DELETE/INSERT はこの run が published でないと何もしないため、
        // 同時実行された手動公開の内容を消さない。manual は従来どおり上書き可能
        self.store
            .exec(&Stmt {
                sql: "UPDATE recommendation_runs SET status='published',response_json=?,published_at=?, \
                      is_primary=CASE WHEN EXISTS (SELECT 1 FROM recommendation_runs old \
                        WHERE old.price_date=recommendation_runs.price_date AND old.is_primary=1) THEN 0 ELSE 1 END \
                      WHERE id=? AND status='pending' \
                      AND (trigger_mode='manual' OR NOT EXISTS (SELECT 1 FROM recommendation_runs pub \
                        WHERE pub.price_date=recommendation_runs.price_date AND pub.status='published'))"
                    .to_string(),
                params: vec![text(response), int(now), int(id)],
            })
            .await?;
        // 最新表示の互換キャッシュだけを更新する。履歴には DELETE を行わない。
        self.store
            .exec(&Stmt {
                sql: "DELETE FROM recommendations WHERE date=(SELECT price_date FROM recommendation_runs WHERE id=? AND status='published')"
                    .to_string(),
                params: vec![int(id)],
            })
            .await?;
        self.store
            .exec(&Stmt {
                sql: "INSERT INTO recommendations(date,rank,code,reason,model,created_at,grade,source) \
                      SELECT r.price_date,c.rank,c.code,c.reason,r.model,r.published_at,c.grade,CASE WHEN r.trigger_mode='manual' THEN 'manual' ELSE 'cron' END \
                      FROM recommendation_runs r JOIN recommendation_candidates c ON c.run_id=r.id \
                      WHERE r.id=? AND r.status='published' AND c.rank IS NOT NULL"
                    .to_string(),
                params: vec![int(id)],
            })
            .await?;
        Ok(())
    }
}

/// publish 全体の再試行。D1 の一時エラーは文ごとに D1Store 側でも再試行されるが、
/// 文と文の間の失敗（画面用行の削除後に挿入が失敗すると画面が空になる）は
/// 全体の再試行で復旧させる
const PUBLISH_RETRIES: u32 = 3;

impl RecommendRepository for D1Recommendations<'_> {
    async fn latest_run(&self) -> Result<Option<DataStatus>, PortError> {
        let rows = self.store.query(&stmt(query::LATEST_RUN)).await?;
        let mut rows = decode_rows::<RunRow>(rows)?;
        Ok(rows.pop().map(|r| DataStatus {
            run_id: r.run_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            status: r.status,
            latest_price_date: r.latest_price_date,
            latest_fin_date: r.latest_fin_date,
        }))
    }

    async fn candidates(&self) -> Result<Vec<StockRow>, PortError> {
        let rows = self
            .store
            .query(&stmt(query::recommend_candidates()))
            .await?;
        Ok(decode_rows::<Row>(rows)?
            .into_iter()
            .map(to_stock_row)
            .collect())
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
        let stmt = Stmt {
            sql: query::policy_phrases(codes.len()),
            params: codes.iter().map(|c| text(c)).collect(),
        };
        let out = decode_rows::<PhraseRow>(self.store.query(&stmt).await?)?
            .into_iter()
            .fold(vec![], |mut out, r| {
                for p in [r.doe_phrase, r.payout_phrase].into_iter().flatten() {
                    let item = (r.code.clone(), p);
                    if !out.contains(&item) {
                        out.push(item);
                    }
                }
                out
            });
        Ok(out)
    }

    async fn themes(&self) -> Result<Vec<(String, String)>, PortError> {
        #[derive(Deserialize)]
        struct ThemeRow {
            code: String,
            name: String,
        }
        let rows = self.store.query(&stmt(query::STOCK_THEME_NAMES)).await?;
        Ok(decode_rows::<ThemeRow>(rows)?
            .into_iter()
            .map(|r| (r.code, r.name))
            .collect())
    }

    async fn exists(&self, date: &str) -> Result<bool, PortError> {
        let stmt = Stmt {
            sql: query::RECOMMENDATION_EXISTS.to_string(),
            params: vec![text(date)],
        };
        let rows = self.store.query(&stmt).await?;
        Ok(rows.first().and_then(|r| r["n"].as_f64()).unwrap_or(0.0) > 0.0)
    }

    async fn begin(&self, a: &Attempt<'_>) -> Result<i64, PortError> {
        let snapshots: Vec<_> = a
            .candidates
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut stock = row_json(r);
                stock["recommendation_quality"] = adapter_cf::recommend::quality_json(r);
                stock["themes"] = json!(a
                    .themes
                    .iter()
                    .filter(|(code, _)| code == &r.code)
                    .map(|(_, name)| name)
                    .collect::<Vec<_>>());
                json!({"code":r.code,"rule_rank":i+1,"stock":stock})
            })
            .collect();
        // run と候補の保存は Worker 側では batch() の1トランザクション。REST には
        // 複数文のトランザクションがないため順に実行する。
        // run の INSERT は再実行すると別の id で重複するためリトライしない（応答喪失時は
        // 候補なしの pending 行が残るが、公開履歴には出ないので翌日の生成で埋まる）。
        let rows = self
            .store
            .query_no_retry(&Stmt {
                sql: "INSERT INTO recommendation_runs (price_date,batch_id,model,prompt_version,rule_version,app_version,request_json,status,trigger_mode,created_at) \
                      VALUES (?,?,?,?,?,?,?,'pending',?,?) RETURNING id"
                    .to_string(),
                params: vec![
                    text(a.date),
                    text(a.batch_id),
                    text(a.model),
                    text(a.prompt_version),
                    text(RULE_VERSION),
                    text(env!("CARGO_PKG_VERSION")),
                    text(a.request),
                    text(if a.manual { "manual" } else { "scheduled" }),
                    int(a.now),
                ],
            })
            .await?;
        let id = rows
            .first()
            .and_then(|r| r["id"].as_i64())
            .ok_or_else(|| PortError("missing recommendation id".into()))?;
        // 候補の run_id は取得済みの id を束縛する。同時生成（Actions と管理者の作り直し）
        // で MAX(id) が別の run を指す事故を防ぐ（PK (run_id,code)・UNIQUE (run_id,rank)
        // があるため、リトライで重複してもエラーになるだけ）
        self.store
            .exec(&Stmt {
                sql: "INSERT INTO recommendation_candidates (run_id,code,rule_rank,snapshot_json) \
                      SELECT ?,json_extract(value,'$.code'), \
                             json_extract(value,'$.rule_rank'),json_set(json_extract(value,'$.stock'), \
                             '$.financial_doc_id',(SELECT fin_doc_id FROM indicators_latest WHERE code=json_extract(value,'$.code')), \
                             '$.financial_disclosed',(SELECT fin_submitted FROM indicators_latest WHERE code=json_extract(value,'$.code'))) FROM json_each(?)"
                    .to_string(),
                params: vec![int(id), text(&json!(snapshots).to_string())],
            })
            .await?;
        Ok(id)
    }

    async fn publish(&self, id: i64, response: &str, picks: &[Pick]) -> Result<(), PortError> {
        let mut attempt = 0;
        loop {
            match self.publish_once(id, response, picks).await {
                Ok(()) => return Ok(()),
                Err(e) if attempt < PUBLISH_RETRIES => {
                    attempt += 1;
                    eprintln!("d1: publish: {e}; retry {attempt}/{PUBLISH_RETRIES}");
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    async fn fail(&self, id: i64, response: Option<&str>, error: &str) -> Result<(), PortError> {
        self.store
            .exec(&Stmt {
                sql: "UPDATE recommendation_runs SET status='failed',response_json=?,error=? WHERE id=? AND status='pending'"
                    .to_string(),
                params: vec![
                    response.map_or(Val::Null, text),
                    text(&error.chars().take(2000).collect::<String>()),
                    int(id),
                ],
            })
            .await?;
        Ok(())
    }

    async fn latest(&self) -> Result<Option<Recommendations>, PortError> {
        let values = self
            .store
            .query(&stmt(query::recommendation_rows()))
            .await?;
        let meta = decode_rows::<RecoRow>(values.clone())?;
        let stocks = decode_rows::<Row>(values)?;
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
                    let stock = to_stock_row(s);
                    RecommendationRow {
                        quality_json: m.quality_json.clone(),
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
