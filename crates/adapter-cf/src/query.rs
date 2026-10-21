//! スクリーニング一覧の SQL を組み立てる。列名は domain の固定の列挙から取り、値は必ずプレースホルダで渡す。

use domain::screening::{ScreeningFilter, SortKey, SortOrder, PAGE_SIZE};

/// プレースホルダに渡す値
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Real(f64),
    Int(i64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub sql: String,
    pub args: Vec<Arg>,
}

pub const LIST_COLUMNS: &str =
    "code, price, price_date, dividend_yield, yield_percentile, pbr, per, \
     payout_ratio, doe, consecutive_increases, operating_margin, no_loss_years, profit_uptrend, \
     valuation, fetched_at";

fn where_clause(f: &ScreeningFilter) -> (String, Vec<Arg>) {
    let mut conds = vec![];
    let mut args = vec![];
    for r in &f.ranges {
        let col = r.metric.key();
        if let Some(min) = r.min {
            conds.push(format!("{col} >= ?"));
            args.push(Arg::Real(min));
        }
        if let Some(max) = r.max {
            conds.push(format!("{col} <= ?"));
            args.push(Arg::Real(max));
        }
    }
    if let Some(v) = f.valuation {
        conds.push("valuation = ?".to_string());
        args.push(Arg::Text(v.as_str().to_string()));
    }
    if f.no_loss_years {
        conds.push("no_loss_years = 1".to_string());
    }
    if f.profit_uptrend {
        conds.push("profit_uptrend = 1".to_string());
    }
    if conds.is_empty() {
        (String::new(), args)
    } else {
        (format!(" WHERE {}", conds.join(" AND ")), args)
    }
}

/// 1ページ分の行
pub fn list(f: &ScreeningFilter) -> Query {
    let (w, mut args) = where_clause(f);
    let col = f.sort.key();
    let dir = match f.order {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    // 値のない銘柄は並べ替えの向きによらず末尾に置く
    let nulls = match f.sort {
        SortKey::Code => String::new(),
        SortKey::Metric(_) => format!("{col} IS NULL, "),
    };
    args.push(Arg::Int(PAGE_SIZE as i64));
    args.push(Arg::Int(f.offset() as i64));
    Query {
        sql: format!(
            "SELECT {LIST_COLUMNS} FROM indicators_latest{w} ORDER BY {nulls}{col} {dir}, code ASC LIMIT ? OFFSET ?"
        ),
        args,
    }
}

/// 条件に合う件数
pub fn count(f: &ScreeningFilter) -> Query {
    let (w, args) = where_clause(f);
    Query {
        sql: format!("SELECT COUNT(*) AS n FROM indicators_latest{w}"),
        args,
    }
}

pub const LATEST_RUN: &str = "SELECT r.run_id, r.started_at, r.finished_at, r.status, \
     (SELECT MAX(price_date) FROM indicators_latest) AS latest_price_date \
     FROM batch_runs r ORDER BY r.started_at DESC LIMIT 1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_filtered_query() {
        let f = ScreeningFilter::from_pairs([
            ("dividend_yield_min", "0.03"),
            ("pbr_max", "1.0"),
            ("valuation", "cheap"),
            ("no_loss_years", "1"),
            ("sort", "pbr"),
            ("order", "asc"),
            ("page", "3"),
        ])
        .unwrap();
        let q = list(&f);
        assert!(q.sql.ends_with(
            "FROM indicators_latest WHERE dividend_yield >= ? AND pbr <= ? AND valuation = ? AND no_loss_years = 1 \
             ORDER BY pbr IS NULL, pbr ASC, code ASC LIMIT ? OFFSET ?"
        ));
        assert_eq!(
            q.args,
            vec![
                Arg::Real(0.03),
                Arg::Real(1.0),
                Arg::Text("cheap".into()),
                Arg::Int(50),
                Arg::Int(100)
            ]
        );
        let c = count(&f);
        assert_eq!(c.args.len(), 3);
        assert!(!c.sql.contains("LIMIT"));
    }

    #[test]
    fn default_query_has_no_where() {
        let q = list(&ScreeningFilter::default());
        assert!(!q.sql.contains("WHERE"));
        assert!(q
            .sql
            .contains("ORDER BY dividend_yield IS NULL, dividend_yield DESC"));
    }
}
