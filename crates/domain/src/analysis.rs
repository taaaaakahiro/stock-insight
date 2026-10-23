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
    let prev = periods.iter().find(|p| {
        p.period_type == latest.period_type
            && is_previous_year(p.fiscal_year_end, latest.fiscal_year_end)
    });
    match prev {
        Some(p) => (
            growth(latest.net_sales, p.net_sales),
            growth(latest.operating_income, p.operating_income),
        ),
        None => (None, None),
    }
}

/// `prev` が `cur` の1年前の決算期か。決算期の変更で期末日が数日ずれても同じ期とみなす
fn is_previous_year(prev: Date, cur: Date) -> bool {
    (prev.to_days() - (cur.to_days() - 365)).abs() <= 15
}

/// 伸び率（0.1 = 10% 増）。前年の値がないか 0 以下（赤字からの改善など）なら None
fn growth(cur: Option<f64>, prev: Option<f64>) -> Option<f64> {
    let (c, p) = (cur?, prev?);
    (p > 0.0).then(|| c / p - 1.0)
}

/// 四半期単独の損益と、前年の同じ四半期からの伸び率（詳細ページの業績表）
#[derive(Debug, Clone, PartialEq)]
pub struct QuarterGrowth {
    pub result: QuarterResult,
    pub sales_growth: Option<f64>,
    pub operating_growth: Option<f64>,
    pub ordinary_growth: Option<f64>,
    pub net_growth: Option<f64>,
}

/// 各四半期に、前年度の同じ四半期（1Q なら前年の 1Q）との伸び率を付ける。並びは入力のまま
pub fn quarter_growth(quarters: &[QuarterResult]) -> Vec<QuarterGrowth> {
    quarters
        .iter()
        .map(|q| {
            let prev = quarters.iter().find(|p| {
                p.quarter == q.quarter && is_previous_year(p.fiscal_year_end, q.fiscal_year_end)
            });
            let g = |f: fn(&QuarterResult) -> Option<f64>| growth(f(q), prev.and_then(f));
            QuarterGrowth {
                result: q.clone(),
                sales_growth: g(|x| x.net_sales),
                operating_growth: g(|x| x.operating_income),
                ordinary_growth: g(|x| x.ordinary_income),
                net_growth: g(|x| x.net_income),
            }
        })
        .collect()
}

/// 直近の四半期（単独）の業績の質。「増益が本物か」を見るための値で、一覧と AI レコメンドの評価に使う
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LatestQuarter {
    /// 直近の四半期単独の営業利益・純利益の前年同期比（0.1 = 10% 増）。前年の値がないか 0 以下なら None
    pub op_growth: Option<f64>,
    pub net_growth: Option<f64>,
    /// 直近4四半期の営業利益の合計と、その前の4四半期の合計の比 − 1（8四半期そろわなければ None）
    pub op_growth_ttm: Option<f64>,
    /// 利益が一過性の疑い: 純利益の伸びが営業利益の伸びを ONE_OFF_NET_GAP 以上上回る（特別利益で膨らんでいる）、
    /// または直近の四半期は増益なのに直近4四半期の合計では前年を下回る。営業利益の伸びが出なければ None
    pub one_off: Option<bool>,
}

/// 純利益の伸びが営業利益の伸びをこれ以上（30ポイント）上回れば、特別利益の疑いとみなす
pub const ONE_OFF_NET_GAP: f64 = 0.3;
/// 直近4四半期とその前の4四半期を比べるのに必要な四半期数
const TTM_QUARTERS: usize = 8;
/// 8四半期の期末の間隔として許す日数（7四半期分 ≈ 639日。四半期が抜けていれば外れる）
const TTM_SPAN_DAYS: std::ops::RangeInclusive<i64> = 600..=680;

/// 直近の四半期（営業利益が分かる最後の四半期）の前年同期比と、直近4四半期合計の伸び、一過性の疑い。
/// `quarters` は standalone_quarters の結果（期末日の古い順）
pub fn latest_quarter(quarters: &[QuarterResult]) -> LatestQuarter {
    let Some(pos) = quarters.iter().rposition(|q| q.operating_income.is_some()) else {
        return LatestQuarter::default();
    };
    let upto = &quarters[..=pos];
    let latest = quarter_growth(upto).pop().expect("upto is not empty");
    let op_growth_ttm = ttm_growth(upto);
    let one_off = latest.operating_growth.map(|op| {
        let special_gain = latest
            .net_growth
            .is_some_and(|net| net - op >= ONE_OFF_NET_GAP);
        let ttm_down = op > 0.0 && op_growth_ttm.is_some_and(|t| t < 0.0);
        special_gain || ttm_down
    });
    LatestQuarter {
        op_growth: latest.operating_growth,
        net_growth: latest.net_growth,
        op_growth_ttm,
        one_off,
    }
}

/// 直近4四半期の営業利益の合計 vs その前の4四半期の合計。8四半期すべて営業利益が分かり、間隔がそろっているとき
fn ttm_growth(quarters: &[QuarterResult]) -> Option<f64> {
    if quarters.len() < TTM_QUARTERS {
        return None;
    }
    let last8 = &quarters[quarters.len() - TTM_QUARTERS..];
    let span = last8[TTM_QUARTERS - 1].period_end.to_days() - last8[0].period_end.to_days();
    if !TTM_SPAN_DAYS.contains(&span) {
        return None;
    }
    let sum = |qs: &[QuarterResult]| qs.iter().map(|q| q.operating_income).sum::<Option<f64>>();
    growth(sum(&last8[4..]), sum(&last8[..4]))
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

    /// 四半期単独の値（期末日順に 1Q から並べる）。net の 1.5 倍を営業利益にする cum と同じ比率
    fn q(fy: &str, pe: &str, quarter: u8, op: f64, net: f64) -> QuarterResult {
        QuarterResult {
            fiscal_year_end: d(fy),
            quarter,
            period_end: d(pe),
            net_sales: Some(op * 10.0),
            operating_income: Some(op),
            ordinary_income: None,
            net_income: Some(net),
        }
    }

    /// 2年分（8四半期）。前年は毎四半期 営業利益 10・純利益 7、今年は `ops`・`nets`
    fn two_years(ops: [f64; 4], nets: [f64; 4]) -> Vec<QuarterResult> {
        let mut out = vec![
            q("2025-03-31", "2024-06-30", 1, 10.0, 7.0),
            q("2025-03-31", "2024-09-30", 2, 10.0, 7.0),
            q("2025-03-31", "2024-12-31", 3, 10.0, 7.0),
            q("2025-03-31", "2025-03-31", 4, 10.0, 7.0),
        ];
        let ends = ["2025-06-30", "2025-09-30", "2025-12-31", "2026-03-31"];
        for i in 0..4 {
            out.push(q("2026-03-31", ends[i], i as u8 + 1, ops[i], nets[i]));
        }
        out
    }

    #[test]
    fn latest_quarter_growth_and_ttm() {
        // 今年: 営業利益 12,12,12,12（各 +20%）、純利益 8.4（+20%）
        let qs = two_years([12.0; 4], [8.4; 4]);
        let l = latest_quarter(&qs);
        assert!((l.op_growth.unwrap() - 0.2).abs() < 1e-9);
        assert!((l.net_growth.unwrap() - 0.2).abs() < 1e-9);
        assert!((l.op_growth_ttm.unwrap() - 0.2).abs() < 1e-9, "48 vs 40");
        assert_eq!(l.one_off, Some(false));

        // 7四半期しかなければ合計は比べない。直近の前年同期比は出る
        let l7 = latest_quarter(&qs[1..]);
        assert_eq!(l7.op_growth_ttm, None);
        assert!(l7.op_growth.is_some());

        // 直近の四半期の営業利益が分からなければ、その前の四半期を直近とする
        let mut missing = qs.clone();
        missing[7].operating_income = None;
        let lm = latest_quarter(&missing);
        assert!((lm.op_growth.unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(lm.op_growth_ttm, None, "8四半期そろわない");

        assert_eq!(latest_quarter(&[]), LatestQuarter::default());
        assert_eq!(latest_quarter(&qs[4..]).op_growth, None, "前年がない");
    }

    #[test]
    fn one_off_profit_is_flagged() {
        // 特別利益: 営業利益 +20% なのに純利益 +60%
        let gain = latest_quarter(&two_years([12.0; 4], [7.0, 7.0, 7.0, 11.2]));
        assert_eq!(gain.one_off, Some(true));
        // 純利益の伸びが 30pt 未満の差なら疑わない（+20% と +45%）
        let mild = latest_quarter(&two_years([12.0; 4], [7.0, 7.0, 7.0, 10.15]));
        assert_eq!(mild.one_off, Some(false));
        // 直近の四半期だけ増益（+20%）で、4四半期の合計（32+12=... 6,6,8,12=32 vs 40）は減益
        let bounce = latest_quarter(&two_years([6.0, 6.0, 8.0, 12.0], [4.0, 4.0, 5.0, 8.4]));
        assert!(bounce.op_growth_ttm.unwrap() < 0.0);
        assert_eq!(bounce.one_off, Some(true));
        // 直近も合計も減益なら「一過性」ではなくただの減益
        let down = latest_quarter(&two_years([8.0; 4], [5.6; 4]));
        assert_eq!(down.one_off, Some(false));
        // 前年の営業利益が赤字で伸び率が出なければ判定しない
        let mut turnaround = two_years([12.0; 4], [8.4; 4]);
        turnaround[3].operating_income = Some(-1.0);
        assert_eq!(latest_quarter(&turnaround).one_off, None);
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
    fn quarter_growth_compares_same_quarter_last_year() {
        let periods = [
            cum("2025-03-31", "2024-06-30", PeriodType::Q1, 10.0),
            cum("2025-03-31", "2024-09-30", PeriodType::Q2, 18.0),
            cum("2026-03-31", "2025-06-30", PeriodType::Q1, 12.0),
            cum("2026-03-31", "2025-09-30", PeriodType::Q2, 26.0),
        ];
        let g = quarter_growth(&standalone_quarters(&periods));
        assert_eq!(g.len(), 4);
        assert_eq!(g[0].net_growth, None, "前年がない");
        let q1 = &g[2];
        assert_eq!(
            (q1.result.quarter, q1.result.fiscal_year_end),
            (1, d("2026-03-31"))
        );
        assert!(
            (q1.net_growth.unwrap() - 0.2).abs() < 1e-9,
            "1Q どうし 10 → 12"
        );
        assert!((q1.sales_growth.unwrap() - 0.2).abs() < 1e-9);
        assert_eq!(q1.ordinary_growth, None, "値がない項目");
        assert!(
            (g[3].net_growth.unwrap() - 0.75).abs() < 1e-9,
            "2Q 単独 8 → 14"
        );
    }

    #[test]
    fn quarter_growth_skips_loss_base_and_tolerates_shifted_year_end() {
        let periods = [
            cum("2025-03-28", "2024-06-30", PeriodType::Q1, -4.0),
            cum("2026-03-31", "2025-06-30", PeriodType::Q1, 6.0),
        ];
        let g = quarter_growth(&standalone_quarters(&periods));
        assert_eq!(g[1].net_growth, None, "前年が赤字なら伸び率は出さない");
        assert!(g[1].sales_growth.is_none());
        let periods = [
            cum("2025-03-28", "2024-06-30", PeriodType::Q1, 4.0),
            cum("2026-03-31", "2025-06-30", PeriodType::Q1, 6.0),
        ];
        let g = quarter_growth(&standalone_quarters(&periods));
        assert!(
            (g[1].net_growth.unwrap() - 0.5).abs() < 1e-9,
            "期末日が数日ずれても前年とみなす"
        );
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
