//! スクリーニング一覧の SQL を組み立てる。列名は domain の固定の列挙から取り、値は必ずプレースホルダで渡す。
//! 指標（indicators_latest）に銘柄マスタ（stocks）をつないで、名称・種類・業種・規模区分も返す。

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
    "i.code, i.price, i.price_date, i.dividend_yield, i.yield_percentile, i.pbr, i.per, \
     i.payout_ratio, i.doe, i.consecutive_increases, i.operating_margin, i.no_loss_years, i.profit_uptrend, \
     i.valuation, i.fetched_at, s.name, COALESCE(s.kind, 'stock') AS kind, s.market, s.sector33, s.size_class, \
     i.dividend_every_year, i.no_quarterly_loss, i.is_value, i.data_years";

/// マスタにまだない銘柄も消えないよう LEFT JOIN（種類は株式として扱う）
const FROM: &str = "FROM indicators_latest i LEFT JOIN stocks s ON s.code = i.code";

fn where_clause(f: &ScreeningFilter) -> (String, Vec<Arg>) {
    let mut conds = vec!["COALESCE(s.kind, 'stock') = ?".to_string()];
    let mut args = vec![Arg::Text(f.kind.as_str().to_string())];
    for r in &f.ranges {
        let col = r.metric.key();
        if let Some(min) = r.min {
            conds.push(format!("i.{col} >= ?"));
            args.push(Arg::Real(min));
        }
        if let Some(max) = r.max {
            conds.push(format!("i.{col} <= ?"));
            args.push(Arg::Real(max));
        }
    }
    if let Some(v) = f.valuation {
        conds.push("i.valuation = ?".to_string());
        args.push(Arg::Text(v.as_str().to_string()));
    }
    for (col, values) in [
        ("s.sector33", &f.sectors),
        ("s.market", &f.markets),
        ("s.size_class", &f.sizes),
    ] {
        if !values.is_empty() {
            conds.push(format!("{col} IN ({})", vec!["?"; values.len()].join(", ")));
            args.extend(values.iter().map(|v| Arg::Text(v.clone())));
        }
    }
    if let Some(q) = &f.query {
        // LIKE の特殊文字をエスケープし、コードは前方一致・名前は部分一致で探す
        let escaped = q
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        conds.push("(i.code LIKE ? ESCAPE '\\' OR s.name LIKE ? ESCAPE '\\')".to_string());
        args.push(Arg::Text(format!("{escaped}%")));
        args.push(Arg::Text(format!("%{escaped}%")));
    }
    if f.no_loss_years {
        conds.push("i.no_loss_years = 1".to_string());
    }
    if f.profit_uptrend {
        conds.push("i.profit_uptrend = 1".to_string());
    }
    if f.is_value {
        conds.push("i.is_value = 1".to_string());
    }
    if f.dividend_every_year {
        conds.push("i.dividend_every_year = 1".to_string());
    }
    if f.no_quarterly_loss {
        conds.push("i.no_quarterly_loss = 1".to_string());
    }
    (format!(" WHERE {}", conds.join(" AND ")), args)
}

/// 1ページ分の行
pub fn list(f: &ScreeningFilter) -> Query {
    let (w, mut args) = where_clause(f);
    let col = format!("i.{}", f.sort.key());
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
            "SELECT {LIST_COLUMNS} {FROM}{w} ORDER BY {nulls}{col} {dir}, i.code ASC LIMIT ? OFFSET ?"
        ),
        args,
    }
}

/// 条件に合う件数
pub fn count(f: &ScreeningFilter) -> Query {
    let (w, args) = where_clause(f);
    Query {
        sql: format!("SELECT COUNT(*) AS n {FROM}{w}"),
        args,
    }
}

/// ポートフォリオに登録した銘柄の最新指標（? はユーザーID）
pub fn portfolio_rows() -> String {
    format!(
        "SELECT {LIST_COLUMNS} FROM portfolio_items p JOIN indicators_latest i ON i.code = p.code \
         LEFT JOIN stocks s ON s.code = p.code WHERE p.user_id = ?"
    )
}

pub const PORTFOLIO_CODES: &str =
    "SELECT code FROM portfolio_items WHERE user_id = ? ORDER BY added_at, code";
pub const PORTFOLIO_ADD: &str =
    "INSERT INTO portfolio_items (user_id, code, added_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING";
pub const PORTFOLIO_REMOVE: &str = "DELETE FROM portfolio_items WHERE user_id = ? AND code = ?";

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
            "FROM indicators_latest i LEFT JOIN stocks s ON s.code = i.code \
             WHERE COALESCE(s.kind, 'stock') = ? AND i.dividend_yield >= ? AND i.pbr <= ? AND i.valuation = ? \
             AND i.no_loss_years = 1 ORDER BY i.pbr IS NULL, i.pbr ASC, i.code ASC LIMIT ? OFFSET ?"
        ));
        assert_eq!(
            q.args,
            vec![
                Arg::Text("stock".into()),
                Arg::Real(0.03),
                Arg::Real(1.0),
                Arg::Text("cheap".into()),
                Arg::Int(50),
                Arg::Int(100)
            ]
        );
        let c = count(&f);
        assert_eq!(c.args.len(), 4);
        assert!(!c.sql.contains("LIMIT"));
    }

    #[test]
    fn default_query_lists_stocks_by_yield() {
        let q = list(&ScreeningFilter::default());
        assert!(q
            .sql
            .contains("WHERE COALESCE(s.kind, 'stock') = ? ORDER BY"));
        assert!(q
            .sql
            .contains("ORDER BY i.dividend_yield IS NULL, i.dividend_yield DESC"));
        assert_eq!(q.args[0], Arg::Text("stock".into()));
    }

    #[test]
    fn search_matches_code_prefix_or_name_substring() {
        let f = ScreeningFilter::from_pairs([("q", "72%")]).unwrap();
        let q = list(&f);
        assert!(q
            .sql
            .contains("(i.code LIKE ? ESCAPE '\\' OR s.name LIKE ? ESCAPE '\\')"));
        assert_eq!(
            q.args[1],
            Arg::Text("72\\%%".into()),
            "% はエスケープ、コードは前方一致"
        );
        assert_eq!(q.args[2], Arg::Text("%72\\%%".into()), "名前は部分一致");
    }

    #[test]
    fn master_filters_use_in_lists() {
        let f =
            ScreeningFilter::from_pairs([("sector", "銀行業,保険業"), ("size", "large")]).unwrap();
        let q = list(&f);
        assert!(q
            .sql
            .contains("s.sector33 IN (?, ?) AND s.size_class IN (?)"));
        assert_eq!(
            q.args[1..4],
            [
                Arg::Text("銀行業".into()),
                Arg::Text("保険業".into()),
                Arg::Text("large".into())
            ]
        );
    }

    #[test]
    fn portfolio_rows_use_the_list_columns() {
        assert!(
            portfolio_rows().starts_with(&format!("SELECT {LIST_COLUMNS} FROM portfolio_items"))
        );
    }

    #[test]
    fn kind_selects_the_tab() {
        let f = ScreeningFilter::from_pairs([("kind", "etf")]).unwrap();
        assert_eq!(list(&f).args[0], Arg::Text("etf".into()));
    }
}
