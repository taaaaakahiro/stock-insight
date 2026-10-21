//! D1（SQLite）に書き込むSQL文の組み立て。値は必ずプレースホルダで渡す。

use domain::stock_master::ListedStock;
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

pub fn snapshot_stmts(s: &Snapshot) -> Vec<Stmt> {
    let i = &s.indicators;
    let mut out = vec![Stmt {
        sql: "INSERT INTO indicators_latest (code, run_id, fetched_at, price, price_date, dividend_ttm, dividend_yield, \
              yield_percentile, pbr, per, payout_ratio, doe, consecutive_increases, operating_margin, no_loss_years, \
              profit_uptrend, valuation, fin_doc_id, fin_submitted) \
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
              ON CONFLICT(code) DO UPDATE SET run_id = excluded.run_id, fetched_at = excluded.fetched_at, \
              price = excluded.price, price_date = excluded.price_date, dividend_ttm = excluded.dividend_ttm, \
              dividend_yield = excluded.dividend_yield, yield_percentile = excluded.yield_percentile, pbr = excluded.pbr, \
              per = excluded.per, payout_ratio = excluded.payout_ratio, doe = excluded.doe, \
              consecutive_increases = excluded.consecutive_increases, operating_margin = excluded.operating_margin, \
              no_loss_years = excluded.no_loss_years, profit_uptrend = excluded.profit_uptrend, \
              valuation = excluded.valuation, fin_doc_id = excluded.fin_doc_id, fin_submitted = excluded.fin_submitted"
            .into(),
        params: vec![
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
            i.consecutive_increases.map_or(Val::Null, |n| Val::Int(n as i64)),
            Val::opt_f(i.operating_margin),
            Val::opt_b(i.no_loss_years),
            Val::opt_b(i.profit_uptrend),
            Val::Text(i.valuation.as_str().into()),
            s.financials.as_ref().map_or(Val::Null, |f| Val::Text(f.doc_id.clone())),
            s.financials.as_ref().and_then(|f| f.submitted).map_or(Val::Null, |d| Val::Text(d.to_string())),
        ],
    }];
    if let Some(f) = &s.financials {
        for y in f.years.iter().filter(|y| y.period_end.is_some()) {
            out.push(Stmt {
                sql: "INSERT INTO financials_annual (code, period_end, doc_id, net_sales, operating_income, ordinary_income, \
                      net_income, equity, eps, bps, dps, operating_cf, fetched_at) \
                      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
                      ON CONFLICT(code, period_end) DO UPDATE SET doc_id = excluded.doc_id, net_sales = excluded.net_sales, \
                      operating_income = COALESCE(excluded.operating_income, financials_annual.operating_income), \
                      ordinary_income = excluded.ordinary_income, net_income = excluded.net_income, equity = excluded.equity, \
                      eps = excluded.eps, bps = excluded.bps, dps = excluded.dps, operating_cf = excluded.operating_cf, \
                      fetched_at = excluded.fetched_at"
                    .into(),
                params: vec![
                    Val::Text(s.code.as_str().into()),
                    Val::Text(y.period_end.unwrap().to_string()),
                    Val::Text(f.doc_id.clone()),
                    Val::opt_f(y.net_sales),
                    Val::opt_f(y.operating_income),
                    Val::opt_f(y.ordinary_income),
                    Val::opt_f(y.net_income),
                    Val::opt_f(y.equity),
                    Val::opt_f(y.eps),
                    Val::opt_f(y.bps),
                    Val::opt_f(y.dps),
                    Val::opt_f(y.operating_cf),
                    Val::Int(s.fetched_at),
                ],
            });
        }
    }
    out
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
    fn render_escapes_quotes() {
        let s = Stmt {
            sql: "SELECT ?, ?, ?".into(),
            params: vec![Val::Text("it's".into()), Val::Null, Val::Real(1.5)],
        };
        assert_eq!(s.render(), "SELECT 'it''s', NULL, 1.5;");
    }
}
