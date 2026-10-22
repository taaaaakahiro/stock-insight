//! D1（SQLite）に書き込むSQL文の組み立て。値は必ずプレースホルダで渡す。

use domain::analysis::PeriodType;
use domain::split::Split;
use domain::stock_master::ListedStock;
use usecase::edinet::{r2_key, DocKey, EdinetDoc};
use usecase::fins::{AnnualReport, ForecastReport, PeriodReport};
use usecase::{BatchRun, Snapshot};

#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Val {
    fn opt_f(v: Option<f64>) -> Self {
        v.filter(|x| x.is_finite()).map_or(Val::Null, Val::Real)
    }
    fn opt_b(v: Option<bool>) -> Self {
        v.map_or(Val::Null, |b| Val::Int(b as i64))
    }
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Val::Null => serde_json::Value::Null,
            Val::Int(i) => (*i).into(),
            Val::Real(f) => (*f).into(),
            Val::Text(s) => s.clone().into(),
        }
    }
    /// SQLファイル出力用のリテラル（ドライラン専用）
    pub fn to_literal(&self) -> String {
        match self {
            Val::Null => "NULL".into(),
            Val::Int(i) => i.to_string(),
            Val::Real(f) => format!("{f:?}"),
            Val::Text(s) => format!("'{}'", s.replace('\'', "''")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub sql: String,
    pub params: Vec<Val>,
}

impl Stmt {
    /// ? を順にリテラルへ置き換える（ドライラン専用。本番は params をAPIに渡す）
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut it = self.params.iter();
        for ch in self.sql.chars() {
            if ch == '?' {
                out.push_str(&it.next().map_or("NULL".into(), Val::to_literal));
            } else {
                out.push(ch);
            }
        }
        out.push(';');
        out
    }
}

pub fn run_stmt(r: &BatchRun) -> Stmt {
    Stmt {
        sql: "INSERT INTO batch_runs (run_id, started_at, finished_at, target, succeeded, failed, status) \
              VALUES (?, ?, ?, ?, ?, ?, ?) \
              ON CONFLICT(run_id) DO UPDATE SET finished_at = excluded.finished_at, target = excluded.target, \
              succeeded = excluded.succeeded, failed = excluded.failed, status = excluded.status"
            .into(),
        params: vec![
            Val::Text(r.run_id.clone()),
            Val::Int(r.started_at),
            r.finished_at.map_or(Val::Null, Val::Int),
            Val::Int(r.target as i64),
            Val::Int(r.succeeded as i64),
            Val::Int(r.failed as i64),
            Val::Text(r.status.into()),
        ],
    }
}

/// 同じ形の行を、パラメータが100個を超えないようにまとめた複数行 INSERT に分ける
fn multi_row(
    head: &str,
    cols: usize,
    tail: &str,
    rows: Vec<Vec<Val>>,
    max_rows: usize,
) -> Vec<Stmt> {
    let per = (100 / cols).min(max_rows).max(1);
    let placeholder = format!("({})", vec!["?"; cols].join(", "));
    rows.chunks(per)
        .map(|chunk| Stmt {
            sql: format!(
                "{head} VALUES {} {tail}",
                vec![placeholder.as_str(); chunk.len()].join(", ")
            ),
            params: chunk.iter().flatten().cloned().collect(),
        })
        .collect()
}

const INDICATOR_COLUMNS: &str = "code, run_id, fetched_at, price, price_date, dividend_ttm, dividend_yield, \
     yield_percentile, pbr, per, payout_ratio, doe, consecutive_increases, operating_margin, no_loss_years, \
     profit_uptrend, valuation, fin_doc_id, fin_submitted, dividend_every_year, no_quarterly_loss, is_value, data_years, \
     sales_growth, op_growth, dividend_basis";

/// 一覧用の最新指標（1銘柄1行）を upsert する
pub fn snapshots_stmts(snaps: &[Snapshot]) -> Vec<Stmt> {
    let rows = snaps
        .iter()
        .map(|s| {
            let i = &s.indicators;
            vec![
                Val::Text(s.code.as_str().into()),
                Val::Text(s.run_id.clone()),
                Val::Int(s.fetched_at),
                Val::Real(i.price),
                Val::Text(i.price_date.to_string()),
                Val::Real(i.dividend_ttm),
                Val::opt_f(i.dividend_yield),
                Val::opt_f(i.yield_percentile),
                Val::opt_f(i.pbr),
                Val::opt_f(i.per),
                Val::opt_f(i.payout_ratio),
                Val::opt_f(i.doe),
                i.consecutive_increases
                    .map_or(Val::Null, |n| Val::Int(n as i64)),
                Val::opt_f(i.operating_margin),
                Val::opt_b(i.no_loss_years),
                Val::opt_b(i.profit_uptrend),
                Val::Text(i.valuation.as_str().into()),
                s.fin_doc_id.clone().map_or(Val::Null, Val::Text),
                s.fin_submitted
                    .map_or(Val::Null, |d| Val::Text(d.to_string())),
                Val::opt_b(i.dividend_every_year),
                Val::opt_b(i.no_quarterly_loss),
                Val::opt_b(i.is_value),
                Val::Int(i.data_years as i64),
                Val::opt_f(i.sales_growth),
                Val::opt_f(i.op_growth),
                i.dividend_is_forecast.map_or(Val::Null, |f| {
                    Val::Text(if f { "forecast" } else { "actual" }.into())
                }),
            ]
        })
        .collect();
    multi_row(
        &format!("INSERT INTO indicators_latest ({INDICATOR_COLUMNS})"),
        26,
        "ON CONFLICT(code) DO UPDATE SET run_id = excluded.run_id, fetched_at = excluded.fetched_at, \
         price = excluded.price, price_date = excluded.price_date, dividend_ttm = excluded.dividend_ttm, \
         dividend_yield = excluded.dividend_yield, yield_percentile = excluded.yield_percentile, pbr = excluded.pbr, \
         per = excluded.per, payout_ratio = excluded.payout_ratio, doe = excluded.doe, \
         consecutive_increases = excluded.consecutive_increases, operating_margin = excluded.operating_margin, \
         no_loss_years = excluded.no_loss_years, profit_uptrend = excluded.profit_uptrend, \
         valuation = excluded.valuation, fin_doc_id = excluded.fin_doc_id, fin_submitted = excluded.fin_submitted, \
         dividend_every_year = excluded.dividend_every_year, no_quarterly_loss = excluded.no_quarterly_loss, \
         is_value = excluded.is_value, data_years = excluded.data_years, \
         sales_growth = excluded.sales_growth, op_growth = excluded.op_growth, \
         dividend_basis = excluded.dividend_basis",
        rows,
        usize::MAX,
    )
}

/// 年次財務（本決算）を upsert する。同じ期の訂正は上書き
pub fn annual_reports_stmts(reports: &[AnnualReport], fetched_at: i64) -> Vec<Stmt> {
    let rows = reports
        .iter()
        .filter_map(|r| {
            let y = &r.financial;
            Some(vec![
                Val::Text(r.code.as_str().into()),
                Val::Text(y.period_end?.to_string()),
                Val::Text(r.doc_id.clone()),
                Val::opt_f(y.net_sales),
                Val::opt_f(y.operating_income),
                Val::opt_f(y.ordinary_income),
                Val::opt_f(y.net_income),
                Val::opt_f(y.equity),
                Val::opt_f(y.eps),
                Val::opt_f(y.bps),
                Val::opt_f(y.dps),
                Val::opt_f(y.operating_cf),
                Val::Int(fetched_at),
                r.disclosed.map_or(Val::Null, |d| Val::Text(d.to_string())),
            ])
        })
        .collect();
    multi_row(
        "INSERT INTO financials_annual (code, period_end, doc_id, net_sales, operating_income, ordinary_income, \
         net_income, equity, eps, bps, dps, operating_cf, fetched_at, disclosed)",
        14,
        "ON CONFLICT(code, period_end) DO UPDATE SET doc_id = excluded.doc_id, net_sales = excluded.net_sales, \
         operating_income = excluded.operating_income, ordinary_income = excluded.ordinary_income, \
         net_income = excluded.net_income, equity = excluded.equity, eps = excluded.eps, bps = excluded.bps, \
         dps = excluded.dps, operating_cf = excluded.operating_cf, fetched_at = excluded.fetched_at, \
         disclosed = excluded.disclosed",
        rows,
        usize::MAX,
    )
}

/// 四半期の累計値（1Q/2Q/3Q だけ。FY は financials_annual にある）を upsert する
pub fn period_reports_stmts(reports: &[PeriodReport], fetched_at: i64) -> Vec<Stmt> {
    let rows = reports
        .iter()
        .filter(|r| r.period.period_type != PeriodType::FY)
        .map(|r| {
            let p = &r.period;
            vec![
                Val::Text(r.code.as_str().into()),
                Val::Text(p.fiscal_year_end.to_string()),
                Val::Text(p.period_type.as_str().into()),
                Val::Text(p.period_end.to_string()),
                Val::Text(r.doc_id.clone()),
                r.disclosed.map_or(Val::Null, |d| Val::Text(d.to_string())),
                Val::opt_f(p.net_sales),
                Val::opt_f(p.operating_income),
                Val::opt_f(p.ordinary_income),
                Val::opt_f(p.net_income),
                Val::Int(fetched_at),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO financials_quarterly (code, fiscal_year_end, period_type, period_end, doc_id, disclosed, \
         net_sales, operating_income, ordinary_income, net_income, fetched_at)",
        11,
        "ON CONFLICT(code, fiscal_year_end, period_type) DO UPDATE SET period_end = excluded.period_end, \
         doc_id = excluded.doc_id, disclosed = excluded.disclosed, net_sales = excluded.net_sales, \
         operating_income = excluded.operating_income, ordinary_income = excluded.ordinary_income, \
         net_income = excluded.net_income, fetched_at = excluded.fetched_at",
        rows,
        usize::MAX,
    )
}

/// 会社予想の配当（開示1件1行）を upsert する
pub fn forecasts_stmts(reports: &[ForecastReport], fetched_at: i64) -> Vec<Stmt> {
    let rows = reports
        .iter()
        .map(|r| {
            vec![
                Val::Text(r.code.as_str().into()),
                Val::Text(r.doc_id.clone()),
                Val::Text(r.disclosed.to_string()),
                r.fiscal_year_end
                    .map_or(Val::Null, |d| Val::Text(d.to_string())),
                Val::opt_f(r.dps),
                Val::Int(fetched_at),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO dividend_forecasts (code, doc_id, disclosed, fiscal_year_end, dps, fetched_at)",
        6,
        "ON CONFLICT(code, doc_id) DO UPDATE SET disclosed = excluded.disclosed, \
         fiscal_year_end = excluded.fiscal_year_end, dps = excluded.dps, fetched_at = excluded.fetched_at",
        rows,
        usize::MAX,
    )
}

/// 株価の履歴（1銘柄1行）を upsert する。1行が数KBになるので、1文あたりの行数を抑える
pub fn price_history_stmts(rows: &[(String, String, String)], now: i64) -> Vec<Stmt> {
    let rows = rows
        .iter()
        .map(|(code, closes, last_date)| {
            vec![
                Val::Text(code.clone()),
                Val::Text(closes.clone()),
                Val::Text(last_date.clone()),
                Val::Int(now),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO price_history (code, closes, last_date, updated_at)",
        4,
        "ON CONFLICT(code) DO UPDATE SET closes = excluded.closes, last_date = excluded.last_date, \
         updated_at = excluded.updated_at",
        rows,
        10,
    )
}

/// 株式分割・併合の upsert（同じ日は上書き）
pub fn splits_stmts(rows: &[(String, Split)], now: i64) -> Vec<Stmt> {
    let rows = rows
        .iter()
        .map(|(code, s)| {
            vec![
                Val::Text(code.clone()),
                Val::Text(s.date.to_string()),
                Val::Real(s.factor),
                Val::Int(now),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO splits (code, date, factor, fetched_at)",
        4,
        "ON CONFLICT(code, date) DO UPDATE SET factor = excluded.factor, fetched_at = excluded.fetched_at",
        rows,
        25,
    )
}

pub fn set_state_stmt(key: &str, value: &str, now: i64) -> Stmt {
    Stmt {
        sql: "INSERT INTO sync_state (key, value, updated_at) VALUES (?, ?, ?) \
              ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
            .into(),
        params: vec![
            Val::Text(key.into()),
            Val::Text(value.into()),
            Val::Int(now),
        ],
    }
}

/// D1 は1文あたりのパラメータが100個まで。8列なので12行ずつまとめる
const STOCK_ROWS_PER_STMT: usize = 12;

/// 銘柄マスタの upsert（複数行の INSERT に分ける）
pub fn stocks_stmts(as_of: &str, stocks: &[ListedStock], now: i64) -> Vec<Stmt> {
    stocks
        .chunks(STOCK_ROWS_PER_STMT)
        .map(|chunk| {
            let values = vec!["(?, ?, ?, ?, ?, ?, ?, ?)"; chunk.len()].join(", ");
            Stmt {
                sql: format!(
                    "INSERT INTO stocks (code, name, kind, market, sector33, size_class, as_of, updated_at) VALUES {values} \
                     ON CONFLICT(code) DO UPDATE SET name = excluded.name, kind = excluded.kind, market = excluded.market, \
                     sector33 = excluded.sector33, size_class = excluded.size_class, as_of = excluded.as_of, \
                     updated_at = excluded.updated_at"
                ),
                params: chunk
                    .iter()
                    .flat_map(|s| {
                        [
                            Val::Text(s.code.as_str().into()),
                            Val::Text(s.name.clone()),
                            Val::Text(s.kind.as_str().into()),
                            Val::Text(s.market.clone()),
                            s.sector33.clone().map_or(Val::Null, Val::Text),
                            Val::Text(s.size_class.as_str().into()),
                            Val::Text(as_of.into()),
                            Val::Int(now),
                        ]
                    })
                    .collect(),
            }
        })
        .collect()
}

/// EDINET の書類（銘柄・種類ごとに最新1通）を upsert する
pub fn edinet_docs_stmts(docs: &[EdinetDoc], fetched_at: i64) -> Vec<Stmt> {
    let rows = docs
        .iter()
        .map(|e| {
            let (d, s) = (&e.doc, &e.signals);
            vec![
                Val::Text(d.code.clone()),
                Val::Text(d.kind.as_str().into()),
                Val::Text(d.doc_id.clone()),
                Val::Text(d.submitted.to_string()),
                d.period_end.map_or(Val::Null, |p| Val::Text(p.to_string())),
                Val::Text(r2_key(&d.code, d.kind)),
                Val::Int(s.progressive as i64),
                Val::Int(s.no_cut as i64),
                Val::Int(s.raise as i64),
                Val::Int(s.doe_target as i64),
                Val::opt_f(s.payout_target),
                Val::Text(serde_json::to_string(&s.phrases).unwrap_or_else(|_| "[]".into())),
                Val::Int(fetched_at),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO edinet_docs (code, doc_kind, doc_id, submitted, period_end, r2_key, progressive_dividend, \
         no_dividend_cut, dividend_raise, doe_target, payout_target, phrases, fetched_at)",
        13,
        "ON CONFLICT(code, doc_kind) DO UPDATE SET doc_id = excluded.doc_id, submitted = excluded.submitted, \
         period_end = excluded.period_end, r2_key = excluded.r2_key, \
         progressive_dividend = excluded.progressive_dividend, no_dividend_cut = excluded.no_dividend_cut, \
         dividend_raise = excluded.dividend_raise, doe_target = excluded.doe_target, \
         payout_target = excluded.payout_target, phrases = excluded.phrases, fetched_at = excluded.fetched_at",
        rows,
        usize::MAX,
    )
}

/// 古くなった書類の行を消す（R2 のファイルはバッチの最後に消す）
pub fn delete_edinet_doc_stmt(key: &DocKey) -> Stmt {
    Stmt {
        sql: "DELETE FROM edinet_docs WHERE code = ? AND doc_kind = ?".into(),
        params: vec![Val::Text(key.0.clone()), Val::Text(key.1.as_str().into())],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stocks_are_chunked_under_the_param_limit() {
        use domain::stock_master::listed_stock_from_jpx;
        let stocks: Vec<_> = (0..30)
            .map(|i| {
                listed_stock_from_jpx(
                    &format!("{}", 1300 + i),
                    "名前",
                    "プライム（内国株式）",
                    "-",
                    "-",
                )
                .unwrap()
            })
            .collect();
        let stmts = stocks_stmts("20260930", &stocks, 1);
        assert_eq!(stmts.len(), 3);
        assert!(stmts.iter().all(|s| s.params.len() <= 100));
        assert_eq!(stmts[2].params.len(), 6 * 8);
        assert_eq!(stmts[0].params[4], Val::Null, "業種 '-' は NULL");
    }

    #[test]
    fn indicator_rows_are_batched_under_the_param_limit() {
        use domain::{Date, Indicators, StockCode, Valuation};
        let snap = |c: &str| Snapshot {
            code: StockCode::parse(c).unwrap(),
            run_id: "r".into(),
            fetched_at: 1,
            indicators: Indicators {
                price: 100.0,
                price_date: Date::new(2026, 7, 1),
                dividend_ttm: 4.0,
                dividend_yield: Some(0.04),
                yield_percentile: None,
                pbr: None,
                per: None,
                payout_ratio: None,
                doe: None,
                consecutive_increases: None,
                operating_margin: None,
                no_loss_years: None,
                profit_uptrend: None,
                valuation: Valuation::Unknown,
                dividend_every_year: None,
                no_quarterly_loss: None,
                is_value: None,
                data_years: 0,
                sales_growth: None,
                op_growth: None,
                dividend_is_forecast: Some(true),
            },
            fin_doc_id: None,
            fin_submitted: None,
        };
        let snaps: Vec<_> = (0..12).map(|i| snap(&format!("{}", 1300 + i))).collect();
        let stmts = snapshots_stmts(&snaps);
        assert_eq!(stmts.len(), 4, "26列なので1文3行まで");
        assert!(stmts.iter().all(|s| s.params.len() <= 100));
        assert_eq!(stmts[0].sql.matches("(?, ").count(), 3);
        assert_eq!(stmts[0].params[25], Val::Text("forecast".into()));
        let h = price_history_stmts(
            &vec![("7203".into(), "x".into(), "2026-07-01".into()); 25],
            1,
        );
        assert_eq!(h.len(), 3, "履歴は1文10行まで");
    }

    #[test]
    fn edinet_rows_keep_signals_and_phrases() {
        use domain::disclosure::{DividendSignals, DocKind};
        use domain::Date;
        use usecase::edinet::DocRef;
        let doc = EdinetDoc {
            doc: DocRef {
                code: "7203".into(),
                kind: DocKind::Annual,
                doc_id: "S100AAAA".into(),
                submitted: Date::new(2026, 6, 20),
                period_end: Some(Date::new(2026, 3, 31)),
            },
            signals: DividendSignals {
                progressive: true,
                payout_target: Some(0.3),
                phrases: vec!["累進配当を基本方針とし…".into()],
                ..Default::default()
            },
        };
        let stmts = edinet_docs_stmts(&vec![doc; 8], 1);
        assert_eq!(stmts.len(), 2, "13列なので1文7行まで");
        let p = &stmts[0].params;
        assert_eq!(p[5], Val::Text("edinet/7203/annual.json.gz".into()));
        assert_eq!(p[6], Val::Int(1));
        assert_eq!(p[7], Val::Int(0));
        assert_eq!(p[11], Val::Text(r#"["累進配当を基本方針とし…"]"#.into()));
        let del = delete_edinet_doc_stmt(&("7203".into(), DocKind::Semiannual));
        assert_eq!(
            del.render(),
            "DELETE FROM edinet_docs WHERE code = '7203' AND doc_kind = 'semiannual';"
        );
    }

    #[test]
    fn render_escapes_quotes() {
        let s = Stmt {
            sql: "SELECT ?, ?, ?".into(),
            params: vec![Val::Text("it's".into()), Val::Null, Val::Real(1.5)],
        };
        assert_eq!(s.render(), "SELECT 'it''s', NULL, 1.5;");
    }
}
