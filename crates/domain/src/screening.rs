//! スクリーニング一覧（F2）の絞り込み条件。値の検証だけを行い、SQLは知らない。

/// 範囲で絞り込める指標（列名は indicators_latest に対応）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    Price,
    DividendYield,
    YieldPercentile,
    Pbr,
    Per,
    PayoutRatio,
    Doe,
    ConsecutiveIncreases,
    OperatingMargin,
    /// 売上高・営業利益の前年同期比
    SalesGrowth,
    OpGrowth,
}

impl Metric {
    pub const ALL: [Metric; 11] = [
        Metric::Price,
        Metric::DividendYield,
        Metric::YieldPercentile,
        Metric::Pbr,
        Metric::Per,
        Metric::PayoutRatio,
        Metric::Doe,
        Metric::ConsecutiveIncreases,
        Metric::OperatingMargin,
        Metric::SalesGrowth,
        Metric::OpGrowth,
    ];

    /// APIのクエリ名・D1の列名
    pub fn key(self) -> &'static str {
        match self {
            Metric::Price => "price",
            Metric::DividendYield => "dividend_yield",
            Metric::YieldPercentile => "yield_percentile",
            Metric::Pbr => "pbr",
            Metric::Per => "per",
            Metric::PayoutRatio => "payout_ratio",
            Metric::Doe => "doe",
            Metric::ConsecutiveIncreases => "consecutive_increases",
            Metric::OperatingMargin => "operating_margin",
            Metric::SalesGrowth => "sales_growth",
            Metric::OpGrowth => "op_growth",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub metric: Metric,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

/// 並べ替えのキー。指標の他に銘柄コード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Code,
    Metric(Metric),
}

impl SortKey {
    pub fn key(self) -> &'static str {
        match self {
            SortKey::Code => "code",
            SortKey::Metric(m) => m.key(),
        }
    }
}

pub const PAGE_SIZE: u32 = 50;
const MAX_PAGE: u32 = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct ScreeningFilter {
    /// 一覧のタブ（株式 / REIT / ETF）
    pub kind: crate::stock_master::AssetKind,
    /// コードまたは銘柄名の部分一致（半角に正規化済み、最大50文字）
    pub query: Option<String>,
    /// 33業種（複数選択。空なら絞らない）
    pub sectors: Vec<String>,
    /// 市場区分（プライム / スタンダード / グロース）
    pub markets: Vec<String>,
    /// 規模区分（large / mid / small / none）
    pub sizes: Vec<String>,
    pub ranges: Vec<Range>,
    /// 割安などのバッジで絞る（Valuation::as_str の値）
    pub valuation: Option<crate::Valuation>,
    pub no_loss_years: bool,
    pub profit_uptrend: bool,
    pub is_value: bool,
    pub dividend_every_year: bool,
    pub no_quarterly_loss: bool,
    /// 有報などに累進配当・非減配の記述がある（EDINET）
    pub dividend_policy: bool,
    pub sort: SortKey,
    pub order: SortOrder,
    /// 1始まり
    pub page: u32,
}

impl Default for ScreeningFilter {
    fn default() -> Self {
        Self {
            kind: crate::stock_master::AssetKind::Stock,
            query: None,
            sectors: vec![],
            markets: vec![],
            sizes: vec![],
            ranges: vec![],
            valuation: None,
            no_loss_years: false,
            profit_uptrend: false,
            is_value: false,
            dividend_every_year: false,
            no_quarterly_loss: false,
            dividend_policy: false,
            sort: SortKey::Metric(Metric::DividendYield),
            order: SortOrder::Desc,
            page: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterError(pub String);

impl ScreeningFilter {
    /// クエリ文字列の組（キー, 値）から組み立てる。
    /// 範囲は `<指標>_min` / `<指標>_max`。未知のキーは無視する。
    pub fn from_pairs<'a>(
        pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, FilterError> {
        let mut f = Self::default();
        for (k, v) in pairs {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            match k {
                "sort" => {
                    f.sort = if v == "code" {
                        SortKey::Code
                    } else {
                        SortKey::Metric(
                            Metric::from_key(v)
                                .ok_or_else(|| FilterError(format!("unknown sort: {v}")))?,
                        )
                    }
                }
                "order" => {
                    f.order = match v {
                        "asc" => SortOrder::Asc,
                        "desc" => SortOrder::Desc,
                        _ => return Err(FilterError(format!("bad order: {v}"))),
                    }
                }
                "page" => {
                    f.page = v
                        .parse()
                        .ok()
                        .filter(|p| (1..=MAX_PAGE).contains(p))
                        .ok_or_else(|| FilterError(format!("bad page: {v}")))?
                }
                "valuation" => {
                    f.valuation = Some(match v {
                        "cheap" => crate::Valuation::Cheap,
                        "neutral" => crate::Valuation::Neutral,
                        "expensive" => crate::Valuation::Expensive,
                        _ => return Err(FilterError(format!("bad valuation: {v}"))),
                    })
                }
                "q" => {
                    let q = crate::stock_master::halfwidth_ascii(v);
                    let q: String = q.chars().take(50).collect();
                    f.query = Some(q.trim().to_string()).filter(|s| !s.is_empty());
                }
                "sector" => f.sectors = parse_list(v, 40, None)?,
                "market" => {
                    f.markets = parse_list(v, 3, Some(&["プライム", "スタンダード", "グロース"]))?
                }
                "size" => f.sizes = parse_list(v, 4, Some(&["large", "mid", "small", "none"]))?,
                "kind" => {
                    f.kind = crate::stock_master::AssetKind::from_key(v)
                        .ok_or_else(|| FilterError(format!("bad kind: {v}")))?
                }
                "no_loss_years" => f.no_loss_years = parse_flag(v)?,
                "profit_uptrend" => f.profit_uptrend = parse_flag(v)?,
                "is_value" => f.is_value = parse_flag(v)?,
                "dividend_every_year" => f.dividend_every_year = parse_flag(v)?,
                "no_quarterly_loss" => f.no_quarterly_loss = parse_flag(v)?,
                "dividend_policy" => f.dividend_policy = parse_flag(v)?,
                _ => {
                    let Some((name, bound)) = k.rsplit_once('_') else {
                        continue;
                    };
                    let Some(metric) = Metric::from_key(name) else {
                        continue;
                    };
                    let x: f64 = v
                        .parse()
                        .ok()
                        .filter(|x: &f64| x.is_finite())
                        .ok_or_else(|| FilterError(format!("bad number for {k}: {v}")))?;
                    let r = f.range_mut(metric);
                    match bound {
                        "min" => r.min = Some(x),
                        "max" => r.max = Some(x),
                        _ => continue,
                    }
                }
            }
        }
        for r in &f.ranges {
            if let (Some(a), Some(b)) = (r.min, r.max) {
                if a > b {
                    return Err(FilterError(format!("{}: min > max", r.metric.key())));
                }
            }
        }
        Ok(f)
    }

    fn range_mut(&mut self, metric: Metric) -> &mut Range {
        let i = match self.ranges.iter().position(|r| r.metric == metric) {
            Some(i) => i,
            None => {
                self.ranges.push(Range {
                    metric,
                    min: None,
                    max: None,
                });
                self.ranges.len() - 1
            }
        };
        &mut self.ranges[i]
    }

    pub fn offset(&self) -> u32 {
        (self.page - 1) * PAGE_SIZE
    }
}

/// カンマ区切りの選択肢。`allowed` があればその中の値だけ、なければ長さだけを見る
fn parse_list(
    v: &str,
    max_items: usize,
    allowed: Option<&[&str]>,
) -> Result<Vec<String>, FilterError> {
    let mut out: Vec<String> = vec![];
    for item in v.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if item.chars().count() > 30 {
            return Err(FilterError(format!("value too long: {item}")));
        }
        if allowed.is_some_and(|a| !a.contains(&item)) {
            return Err(FilterError(format!("bad value: {item}")));
        }
        if !out.iter().any(|x| x == item) {
            out.push(item.to_string());
        }
    }
    if out.len() > max_items {
        return Err(FilterError("too many values".into()));
    }
    Ok(out)
}

fn parse_flag(v: &str) -> Result<bool, FilterError> {
    match v {
        "1" | "true" => Ok(true),
        "0" | "false" => Ok(false),
        _ => Err(FilterError(format!("bad flag: {v}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges_and_flags() {
        let f = ScreeningFilter::from_pairs([
            ("dividend_yield_min", "0.03"),
            ("pbr_max", "1.2"),
            ("dividend_yield_max", "0.08"),
            ("no_loss_years", "1"),
            ("valuation", "cheap"),
            ("sort", "pbr"),
            ("order", "asc"),
            ("page", "2"),
            ("kind", "reit"),
            ("q", " ＮＴＴ "),
            ("sector", "銀行業,保険業, 銀行業"),
            ("market", "プライム"),
            ("size", "large,mid"),
            ("sales_growth_min", "0.1"),
            ("dividend_policy", "1"),
            ("unknown", "x"),
        ])
        .unwrap();
        assert_eq!(f.ranges.len(), 3);
        assert_eq!(f.ranges[2].metric, Metric::SalesGrowth);
        assert!(f.dividend_policy);
        assert_eq!(
            f.ranges[0],
            Range {
                metric: Metric::DividendYield,
                min: Some(0.03),
                max: Some(0.08)
            }
        );
        assert!(f.no_loss_years && !f.profit_uptrend);
        assert_eq!(f.valuation, Some(crate::Valuation::Cheap));
        assert_eq!(f.sort, SortKey::Metric(Metric::Pbr));
        assert_eq!(f.order, SortOrder::Asc);
        assert_eq!(f.offset(), 50);
        assert_eq!(f.kind, crate::stock_master::AssetKind::Reit);
        assert_eq!(
            f.query.as_deref(),
            Some("NTT"),
            "全角英数字は半角にして前後の空白を除く"
        );
        assert_eq!(f.sectors, vec!["銀行業", "保険業"], "重複は除く");
        assert_eq!(f.markets, vec!["プライム"]);
        assert_eq!(f.sizes, vec!["large", "mid"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(ScreeningFilter::from_pairs([("pbr_min", "abc")]).is_err());
        assert!(ScreeningFilter::from_pairs([("pbr_min", "2"), ("pbr_max", "1")]).is_err());
        assert!(ScreeningFilter::from_pairs([("sort", "name; DROP TABLE x")]).is_err());
        assert!(ScreeningFilter::from_pairs([("page", "0")]).is_err());
        assert!(ScreeningFilter::from_pairs([("kind", "fund")]).is_err());
        assert!(ScreeningFilter::from_pairs([("size", "huge")]).is_err());
        assert!(ScreeningFilter::from_pairs([("market", "ナスダック")]).is_err());
        assert!(ScreeningFilter::from_pairs([("pbr_min", "NaN")]).is_err());
    }

    #[test]
    fn empty_values_are_ignored() {
        let f = ScreeningFilter::from_pairs([("pbr_min", ""), ("page", " ")]).unwrap();
        assert_eq!(f, ScreeningFilter::default());
    }
}
