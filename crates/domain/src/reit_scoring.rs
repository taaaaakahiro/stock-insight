//! Deterministic scoring over a frozen J-REIT universe.
use std::collections::BTreeMap;

pub const VERSION: &str = "jreit-v1";
pub const MIN_PEERS: usize = 8;
pub const MIN_COVERAGE: u32 = 80;
pub const PRICE_MAX_AGE: i64 = 10;
pub const DISCLOSURE_MAX_AGE: i64 = 270;
pub const CATEGORIES: [(&str, &str); 7] = [
    ("distribution", "分配金成長・持続性"),
    ("profitability", "保有物件の収益力"),
    ("finance", "財務健全性・金利耐性"),
    ("nav", "不動産の含み益・NAV"),
    ("valuation", "割安度"),
    ("diversification", "資産規模・分散・質"),
    ("management", "スポンサー・運用能力"),
];

pub struct Metric {
    pub key: &'static str,
    pub category: usize,
    pub weight: u32,
    pub lower: bool,
    pub enabled: bool,
    pub unit: &'static str,
    pub label: &'static str,
    pub formula: &'static str,
}

macro_rules! metric {
    ($key:literal,$cat:literal,$weight:literal,$lower:literal,$enabled:literal,$unit:literal,$label:literal,$formula:literal) => {
        Metric {
            key: $key,
            category: $cat,
            weight: $weight,
            lower: $lower,
            enabled: $enabled,
            unit: $unit,
            label: $label,
            formula: $formula,
        }
    };
}

pub const METRICS: &[Metric] = &[
    metric!(
        "normalized_dpu_cagr",
        0,
        12,
        false,
        true,
        "ratio",
        "正常化年間DPU 5年CAGR",
        "(最新 / 5年前)^(1/5) − 1"
    ),
    metric!(
        "affo_coverage",
        0,
        8,
        false,
        true,
        "times",
        "継続分配原資カバー率",
        "正常化年間AFFO / 正常化年間分配額"
    ),
    metric!(
        "sale_dependence",
        0,
        5,
        true,
        true,
        "ratio",
        "売却益等依存率",
        "一時分配金 / 確定年間DPU"
    ),
    metric!(
        "same_noi_cagr",
        1,
        10,
        false,
        true,
        "ratio",
        "同一物件NOI 3年CAGR",
        "同じ保有物件群の (最新 / 3年前)^(1/3) − 1"
    ),
    metric!(
        "ffo_cagr",
        1,
        7,
        false,
        true,
        "ratio",
        "年間1口FFO 5年CAGR",
        "(最新 / 5年前)^(1/5) − 1"
    ),
    metric!(
        "occupancy",
        1,
        3,
        false,
        true,
        "ratio",
        "稼働率",
        "賃貸面積 / 賃貸可能面積"
    ),
    metric!(
        "ltv",
        2,
        5,
        true,
        true,
        "ratio",
        "比較可能LTV",
        "期末有利子負債 / 期末総資産"
    ),
    metric!(
        "floating_ratio",
        2,
        6,
        true,
        true,
        "ratio",
        "実質変動金利比率",
        "ヘッジ後の変動有利子負債 / 全有利子負債"
    ),
    metric!(
        "maturity_24m",
        2,
        3,
        true,
        true,
        "ratio",
        "24か月満期集中率",
        "24か月内の満期額 / 全有利子負債"
    ),
    metric!(
        "interest_cover",
        2,
        3,
        false,
        true,
        "times",
        "利払い余力",
        "調整後年間EBITDA / 年間支払利息"
    ),
    metric!(
        "stress_100",
        2,
        3,
        true,
        true,
        "ratio",
        "金利+1.0%ptの負担率",
        "有利子負債 × 実質変動金利比率 × 0.01 / 正常化年間分配額"
    ),
    metric!(
        "unrealized_equity",
        3,
        7,
        false,
        true,
        "ratio",
        "含み益 / 純資産",
        "(鑑定評価額 − 帳簿価額) / 純資産"
    ),
    metric!(
        "nav_cagr",
        3,
        5,
        false,
        true,
        "ratio",
        "1口NAV 3年CAGR",
        "(最新 / 3年前)^(1/3) − 1"
    ),
    metric!(
        "liquidity",
        3,
        3,
        false,
        false,
        "index",
        "物件売却流動性",
        "評価方法の整備待ち"
    ),
    metric!(
        "p_nav",
        4,
        6,
        true,
        true,
        "times",
        "NAV倍率",
        "同じ口数基準の価格 / 1口NAV"
    ),
    metric!(
        "p_ffo",
        4,
        4,
        true,
        true,
        "times",
        "価格 / 年間1口FFO",
        "同じ口数基準の価格 / 年間1口FFO"
    ),
    metric!(
        "noi_diversification",
        5,
        3,
        false,
        true,
        "index",
        "NOI実効分散指数",
        "1 / Σ(物件別NOI構成比²)"
    ),
    metric!(
        "noi_concentration",
        5,
        2,
        true,
        true,
        "ratio",
        "最大物件NOI依存率",
        "最大物件NOI / 全物件NOI"
    ),
    metric!(
        "sponsor",
        6,
        3,
        false,
        false,
        "index",
        "スポンサーの信用力",
        "評価方法の整備待ち"
    ),
    metric!(
        "capital_allocation",
        6,
        2,
        false,
        false,
        "index",
        "資本配分実績",
        "評価方法の整備待ち"
    ),
];

pub fn valid_value(key: &str, v: f64) -> bool {
    v.is_finite()
        && match key {
            "normalized_dpu_cagr" | "same_noi_cagr" | "ffo_cagr" | "nav_cagr" => v >= -1.0,
            "unrealized_equity" => true,
            "occupancy" | "floating_ratio" | "maturity_24m" | "sale_dependence"
            | "noi_concentration" => (0.0..=1.0).contains(&v),
            "p_nav" | "p_ffo" => v > 0.0,
            "noi_diversification" => v >= 1.0,
            _ => v >= 0.0,
        }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Scored,
    InsufficientPeers,
    Missing,
    NotRated,
}

impl Status {
    pub fn key(self) -> &'static str {
        match self {
            Self::Scored => "scored",
            Self::InsufficientPeers => "insufficient_peers",
            Self::Missing => "missing",
            Self::NotRated => "not_rated",
        }
    }
}

#[derive(Debug)]
pub struct Component {
    pub metric: &'static Metric,
    pub percentile: Option<f64>,
    pub benchmark_size: usize,
    pub status: Status,
}

impl std::fmt::Debug for Metric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.key)
    }
}

#[derive(Debug, Default)]
pub struct Category {
    pub points: f64,
    pub available: u32,
    pub maximum: u32,
}

impl Category {
    pub fn rate(&self) -> Option<f64> {
        (self.available > 0).then(|| self.points * 100.0 / f64::from(self.available))
    }
}

#[derive(Debug)]
pub struct Score {
    pub total: Option<f64>,
    pub coverage: u32,
    pub categories: [Category; 7],
    pub components: Vec<Component>,
}

pub fn score(inputs: &[BTreeMap<&str, f64>]) -> Vec<Score> {
    let benchmarks: Vec<_> = METRICS
        .iter()
        .map(|m| {
            let mut values: Vec<_> = inputs
                .iter()
                .filter_map(|row| row.get(m.key).copied())
                .filter(|&v| m.enabled && valid_value(m.key, v))
                .collect();
            values.sort_by(f64::total_cmp);
            values
        })
        .collect();
    inputs
        .iter()
        .map(|row| {
            let mut categories: [Category; 7] = Default::default();
            let components = METRICS
                .iter()
                .zip(&benchmarks)
                .map(|(metric, peers)| {
                    let category = &mut categories[metric.category];
                    category.maximum += metric.weight;
                    let (percentile, status) = match row.get(metric.key).copied() {
                        _ if !metric.enabled => (None, Status::NotRated),
                        Some(v) if valid_value(metric.key, v) && peers.len() >= MIN_PEERS => {
                            let lo = peers.partition_point(|&p| p < v);
                            let hi = peers.partition_point(|&p| p <= v);
                            let rank = (lo + hi - 1) as f64 / 2.0;
                            let p = rank * 100.0 / (peers.len() - 1) as f64;
                            let p = if metric.lower { 100.0 - p } else { p };
                            category.points += p * f64::from(metric.weight) / 100.0;
                            category.available += metric.weight;
                            (Some(p), Status::Scored)
                        }
                        Some(v) if valid_value(metric.key, v) => (None, Status::InsufficientPeers),
                        _ => (None, Status::Missing),
                    };
                    Component {
                        metric,
                        percentile,
                        status,
                        benchmark_size: peers.len(),
                    }
                })
                .collect();
            let coverage = categories.iter().map(|c| c.available).sum();
            let total = (coverage > 0).then(|| {
                categories.iter().map(|c| c.points).sum::<f64>() * 100.0 / f64::from(coverage)
            });
            Score {
                total,
                coverage,
                categories,
                components,
            }
        })
        .collect()
}

pub fn rank_cmp(a: (f64, u32, &str), b: (f64, u32, &str)) -> std::cmp::Ordering {
    b.0.total_cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(b.2))
}

pub fn comparison_codes(raw: &str) -> Option<Vec<String>> {
    let mut codes = Vec::new();
    for part in raw.split(',') {
        if codes.len() == 5 || part.len() != 4 {
            return None;
        }
        let code = crate::StockCode::parse(part)?.as_str().to_owned();
        if codes.contains(&code) {
            return None;
        }
        codes.push(code);
    }
    Some(codes)
}

pub fn ratio(n: f64, d: f64) -> Option<f64> {
    let v = n / d;
    (d.is_finite() && d > 0.0 && v.is_finite()).then_some(v)
}

pub fn cagr(end: f64, start: f64, years: i32) -> Option<f64> {
    (end >= 0.0 && years > 0).then_some(())?;
    Some(ratio(end, start)?.powf(1.0 / f64::from(years)) - 1.0)
}

pub fn interest_stress(debt: f64, floating: f64, shock: f64, payout: f64) -> Option<f64> {
    (debt >= 0.0 && (0.0..=1.0).contains(&floating) && shock >= 0.0).then_some(())?;
    ratio(debt * floating * shock, payout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_zero_missing_and_minimum_peer_count() {
        assert_eq!(METRICS.iter().map(|m| m.weight).sum::<u32>(), 100);
        let mut inputs: Vec<_> = (0..8)
            .map(|i| {
                BTreeMap::from([
                    ("occupancy", f64::from(i / 2) / 10.0),
                    ("ltv", f64::from(i) / 10.0),
                ])
            })
            .collect();
        let scores = score(&inputs);
        assert_eq!(scores[0].coverage, 8);
        assert_eq!(
            scores[0].components[5].percentile,
            scores[1].components[5].percentile
        );
        assert_eq!(scores[0].components[6].percentile, Some(100.0));
        assert_eq!(scores[7].components[6].percentile, Some(0.0));
        assert_eq!(scores[0].categories[2].points, 5.0);
        assert_eq!(scores[0].categories[2].maximum, 20);
        inputs.push(BTreeMap::new());
        assert_eq!(score(&inputs)[8].total, None);
        let below = score(&inputs[..7]);
        assert_eq!(below[0].total, None);
        assert_eq!(below[0].components[5].status, Status::InsufficientPeers);
    }

    #[test]
    fn compares_up_to_five_distinct_codes() {
        assert_eq!(
            comparison_codes("8951,8952,8953,8954,8955").unwrap().len(),
            5
        );
        assert!(comparison_codes("8951,8952,8953,8954,8955,8956").is_none());
        assert!(comparison_codes("8951,8951").is_none());
        assert!(comparison_codes("").is_none());
        assert!(comparison_codes("8951,unknown").is_none());
    }

    #[test]
    fn stable_unrounded_order_and_floating_stress() {
        assert!(rank_cmp((90.04, 80, "9999"), (90.0, 92, "1000")).is_lt());
        assert!(rank_cmp((90.0, 92, "9999"), (90.0, 80, "1000")).is_lt());
        assert!(rank_cmp((90.0, 92, "1000"), (90.0, 92, "9999")).is_lt());
        assert_eq!(interest_stress(1_000_000.0, 0.0, 0.01, 50_000.0), Some(0.0));
        assert_eq!(
            interest_stress(1_000_000.0, 0.4, 0.01, 50_000.0),
            Some(0.08)
        );
        assert_eq!(interest_stress(1_000_000.0, 0.4, 0.01, 0.0), None);
    }
}
