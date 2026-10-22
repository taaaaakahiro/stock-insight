//! 5年分の財務から、配当の継続・増配・赤字の有無を判定する（一覧のバッジと詳細ページの元データ）。

use crate::Date;

/// 決算期間の種類。J-Quants の CurPerType（1Q/2Q/3Q/FY）。4Q・5Q（変則決算）は扱わない
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PeriodType {
    Q1,
    Q2,
    Q3,
    FY,
}

impl PeriodType {
    pub fn from_jquants(s: &str) -> Option<Self> {
        Some(match s.trim() {
            "1Q" => PeriodType::Q1,
            "2Q" => PeriodType::Q2,
            "3Q" => PeriodType::Q3,
            "FY" => PeriodType::FY,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PeriodType::Q1 => "1Q",
            PeriodType::Q2 => "2Q",
            PeriodType::Q3 => "3Q",
            PeriodType::FY => "FY",
        }
    }

    fn quarter(self) -> u8 {
        match self {
            PeriodType::Q1 => 1,
            PeriodType::Q2 => 2,
            PeriodType::Q3 => 3,
            PeriodType::FY => 4,
        }
    }
}

/// 決算短信の1行（期首からの累計値）
#[derive(Debug, Clone, PartialEq)]
pub struct CumulativePeriod {
    pub fiscal_year_end: Date,
    pub period_end: Date,
    pub period_type: PeriodType,
    pub net_sales: Option<f64>,
    pub operating_income: Option<f64>,
    pub ordinary_income: Option<f64>,
    pub net_income: Option<f64>,
}

/// 四半期単独の損益（累計の差分）
#[derive(Debug, Clone, PartialEq)]
pub struct QuarterResult {
    pub fiscal_year_end: Date,
    pub quarter: u8,
    pub period_end: Date,
    pub net_sales: Option<f64>,
    pub operating_income: Option<f64>,
    pub ordinary_income: Option<f64>,
    pub net_income: Option<f64>,
}

fn diff(cur: Option<f64>, prev: Option<f64>) -> Option<f64> {
    Some(cur? - prev?)
}

/// 累計値から四半期単独の値を出す。前の四半期の行がなければその項目は不明（None）。
/// 結果は期末日の古い順
pub fn standalone_quarters(periods: &[CumulativePeriod]) -> Vec<QuarterResult> {
    let mut sorted: Vec<&CumulativePeriod> = periods.iter().collect();
    sorted.sort_by_key(|p| (p.fiscal_year_end, p.period_type));
    sorted.dedup_by_key(|p| (p.fiscal_year_end, p.period_type));
    let mut out = vec![];
    for (i, p) in sorted.iter().enumerate() {
        let prev = match p.period_type {
            PeriodType::Q1 => None,
            _ => sorted[..i]
                .iter()
                .rev()
                .find(|q| q.fiscal_year_end == p.fiscal_year_end)
                .copied(),
        };
        let (sales, op, ord, net) = match (p.period_type, prev) {
            (PeriodType::Q1, _) => (
                p.net_sales,
                p.operating_income,
                p.ordinary_income,
                p.net_income,
            ),
            // 直前の四半期の累計が同じ年度にある場合だけ差分が取れる
            (t, Some(q)) if q.period_type.quarter() + 1 == t.quarter() => (
                diff(p.net_sales, q.net_sales),
                diff(p.operating_income, q.operating_income),
                diff(p.ordinary_income, q.ordinary_income),
                diff(p.net_income, q.net_income),
            ),
            _ => (None, None, None, None),
        };
        out.push(QuarterResult {
            fiscal_year_end: p.fiscal_year_end,
            quarter: p.period_type.quarter(),
            period_end: p.period_end,
            net_sales: sales,
            operating_income: op,
            ordinary_income: ord,
            net_income: net,
        });
    }
    out
}

/// 直近 `last_n` 四半期に赤字（純利益または営業利益がマイナス）がないか。
/// 判定できる四半期が `min_known` 未満なら None
pub fn no_quarterly_loss(
    quarters: &[QuarterResult],
    last_n: usize,
    min_known: usize,
) -> Option<bool> {
    let recent: Vec<&QuarterResult> = quarters.iter().rev().take(last_n).collect();
    let known: Vec<&&QuarterResult> = recent
        .iter()
        .filter(|q| q.net_income.is_some() || q.operating_income.is_some())
        .collect();
    if known.len() < min_known {
        return None;
    }
    Some(known.iter().all(|q| {
        q.net_income.is_none_or(|v| v >= 0.0) && q.operating_income.is_none_or(|v| v >= 0.0)
    }))
}

/// 直近 `years` 期すべてで配当（1株配当 > 0）があるか。期数が足りなければ None。`dps` は新しい期が先頭
pub fn dividend_every_year(dps: &[Option<f64>], years: usize) -> Option<bool> {
    if dps.len() < years {
        return None;
    }
    Some(dps[..years].iter().all(|d| d.is_some_and(|v| v > 0.0)))
}

/// 前年同期比の伸び率（売上高, 営業利益）。0.1 = 10% 増。
/// 直近の期（1Q〜FY の期首からの累計）と、1年前の同じ期の累計を比べる。
/// 前年の値がないか 0 以下（赤字からの改善など）なら、その項目は None
pub fn yoy_growth(periods: &[CumulativePeriod]) -> (Option<f64>, Option<f64>) {
    let Some(latest) = periods
        .iter()
        .filter(|p| p.net_sales.is_some() || p.operating_income.is_some())
        .max_by_key(|p| (p.period_end, p.period_type))
    else {
        return (None, None);
    };
    // 決算期の変更で期末日が数日ずれても同じ期とみなす
    let one_year_ago = latest.fiscal_year_end.to_days() - 365;
    let prev = periods.iter().find(|p| {
        p.period_type == latest.period_type
            && (p.fiscal_year_end.to_days() - one_year_ago).abs() <= 15
    });
    let growth = |cur: Option<f64>, prev: Option<f64>| {
        let (c, p) = (cur?, prev?);
        (p > 0.0).then(|| c / p - 1.0)
    };
    match prev {
        Some(p) => (
            growth(latest.net_sales, p.net_sales),
            growth(latest.operating_income, p.operating_income),
        ),
        None => (None, None),
    }
}

/// バリュー株の目安: PBR と PER の両方が基準以下（PER は黒字が前提）
pub const VALUE_PBR_MAX: f64 = 1.0;
pub const VALUE_PER_MAX: f64 = 15.0;

pub fn is_value(pbr: Option<f64>, per: Option<f64>) -> Option<bool> {
    Some(pbr? <= VALUE_PBR_MAX && (0.0..=VALUE_PER_MAX).contains(&per?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn cum(fy: &str, pe: &str, t: PeriodType, net: f64) -> CumulativePeriod {
        CumulativePeriod {
            fiscal_year_end: d(fy),
            period_end: d(pe),
            period_type: t,
            net_sales: Some(net * 10.0),
            operating_income: Some(net * 1.5),
            ordinary_income: None,
            net_income: Some(net),
        }
    }

    #[test]
    fn splits_cumulative_into_quarters() {
        let periods = [
            cum("2026-03-31", "2025-12-31", PeriodType::Q3, 30.0),
            cum("2026-03-31", "2025-06-30", PeriodType::Q1, 10.0),
            cum("2026-03-31", "2026-03-31", PeriodType::FY, 25.0), // 4Q は赤字
            cum("2026-03-31", "2025-09-30", PeriodType::Q2, 22.0),
            cum("2026-03-31", "2025-09-30", PeriodType::Q2, 22.0), // 訂正で重複
        ];
        let q = standalone_quarters(&periods);
        let nets: Vec<Option<f64>> = q.iter().map(|x| x.net_income).collect();
        assert_eq!(nets, vec![Some(10.0), Some(12.0), Some(8.0), Some(-5.0)]);
        assert_eq!(q[3].quarter, 4);
        assert_eq!(q[1].net_sales, Some(120.0));
    }

    #[test]
    fn missing_previous_quarter_is_unknown() {
        let periods = [
            cum("2026-03-31", "2025-12-31", PeriodType::Q3, 30.0),
            cum("2026-03-31", "2026-03-31", PeriodType::FY, 40.0),
        ];
        let q = standalone_quarters(&periods);
        assert_eq!(q[0].net_income, None, "2Q がないので 3Q 単独は不明");
        assert_eq!(q[1].net_income, Some(10.0));
    }

    #[test]
    fn quarterly_loss_flag() {
        let periods = [
            cum("2026-03-31", "2025-06-30", PeriodType::Q1, 10.0),
            cum("2026-03-31", "2025-09-30", PeriodType::Q2, 22.0),
            cum("2026-03-31", "2025-12-31", PeriodType::Q3, 30.0),
            cum("2026-03-31", "2026-03-31", PeriodType::FY, 25.0),
        ];
        let q = standalone_quarters(&periods);
        assert_eq!(no_quarterly_loss(&q, 20, 4), Some(false));
        assert_eq!(
            no_quarterly_loss(&q[..3], 20, 4),
            None,
            "判定できる四半期が足りない"
        );
        assert_eq!(no_quarterly_loss(&q[..3], 20, 3), Some(true));
    }

    #[test]
    fn growth_compares_same_period_last_year() {
        let periods = [
            cum("2025-03-31", "2024-09-30", PeriodType::Q2, 20.0),
            cum("2025-03-31", "2025-03-31", PeriodType::FY, 40.0),
            cum("2026-03-31", "2025-09-30", PeriodType::Q2, 25.0),
        ];
        let (sales, op) = yoy_growth(&periods);
        assert!(
            (sales.unwrap() - 0.25).abs() < 1e-9,
            "2Q 累計どうし（FY とは比べない）"
        );
        assert!((op.unwrap() - 0.25).abs() < 1e-9);
        assert_eq!(yoy_growth(&periods[..2]), (None, None), "1年前の FY がない");
        let mut loss = periods.clone();
        loss[0].operating_income = Some(-5.0);
        assert_eq!(yoy_growth(&loss).1, None, "前年が赤字なら伸び率は出さない");
        assert_eq!(yoy_growth(&[]), (None, None));
    }

    #[test]
    fn dividend_and_value_flags() {
        assert_eq!(
            dividend_every_year(&[Some(10.0), Some(8.0), Some(8.0), Some(5.0), Some(1.0)], 5),
            Some(true)
        );
        assert_eq!(
            dividend_every_year(&[Some(10.0), Some(0.0), Some(8.0), Some(5.0), Some(1.0)], 5),
            Some(false)
        );
        assert_eq!(
            dividend_every_year(&[Some(10.0), None, Some(8.0), Some(5.0), Some(1.0)], 5),
            Some(false)
        );
        assert_eq!(dividend_every_year(&[Some(10.0)], 5), None);
        assert_eq!(is_value(Some(0.8), Some(12.0)), Some(true));
        assert_eq!(is_value(Some(1.2), Some(12.0)), Some(false));
        assert_eq!(
            is_value(Some(0.8), Some(-3.0)),
            Some(false),
            "赤字（PER マイナス）は除く"
        );
        assert_eq!(is_value(None, Some(12.0)), None);
    }
}
