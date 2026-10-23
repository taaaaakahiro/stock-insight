//! D1（SQLite）に書き込むSQL文の組み立て。値は必ずプレースホルダで渡す。

use crate::edinet::ReitRecord;
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
     sales_growth, op_growth, dividend_basis, dividend_cut, op_growth_q, net_growth_q, op_growth_ttm, profit_one_off";

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
                Val::opt_b(i.dividend_cut),
                Val::opt_f(i.op_growth_q),
                Val::opt_f(i.net_growth_q),
                Val::opt_f(i.op_growth_ttm),
                Val::opt_b(i.profit_one_off),
            ]
        })
        .collect();
    multi_row(
        &format!("INSERT INTO indicators_latest ({INDICATOR_COLUMNS})"),
        31,
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
         dividend_basis = excluded.dividend_basis, dividend_cut = excluded.dividend_cut, \
         op_growth_q = excluded.op_growth_q, net_growth_q = excluded.net_growth_q, \
         op_growth_ttm = excluded.op_growth_ttm, profit_one_off = excluded.profit_one_off",
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
                Val::opt_f(s.doe_floor),
                Val::opt_f(s.doe_goal),
                s.doe_phrase.clone().map_or(Val::Null, Val::Text),
                Val::opt_f(s.payout_floor),
                Val::opt_f(s.payout_goal),
                s.payout_phrase.clone().map_or(Val::Null, Val::Text),
            ]
        })
        .collect();
    multi_row(
        "INSERT INTO edinet_docs (code, doc_kind, doc_id, submitted, period_end, r2_key, progressive_dividend, \
         no_dividend_cut, dividend_raise, doe_target, payout_target, phrases, fetched_at, \
         doe_floor, doe_goal, doe_phrase, payout_floor, payout_goal, payout_phrase)",
        19,
        "ON CONFLICT(code, doc_kind) DO UPDATE SET doc_id = excluded.doc_id, submitted = excluded.submitted, \
         period_end = excluded.period_end, r2_key = excluded.r2_key, \
         progressive_dividend = excluded.progressive_dividend, no_dividend_cut = excluded.no_dividend_cut, \
         dividend_raise = excluded.dividend_raise, doe_target = excluded.doe_target, \
         payout_target = excluded.payout_target, phrases = excluded.phrases, fetched_at = excluded.fetched_at, \
         doe_floor = excluded.doe_floor, doe_goal = excluded.doe_goal, doe_phrase = excluded.doe_phrase, \
         payout_floor = excluded.payout_floor, payout_goal = excluded.payout_goal, \
         payout_phrase = excluded.payout_phrase",
        rows,
        usize::MAX,
    )
}

/// REIT の最新の要約。同じ銘柄のもっと新しい期がすでにあれば上書きしない（古い書類から順に取り込むため）
pub fn reit_profile_stmt(r: &ReitRecord, sector: Option<&str>, fetched_at: i64) -> Stmt {
    let (b, m, d, p) = (&r.report.balance, &r.mix, &r.debt, &r.portfolio);
    let checks = serde_json::json!({ "loans": r.checks.loans, "properties": r.checks.properties })
        .to_string();
    let cols = [
        "code",
        "doc_id",
        "submitted",
        "period_end",
        "sector",
        "short_loans",
        "long_loans",
        "bonds",
        "rate_fixed",
        "rate_swap_fixed",
        "rate_swap_amount",
        "rate_floating",
        "rate_unknown",
        "swap_notional",
        "avg_rate",
        "avg_years",
        "reprice_1y",
        "reprice_1y_max",
        "property_count",
        "leasable_m2",
        "leased_m2",
        "occupancy",
        "checks",
        "fetched_at",
    ];
    let update: Vec<String> = cols[1..]
        .iter()
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    Stmt {
        sql: format!(
            "INSERT INTO reit_profile ({}) VALUES ({}) ON CONFLICT(code) DO UPDATE SET {} \
             WHERE excluded.period_end >= reit_profile.period_end",
            cols.join(", "),
            vec!["?"; cols.len()].join(", "),
            update.join(", ")
        ),
        params: vec![
            Val::Text(r.code.clone()),
            Val::Text(r.doc.doc_id.clone()),
            Val::Text(r.doc.submitted.to_string()),
            Val::Text(r.period_end.to_string()),
            sector.map_or(Val::Null, |s| Val::Text(s.into())),
            Val::Int(b.short_loans),
            Val::Int(b.long_loans),
            Val::Int(b.bonds),
            Val::Int(m.fixed),
            Val::Int(m.swap_fixed),
            Val::Int(m.swap_amount_only),
            Val::Int(m.floating),
            Val::Int(m.unknown),
            r.report.debt.swap_yen.map_or(Val::Null, Val::Int),
            Val::opt_f(d.avg_rate_pct),
            Val::opt_f(d.avg_years),
            Val::Int(d.reprice_1y),
            Val::Int(d.reprice_1y_max),
            Val::Int(p.property_count as i64),
            Val::opt_f((p.property_count > 0).then_some(p.leasable_m2)),
            Val::opt_f((p.property_count > 0).then_some(p.leased_m2)),
            Val::opt_f(p.occupancy_pct),
            Val::Text(checks),
            Val::Int(fetched_at),
        ],
    }
}

/// REIT の最新の明細（JSON 1つ）。要約と同じく新しい期だけで上書きする
pub fn reit_detail_stmt(r: &ReitRecord, body: &serde_json::Value, fetched_at: i64) -> Stmt {
    Stmt {
        sql: "INSERT INTO reit_detail (code, period_end, body, fetched_at) VALUES (?, ?, ?, ?) \
              ON CONFLICT(code) DO UPDATE SET period_end = excluded.period_end, body = excluded.body, \
              fetched_at = excluded.fetched_at WHERE excluded.period_end >= reit_detail.period_end"
            .into(),
        params: vec![
            Val::Text(r.code.clone()),
            Val::Text(r.period_end.to_string()),
            Val::Text(body.to_string()),
            Val::Int(fetched_at),
        ],
    }
}

/// REIT の決算ごとの要約
pub fn reit_history_stmt(r: &ReitRecord, fetched_at: i64) -> Stmt {
    let loans: i64 = r.report.debt.loans.iter().map(|l| l.balance_yen).sum();
    let p = &r.portfolio;
    Stmt {
        sql: "INSERT INTO reit_history (code, period_end, doc_id, loans_total, bonds, rate_floating, rate_unknown, \
              swap_notional, avg_rate, property_count, leasable_m2, occupancy, fetched_at) \
              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
              ON CONFLICT(code, period_end) DO UPDATE SET doc_id = excluded.doc_id, loans_total = excluded.loans_total, \
              bonds = excluded.bonds, rate_floating = excluded.rate_floating, rate_unknown = excluded.rate_unknown, \
              swap_notional = excluded.swap_notional, avg_rate = excluded.avg_rate, \
              property_count = excluded.property_count, leasable_m2 = excluded.leasable_m2, \
              occupancy = excluded.occupancy, fetched_at = excluded.fetched_at"
            .into(),
        params: vec![
            Val::Text(r.code.clone()),
            Val::Text(r.period_end.to_string()),
            Val::Text(r.doc.doc_id.clone()),
            // 明細が読めなかったときは BS の額
            Val::Int(if loans > 0 { loans } else { r.report.balance.short_loans + r.report.balance.long_loans }),
            Val::Int(r.report.balance.bonds),
            Val::Int(r.mix.floating),
            Val::Int(r.mix.unknown),
            r.report.debt.swap_yen.map_or(Val::Null, Val::Int),
            Val::opt_f(r.debt.avg_rate_pct),
            Val::Int(p.property_count as i64),
            Val::opt_f((p.property_count > 0).then_some(p.leasable_m2)),
            Val::opt_f(p.occupancy_pct),
            Val::Int(fetched_at),
        ],
    }
}

/// 用途の対応表（data/reit_sectors.csv）を reit_profile に反映する。対応表を直したときに新しい書類を待たずに効くよう毎回流す
pub fn reit_sector_stmts(sectors: &std::collections::HashMap<String, String>) -> Vec<Stmt> {
    let mut pairs: Vec<(&String, &String)> = sectors.iter().collect();
    pairs.sort();
    // CASE の (code, sector) と IN の code で1組3個。パラメータは100個まで
    pairs
        .chunks(33)
        .map(|chunk| Stmt {
            sql: format!(
                "UPDATE reit_profile SET sector = CASE code {} END WHERE code IN ({})",
                vec!["WHEN ? THEN ?"; chunk.len()].join(" "),
                vec!["?"; chunk.len()].join(", ")
            ),
            params: chunk
                .iter()
                .flat_map(|(c, s)| [Val::Text((*c).clone()), Val::Text((*s).clone())])
                .chain(chunk.iter().map(|(c, _)| Val::Text((*c).clone())))
                .collect(),
        })
        .collect()
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
                dividend_cut: Some(true),
                op_growth_q: Some(0.2),
                net_growth_q: None,
                op_growth_ttm: None,
                profit_one_off: Some(false),
            },
            fin_doc_id: None,
            fin_submitted: None,
        };
        let snaps: Vec<_> = (0..12).map(|i| snap(&format!("{}", 1300 + i))).collect();
        let stmts = snapshots_stmts(&snaps);
        assert_eq!(stmts.len(), 4, "31列なので1文3行まで");
        assert!(stmts.iter().all(|s| s.params.len() <= 100));
        assert_eq!(stmts[0].sql.matches("(?, ").count(), 3);
        assert_eq!(stmts[0].params[25], Val::Text("forecast".into()));
        assert_eq!(stmts[0].params[26], Val::Int(1));
        assert_eq!(stmts[0].params[27], Val::Real(0.2));
        assert_eq!(stmts[0].params[30], Val::Int(0));
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
                payout_floor: Some(0.3),
                payout_phrase: Some("配当性向30%以上を基本とします。".into()),
                doe_floor: Some(0.035),
                doe_phrase: Some("DOE3.5%を下限とします。".into()),
                phrases: vec!["累進配当を基本方針とし…".into()],
                ..Default::default()
            },
        };
        let stmts = edinet_docs_stmts(&vec![doc; 8], 1);
        assert_eq!(stmts.len(), 2, "19列なので1文5行まで");
        let p = &stmts[0].params;
        assert_eq!(p[5], Val::Text("edinet/7203/annual.json.gz".into()));
        assert_eq!(p[6], Val::Int(1));
        assert_eq!(p[7], Val::Int(0));
        assert_eq!(p[11], Val::Text(r#"["累進配当を基本方針とし…"]"#.into()));
        assert_eq!(p[13], Val::Real(0.035));
        assert_eq!(p[14], Val::Null);
        assert_eq!(p[15], Val::Text("DOE3.5%を下限とします。".into()));
        assert_eq!(p[16], Val::Real(0.3));
        assert_eq!(p[17], Val::Null);
        assert_eq!(p[18], Val::Text("配当性向30%以上を基本とします。".into()));
        let del = delete_edinet_doc_stmt(&("7203".into(), DocKind::Semiannual));
        assert_eq!(
            del.render(),
            "DELETE FROM edinet_docs WHERE code = '7203' AND doc_kind = 'semiannual';"
        );
    }

    #[test]
    fn reit_sectors_fit_the_parameter_limit() {
        let sectors: std::collections::HashMap<String, String> = (0..70)
            .map(|i| (format!("{:04}", 1000 + i), "office".to_string()))
            .collect();
        let stmts = reit_sector_stmts(&sectors);
        assert_eq!(stmts.len(), 3);
        assert!(stmts.iter().all(|s| s.params.len() <= 100));
        assert_eq!(stmts[0].params.len(), 99);
        assert!(stmts[0]
            .sql
            .starts_with("UPDATE reit_profile SET sector = CASE code WHEN ? THEN ?"));
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
