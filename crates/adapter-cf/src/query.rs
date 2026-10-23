//! スクリーニング一覧の SQL を組み立てる。列名は domain の固定の列挙から取り、値は必ずプレースホルダで渡す。
//! 指標（indicators_latest）に銘柄マスタ（stocks）をつないで、名称・種類・業種・規模区分も返す。

use domain::screening::{Metric, ScreeningFilter, SortKey, SortOrder, PAGE_SIZE};

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

/// REIT の有利子負債の合計（金利の内訳の5つ＋投資法人債）。0 なら NULL にして割り算を避ける
const REIT_DEBT: &str =
    "NULLIF(r.rate_fixed + r.rate_swap_fixed + r.rate_swap_amount + r.rate_floating \
     + r.rate_unknown + r.bonds, 0)";

/// 有報・半期報告書に書かれた DOE の下限・目標。両方の書類にあれば新しいほう
macro_rules! doe_floor_sql {
    () => {
        "(SELECT e.doe_floor FROM edinet_docs e WHERE e.code = i.code AND e.doe_floor IS NOT NULL \
         ORDER BY e.submitted DESC LIMIT 1)"
    };
}
macro_rules! doe_goal_sql {
    () => {
        "(SELECT e.doe_goal FROM edinet_docs e WHERE e.code = i.code AND e.doe_goal IS NOT NULL \
         ORDER BY e.submitted DESC LIMIT 1)"
    };
}
/// 配当性向の下限・目標も同じく、書かれた新しい書類の値
macro_rules! payout_floor_sql {
    () => {
        "(SELECT e.payout_floor FROM edinet_docs e WHERE e.code = i.code AND e.payout_floor IS NOT NULL \
         ORDER BY e.submitted DESC LIMIT 1)"
    };
}
macro_rules! payout_goal_sql {
    () => {
        "(SELECT e.payout_goal FROM edinet_docs e WHERE e.code = i.code AND e.payout_goal IS NOT NULL \
         ORDER BY e.submitted DESC LIMIT 1)"
    };
}
/// 比べる DOE: 会社予想の配当があれば 利回り × PBR（予想の配当 ÷ BPS）、なければ実績の DOE
const DOE_NOW: &str =
    "(CASE WHEN i.dividend_basis = 'forecast' THEN i.dividend_yield * i.pbr ELSE i.doe END)";

/// 指標の SQL の式。株式などは indicators_latest の列、REIT は reit_profile から計算する（比率は小数）
pub fn metric_sql(m: Metric) -> String {
    match m {
        Metric::FloatingRatio => format!("(r.rate_floating * 1.0 / {REIT_DEBT})"),
        Metric::UnknownRatio => format!("(r.rate_unknown * 1.0 / {REIT_DEBT})"),
        Metric::RepriceRatio => format!("(r.reprice_1y * 1.0 / {REIT_DEBT})"),
        Metric::RepriceMaxRatio => format!("(r.reprice_1y_max * 1.0 / {REIT_DEBT})"),
        Metric::AvgRate => "(r.avg_rate / 100.0)".into(),
        Metric::Occupancy => "(r.occupancy / 100.0)".into(),
        Metric::DoeFloor => doe_floor_sql!().into(),
        Metric::DoeMargin => format!("({DOE_NOW} - {})", doe_floor_sql!()),
        _ => format!("i.{}", m.key()),
    }
}

/// REIT の列（一覧・ポートフォリオで返す）
fn reit_columns() -> String {
    let m = |x: Metric| format!("{} AS {}", metric_sql(x), x.key());
    format!(
        "r.sector AS reit_sector, {}, {}, {}, {}, {}, {}, r.period_end AS reit_period_end",
        m(Metric::FloatingRatio),
        m(Metric::UnknownRatio),
        m(Metric::RepriceRatio),
        m(Metric::RepriceMaxRatio),
        m(Metric::AvgRate),
        m(Metric::Occupancy)
    )
}

pub const LIST_COLUMNS: &str = concat!(
    "i.code, i.price, i.price_date, i.dividend_ttm, i.dividend_basis, i.dividend_yield, i.yield_percentile, i.pbr, i.per, \
     i.payout_ratio, i.doe, i.consecutive_increases, i.operating_margin, i.no_loss_years, i.profit_uptrend, \
     i.valuation, i.fetched_at, s.name, COALESCE(s.kind, 'stock') AS kind, s.market, s.sector33, s.size_class, \
     i.dividend_every_year, i.no_quarterly_loss, i.is_value, i.data_years, i.sales_growth, i.op_growth, i.dividend_cut, \
     i.op_growth_q, i.net_growth_q, i.op_growth_ttm, i.profit_one_off, \
     (SELECT MAX(e.progressive_dividend) FROM edinet_docs e WHERE e.code = i.code) AS progressive_dividend, \
     (SELECT MAX(e.no_dividend_cut) FROM edinet_docs e WHERE e.code = i.code) AS no_dividend_cut, ",
    doe_floor_sql!(),
    " AS doe_floor, ",
    doe_goal_sql!(),
    " AS doe_goal, ",
    payout_floor_sql!(),
    " AS payout_floor, ",
    payout_goal_sql!(),
    " AS payout_goal"
);

/// マスタにまだない銘柄も消えないよう LEFT JOIN（種類は株式として扱う）。REIT の要約も LEFT JOIN
const FROM: &str = "FROM indicators_latest i LEFT JOIN stocks s ON s.code = i.code \
     LEFT JOIN reit_profile r ON r.code = i.code";

fn where_clause(f: &ScreeningFilter) -> (String, Vec<Arg>) {
    let mut conds = vec!["COALESCE(s.kind, 'stock') = ?".to_string()];
    let mut args = vec![Arg::Text(f.kind.as_str().to_string())];
    for r in &f.ranges {
        let col = metric_sql(r.metric);
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
        conds.push("i.valuation = ?".to_string());
        args.push(Arg::Text(v.as_str().to_string()));
    }
    for (col, values) in [
        ("s.sector33", &f.sectors),
        ("s.market", &f.markets),
        ("s.size_class", &f.sizes),
        ("r.sector", &f.reit_sectors),
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
    if f.dividend_policy {
        conds.push(
            "EXISTS (SELECT 1 FROM edinet_docs e WHERE e.code = i.code \
             AND (e.progressive_dividend = 1 OR e.no_dividend_cut = 1))"
                .to_string(),
        );
    }
    (format!(" WHERE {}", conds.join(" AND ")), args)
}

/// 1ページ分の行。条件に合う件数も同じスキャンで total 列に入れる（件数のために全件を読み直さない）
pub fn list(f: &ScreeningFilter) -> Query {
    let (w, mut args) = where_clause(f);
    let col = match f.sort {
        SortKey::Code => "i.code".to_string(),
        SortKey::Metric(m) => metric_sql(m),
    };
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
            "SELECT {LIST_COLUMNS}, {}, COUNT(*) OVER () AS total {FROM}{w} \
             ORDER BY {nulls}{col} {dir}, i.code ASC LIMIT ? OFFSET ?",
            reit_columns()
        ),
        args,
    }
}

/// 条件に合う件数（最後のページより先を開いて list が0行だったときだけ使う）
pub fn count(f: &ScreeningFilter) -> Query {
    let (w, args) = where_clause(f);
    Query {
        sql: format!("SELECT COUNT(*) AS n {FROM}{w}"),
        args,
    }
}

/// 最新の本決算の EPS。期末のほかに数字が1つもない行（配当予想の修正が残ったもの）は飛ばす
/// （`AnnualFinancial::has_no_figures` と同じ条件）
const LATEST_EPS: &str = "(SELECT f.eps FROM financials_annual f WHERE f.code = p.code \
     AND COALESCE(f.net_sales, f.operating_income, f.ordinary_income, f.net_income, f.equity, \
     f.eps, f.bps, f.dps, f.operating_cf) IS NOT NULL ORDER BY f.period_end DESC LIMIT 1) AS latest_eps";

/// ポートフォリオに登録した銘柄の最新指標と、売りサイン用の最新 EPS（? はユーザーID）
pub fn portfolio_rows() -> String {
    format!(
        "SELECT {LIST_COLUMNS}, {}, {LATEST_EPS} FROM portfolio_items p JOIN indicators_latest i ON i.code = p.code \
         LEFT JOIN stocks s ON s.code = p.code LEFT JOIN reit_profile r ON r.code = p.code WHERE p.user_id = ?",
        reit_columns()
    )
}

/// 1銘柄の EDINET の書類と配当方針の手がかり（? は銘柄コード）
pub const EDINET_DOCS: &str =
    "SELECT doc_kind, doc_id, submitted, period_end, progressive_dividend, \
     no_dividend_cut, dividend_raise, doe_target, payout_target, doe_floor, doe_goal, doe_phrase, \
     payout_floor, payout_goal, payout_phrase, phrases \
     FROM edinet_docs WHERE code = ? ORDER BY doc_kind";

pub const PORTFOLIO_CODES: &str =
    "SELECT code FROM portfolio_items WHERE user_id = ? ORDER BY added_at, code";
pub const PORTFOLIO_ADD: &str =
    "INSERT INTO portfolio_items (user_id, code, added_at) VALUES (?, ?, ?) ON CONFLICT DO NOTHING";
pub const PORTFOLIO_REMOVE: &str = "DELETE FROM portfolio_items WHERE user_id = ? AND code = ?";

/// 詳細ページ用: 1銘柄の最新の指標（一覧と同じ列）
pub fn latest_by_code(code: &str) -> Query {
    Query {
        sql: format!(
            "SELECT {LIST_COLUMNS}, {} {FROM} WHERE i.code = ?",
            reit_columns()
        ),
        args: vec![Arg::Text(code.to_string())],
    }
}

pub const PRICE_HISTORY_BY_CODE: &str = "SELECT closes FROM price_history WHERE code = ?";

/// 1銘柄の株式分割・併合（日付の古い順）
pub const SPLITS_BY_CODE: &str = "SELECT date, factor FROM splits WHERE code = ? ORDER BY date";

pub const FORECASTS_BY_CODE: &str =
    "SELECT doc_id, disclosed, fiscal_year_end, dps FROM dividend_forecasts WHERE code = ? ORDER BY disclosed";

pub const ANNUALS_BY_CODE: &str =
    "SELECT period_end, doc_id, disclosed, net_sales, operating_income, \
     ordinary_income, net_income, equity, eps, bps, dps, operating_cf \
     FROM financials_annual WHERE code = ? ORDER BY period_end DESC LIMIT 10";

pub const QUARTERLIES_BY_CODE: &str =
    "SELECT fiscal_year_end, period_type, period_end, net_sales, \
     operating_income, ordinary_income, net_income \
     FROM financials_quarterly WHERE code = ? ORDER BY period_end DESC LIMIT 40";

pub const LATEST_RUN: &str = "SELECT r.run_id, r.started_at, r.finished_at, r.status, \
     (SELECT MAX(price_date) FROM indicators_latest) AS latest_price_date, \
     (SELECT MAX(disclosed) FROM (SELECT disclosed FROM financials_annual \
      UNION ALL SELECT disclosed FROM financials_quarterly)) AS latest_fin_date \
     FROM batch_runs r ORDER BY r.started_at DESC LIMIT 1";

/// AIレコメンドの候補（株式で、バリュー株か「5年連続配当かつ通期黒字」）。最終の絞り込みは domain で行う
pub fn recommend_candidates() -> String {
    format!(
        "SELECT {LIST_COLUMNS} {FROM} WHERE COALESCE(s.kind, 'stock') = 'stock' \
         AND i.dividend_yield > 0 \
         AND (i.is_value = 1 OR (i.dividend_every_year = 1 AND i.no_loss_years = 1)) \
         ORDER BY i.code"
    )
}

/// 最新日のレコメンド（順位順）に、銘柄名と最新の指標をつける
pub fn recommendation_rows() -> String {
    format!(
        "SELECT r.date, r.rank, r.reason, r.model, r.created_at, {LIST_COLUMNS} \
         FROM recommendations r JOIN indicators_latest i ON i.code = r.code \
         LEFT JOIN stocks s ON s.code = r.code \
         WHERE r.date = (SELECT MAX(date) FROM recommendations) ORDER BY r.rank"
    )
}

pub const RECOMMENDATION_EXISTS: &str = "SELECT COUNT(*) AS n FROM recommendations WHERE date = ?";
pub const RECOMMENDATION_INSERT: &str = "INSERT INTO recommendations \
     (date, rank, code, reason, model, created_at) VALUES (?, ?, ?, ?, ?, ?)";

/// 1銘柄の REIT の要約と明細（? は銘柄コード）
pub const REIT_DETAIL: &str = "SELECT p.doc_id, p.submitted, p.period_end, p.sector, p.short_loans, p.long_loans, \
     p.bonds, p.rate_fixed, p.rate_swap_fixed, p.rate_swap_amount, p.rate_floating, p.rate_unknown, p.swap_notional, \
     p.avg_rate, p.avg_years, p.reprice_1y, p.reprice_1y_max, p.property_count, p.leasable_m2, p.leased_m2, \
     p.occupancy, p.checks, d.body FROM reit_profile p LEFT JOIN reit_detail d ON d.code = p.code WHERE p.code = ?";

/// 1銘柄の REIT の決算ごとの要約（? は銘柄コード）
pub const REIT_HISTORY: &str = "SELECT period_end, loans_total, bonds, rate_floating, rate_unknown, swap_notional, \
     avg_rate, property_count, leasable_m2, occupancy FROM reit_history WHERE code = ? ORDER BY period_end";

/// 一覧・配当方針・状態の中身が変わったかの目印（サーバー側キャッシュのキー）。
/// 最新のバッチ（rowid は追加順）と、EDINET・REIT の取り込みを最後に終えた時刻。数行しか読まない
pub const DATA_VERSION: &str = "SELECT r.run_id, r.started_at, r.finished_at, \
     (SELECT updated_at FROM sync_state WHERE key = 'edinet_listed_until') AS edinet_at, \
     (SELECT updated_at FROM sync_state WHERE key = 'reit_listed_until') AS reit_at \
     FROM batch_runs r ORDER BY r.rowid DESC LIMIT 1";

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
        assert!(q.sql.contains(", COUNT(*) OVER () AS total FROM"));
        assert!(q.sql.ends_with(
            "FROM indicators_latest i LEFT JOIN stocks s ON s.code = i.code \
             LEFT JOIN reit_profile r ON r.code = i.code \
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
        assert!(portfolio_rows().starts_with(&format!(
            "SELECT {LIST_COLUMNS}, {}, {LATEST_EPS} FROM portfolio_items",
            reit_columns()
        )));
    }

    #[test]
    fn doe_columns_use_the_newest_document_and_forecast() {
        assert!(LIST_COLUMNS.ends_with(&format!(
            "{} AS doe_floor, {} AS doe_goal, {} AS payout_floor, {} AS payout_goal",
            doe_floor_sql!(),
            doe_goal_sql!(),
            payout_floor_sql!(),
            payout_goal_sql!()
        )));
        assert!(doe_floor_sql!().contains("ORDER BY e.submitted DESC LIMIT 1"));
        let margin = metric_sql(Metric::DoeMargin);
        assert!(margin.starts_with(
            "((CASE WHEN i.dividend_basis = 'forecast' THEN i.dividend_yield * i.pbr"
        ));
        assert!(margin.ends_with(&format!(" - {})", doe_floor_sql!())));
        let f = ScreeningFilter {
            sort: SortKey::Metric(Metric::DoeMargin),
            ..Default::default()
        };
        assert!(list(&f)
            .sql
            .contains(&format!("ORDER BY {margin} IS NULL, {margin}")));
    }

    #[test]
    fn detail_query_selects_one_code() {
        let q = latest_by_code("7203");
        assert!(q
            .sql
            .ends_with("LEFT JOIN reit_profile r ON r.code = i.code WHERE i.code = ?"));
        assert_eq!(q.args, vec![Arg::Text("7203".into())]);
    }

    #[test]
    fn dividend_policy_filter_uses_edinet_docs() {
        let f = ScreeningFilter::from_pairs([("dividend_policy", "1"), ("sort", "sales_growth")])
            .unwrap();
        let q = list(&f);
        assert!(q.sql.contains(
            "EXISTS (SELECT 1 FROM edinet_docs e WHERE e.code = i.code \
             AND (e.progressive_dividend = 1 OR e.no_dividend_cut = 1))"
        ));
        assert!(q
            .sql
            .contains("ORDER BY i.sales_growth IS NULL, i.sales_growth DESC"));
        assert_eq!(q.args.len(), 3, "kind と LIMIT・OFFSET だけ");
    }

    #[test]
    fn kind_selects_the_tab() {
        let f = ScreeningFilter::from_pairs([("kind", "etf")]).unwrap();
        assert_eq!(list(&f).args[0], Arg::Text("etf".into()));
    }

    #[test]
    fn reit_metrics_sort_and_filter_on_reit_profile() {
        let f = ScreeningFilter::from_pairs([
            ("kind", "reit"),
            ("reit_sector", "logistics,hotel"),
            ("floating_ratio_max", "0.2"),
            ("sort", "reprice_max_ratio"),
            ("order", "asc"),
        ])
        .unwrap();
        let q = list(&f);
        assert!(q
            .sql
            .contains("LEFT JOIN reit_profile r ON r.code = i.code"));
        assert!(q.sql.contains("r.sector IN (?, ?)"));
        assert!(q.sql.contains("(r.rate_floating * 1.0 / NULLIF("));
        assert!(q.sql.contains("<= ?"));
        assert!(
            q.sql.contains("ORDER BY (r.reprice_1y_max * 1.0 / NULLIF("),
            "{}",
            q.sql
        );
        assert!(q.sql.contains("AS reprice_max_ratio"));
        assert!(q.args.contains(&Arg::Text("hotel".into())));
        assert!(q.args.contains(&Arg::Real(0.2)));
        assert!(ScreeningFilter::from_pairs([("reit_sector", "casino")]).is_err());
    }

    #[test]
    fn recommendation_queries_use_the_list_columns() {
        assert!(recommend_candidates()
            .contains(&format!("SELECT {LIST_COLUMNS} FROM indicators_latest")));
        assert!(recommend_candidates().contains("COALESCE(s.kind, 'stock') = 'stock'"));
        assert!(recommendation_rows().contains(LIST_COLUMNS));
        assert!(recommendation_rows().ends_with("ORDER BY r.rank"));
    }
}
