use crate::model::*;

/// 一覧に出す最新指標（1銘柄1行）
#[derive(Debug, Clone, PartialEq)]
pub struct Indicators {
    pub price: f64,
    pub price_date: Date,
    /// 計算に使った年間配当（開示がなければ 0）
    pub dividend_ttm: f64,
    pub dividend_yield: Option<f64>,
    /// 配当利回りが取れる期間（最大10年）のどこにいるか（0.0〜1.0。高いほど割安）
    pub yield_percentile: Option<f64>,
    pub pbr: Option<f64>,
    pub per: Option<f64>,
    pub payout_ratio: Option<f64>,
    /// 簡易DOE = 1株配当 ÷ BPS
    pub doe: Option<f64>,
    pub consecutive_increases: Option<u32>,
    pub operating_margin: Option<f64>,
    pub no_loss_years: Option<bool>,
    pub profit_uptrend: Option<bool>,
    pub valuation: Valuation,
    /// 直近5期すべてで配当がある（期数が足りなければ None）
    pub dividend_every_year: Option<bool>,
    /// 直近20四半期に赤字がない（判定できる四半期が足りなければ None）
    pub no_quarterly_loss: Option<bool>,
    /// バリュー株の目安（PBR 1倍以下 かつ PER 15倍以下）
    pub is_value: Option<bool>,
    /// 判定に使えた年次財務の期数
    pub data_years: u32,
    /// 売上高・営業利益の前年同期比（直近の期の累計。0.1 = 10% 増）。compute の後に analysis::yoy_growth で入れる
    pub sales_growth: Option<f64>,
    pub op_growth: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Valuation {
    Cheap,
    Neutral,
    Expensive,
    Unknown,
}

impl Valuation {
    pub fn as_str(self) -> &'static str {
        match self {
            Valuation::Cheap => "cheap",
            Valuation::Neutral => "neutral",
            Valuation::Expensive => "expensive",
            Valuation::Unknown => "unknown",
        }
    }
}

pub const CHEAP_PERCENTILE: f64 = 0.8;
pub const EXPENSIVE_PERCENTILE: f64 = 0.2;
const MIN_HISTORY_SAMPLES: usize = 52;
/// 利回りの過去比に使う期間（J-Quants Light で取れる5年）。終値の履歴はこれ以上の期間を持つ（price_history::KEEP_DAYS）
pub const HISTORY_DAYS: i64 = 5 * 366;
/// 配当の継続・通期赤字の判定に使う期数
pub const ANALYSIS_YEARS: usize = 5;
/// 四半期の赤字判定に使う四半期数と、判定に最低限必要な四半期数
pub const ANALYSIS_QUARTERS: usize = 20;
const MIN_KNOWN_QUARTERS: usize = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum IndicatorError {
    NoPrice,
}

/// 指定日に有効な年間配当（その日までに開示された最新のもの）。まだ開示がなければ None
pub fn annual_dividend_at(divs: &[Dividend], date: Date) -> Option<f64> {
    divs.iter()
        .filter(|d| d.effective <= date)
        .max_by_key(|d| d.effective)
        .map(|d| d.annual)
}

/// 週1点（5営業日ごと）で過去の配当利回りを並べる。
/// その時点で開示済みの年間配当 ÷ その日の終値（どちらも分割調整前なので、分割をまたいでもずれない）
pub fn yield_history(market: &MarketData) -> Vec<f64> {
    let Some(last) = market.bars.last() else {
        return vec![];
    };
    let start = last.date.to_days() - HISTORY_DAYS;
    market
        .bars
        .iter()
        .filter(|b| b.date.to_days() >= start)
        .rev()
        .step_by(5)
        .filter(|b| b.close > 0.0)
        .filter_map(|b| annual_dividend_at(&market.dividends, b.date).map(|d| d / b.close))
        .collect()
}

/// current が samples の中で下から何割の位置か（0.0〜1.0）
pub fn percentile_rank(samples: &[f64], current: f64) -> Option<f64> {
    if samples.len() < MIN_HISTORY_SAMPLES {
        return None;
    }
    // 同じ値は半分だけ数える（中央順位）。利回りが変わらない期間が長くても、端に寄らない
    let below = samples.iter().filter(|&&s| s < current).count() as f64;
    let equal = samples.iter().filter(|&&s| s == current).count() as f64;
    Some((below + equal / 2.0) / samples.len() as f64)
}

/// 新しい順に並んだ値の連続増加回数
pub fn consecutive_increases(newest_first: &[f64]) -> u32 {
    newest_first.windows(2).take_while(|w| w[0] > w[1]).count() as u32
}

/// 最小二乗法の傾きが正か（古い順に並べ替えて計算）
pub fn is_uptrend(newest_first: &[f64]) -> Option<bool> {
    if newest_first.len() < 3 {
        return None;
    }
    let ys: Vec<f64> = newest_first.iter().rev().copied().collect();
    let n = ys.len() as f64;
    let mx = (n - 1.0) / 2.0;
    let my = ys.iter().sum::<f64>() / n;
    let (mut num, mut den) = (0.0, 0.0);
    for (i, y) in ys.iter().enumerate() {
        let dx = i as f64 - mx;
        num += dx * (y - my);
        den += dx * dx;
    }
    Some(num / den > 0.0)
}

fn ratio(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) if b > 0.0 => Some(a / b),
        _ => None,
    }
}

pub fn compute(
    market: &MarketData,
    fin: Option<&FinancialData>,
    quarters: &[crate::analysis::QuarterResult],
) -> Result<Indicators, IndicatorError> {
    let last = *market.bars.last().ok_or(IndicatorError::NoPrice)?;
    let price = last.close;
    let annual = annual_dividend_at(&market.dividends, last.date);
    let ttm = annual.unwrap_or(0.0);
    let dividend_yield = annual.filter(|_| price > 0.0).map(|d| d / price);
    let yield_percentile = dividend_yield.and_then(|y| percentile_rank(&yield_history(market), y));

    let years: &[AnnualFinancial] = fin.map(|f| f.years.as_slice()).unwrap_or(&[]);
    let latest = years.first();
    let eps = latest.and_then(|y| y.eps);
    let bps = latest.and_then(|y| y.bps);
    let dps = latest.and_then(|y| y.dps);

    let dps_series: Vec<f64> = years.iter().map_while(|y| y.dps).collect();
    let profit_series: Vec<f64> = years
        .iter()
        .map_while(|y| y.operating_income.or(y.ordinary_income))
        .collect();
    let incomes: Vec<f64> = years.iter().filter_map(|y| y.net_income).collect();

    let dps_all: Vec<Option<f64>> = years.iter().take(ANALYSIS_YEARS).map(|y| y.dps).collect();
    let pbr = ratio(Some(price), bps);
    let per = ratio(Some(price), eps);

    let valuation = match yield_percentile {
        Some(p) if p >= CHEAP_PERCENTILE => Valuation::Cheap,
        Some(p) if p <= EXPENSIVE_PERCENTILE => Valuation::Expensive,
        Some(_) => Valuation::Neutral,
        None => Valuation::Unknown,
    };

    Ok(Indicators {
        price,
        price_date: last.date,
        dividend_ttm: ttm,
        dividend_yield,
        yield_percentile,
        pbr,
        per,
        payout_ratio: ratio(dps, eps),
        doe: ratio(dps, bps),
        consecutive_increases: (dps_series.len() >= 2).then(|| consecutive_increases(&dps_series)),
        operating_margin: latest.and_then(|y| ratio(y.operating_income, y.net_sales)),
        no_loss_years: (!incomes.is_empty())
            .then(|| incomes.iter().all(|&v| v > 0.0) && profit_series.iter().all(|&v| v > 0.0)),
        profit_uptrend: is_uptrend(&profit_series),
        valuation,
        dividend_every_year: crate::analysis::dividend_every_year(&dps_all, ANALYSIS_YEARS),
        no_quarterly_loss: crate::analysis::no_quarterly_loss(
            quarters,
            ANALYSIS_QUARTERS,
            MIN_KNOWN_QUARTERS,
        ),
        is_value: crate::analysis::is_value(pbr, per),
        data_years: years.len().min(ANALYSIS_YEARS) as u32,
        sales_growth: None,
        op_growth: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn date_roundtrip() {
        for s in ["1970-01-01", "2000-02-29", "2026-10-21", "2024-12-31"] {
            let x = d(s);
            assert_eq!(Date::from_days(x.to_days()), x);
            assert_eq!(x.to_string(), s);
        }
        assert_eq!(d("1970-01-02").to_days(), 1);
        let midnight_utc = d("2026-10-20").to_days() * 86_400;
        // 06:30 UTC = 15:30 JST（同日）、16:00 UTC = 翌01:00 JST
        assert_eq!(
            Date::from_unix_jst(midnight_utc + 6 * 3600 + 1800).to_string(),
            "2026-10-20"
        );
        assert_eq!(
            Date::from_unix_jst(midnight_utc + 16 * 3600).to_string(),
            "2026-10-21"
        );
    }

    #[test]
    fn annual_dividend_uses_the_latest_disclosure() {
        let divs = [
            Dividend {
                effective: d("2025-05-10"),
                annual: 40.0,
            },
            Dividend {
                effective: d("2026-05-12"),
                annual: 44.0,
            },
        ];
        assert_eq!(annual_dividend_at(&divs, d("2025-05-09")), None);
        assert_eq!(annual_dividend_at(&divs, d("2025-12-31")), Some(40.0));
        assert_eq!(annual_dividend_at(&divs, d("2026-05-12")), Some(44.0));
    }

    #[test]
    fn increases_and_trend() {
        assert_eq!(consecutive_increases(&[30.0, 28.0, 25.0, 25.0, 20.0]), 2);
        assert_eq!(consecutive_increases(&[30.0]), 0);
        assert_eq!(is_uptrend(&[5.0, 4.0, 3.0]), Some(true));
        assert_eq!(is_uptrend(&[3.0, 4.0, 5.0]), Some(false));
        assert_eq!(is_uptrend(&[1.0, 2.0]), None);
    }

    #[test]
    fn percentile_needs_history() {
        assert_eq!(percentile_rank(&[0.03; 10], 0.03), None);
        let s: Vec<f64> = (1..=100).map(|i| i as f64 / 1000.0).collect();
        assert_eq!(percentile_rank(&s, 0.08), Some(0.795));
        // ずっと同じ値なら真ん中
        assert_eq!(percentile_rank(&[0.03; 60], 0.03), Some(0.5));
    }

    #[test]
    fn compute_full() {
        // 3年分の平日終値（1000円→900円に下落）と年2回の配当
        let start = d("2023-10-02");
        let mut bars = vec![];
        let mut day = start;
        let mut i = 0;
        while day <= d("2026-10-20") {
            let wd = (day.to_days() + 4).rem_euclid(7); // 0=日曜
            if (1..=5).contains(&wd) {
                bars.push(PriceBar {
                    date: day,
                    close: 1000.0 - i as f64 * 0.13,
                });
                i += 1;
            }
            day = day.add_days(1);
        }
        // 年間配当: 2024年5月に32円、2025年5月に40円、2026年5月に44円と開示
        let dividends = vec![
            Dividend {
                effective: d("2024-05-10"),
                annual: 32.0,
            },
            Dividend {
                effective: d("2025-05-10"),
                annual: 40.0,
            },
            Dividend {
                effective: d("2026-05-12"),
                annual: 44.0,
            },
        ];
        let market = MarketData { bars, dividends };
        let fin = FinancialData {
            doc_id: "S100TEST".into(),
            submitted: Some(d("2026-06-20")),
            years: vec![
                AnnualFinancial {
                    net_sales: Some(1000.0),
                    operating_income: Some(120.0),
                    net_income: Some(80.0),
                    eps: Some(110.0),
                    bps: Some(1200.0),
                    dps: Some(44.0),
                    ..Default::default()
                },
                AnnualFinancial {
                    net_sales: Some(950.0),
                    operating_income: Some(110.0),
                    net_income: Some(70.0),
                    eps: Some(100.0),
                    bps: Some(1150.0),
                    dps: Some(40.0),
                    ..Default::default()
                },
                AnnualFinancial {
                    net_sales: Some(900.0),
                    operating_income: Some(100.0),
                    net_income: Some(60.0),
                    eps: Some(90.0),
                    bps: Some(1100.0),
                    dps: Some(36.0),
                    ..Default::default()
                },
            ],
        };
        let ind = compute(&market, Some(&fin), &[]).unwrap();
        // 最新の開示（2026年5月）の 44円を使う
        assert!((ind.dividend_ttm - 44.0).abs() < 1e-9);
        assert!((ind.payout_ratio.unwrap() - 0.4).abs() < 1e-9);
        assert!((ind.doe.unwrap() - 44.0 / 1200.0).abs() < 1e-9);
        assert_eq!(ind.consecutive_increases, Some(2));
        assert_eq!(ind.no_loss_years, Some(true));
        assert_eq!(ind.profit_uptrend, Some(true));
        assert!((ind.operating_margin.unwrap() - 0.12).abs() < 1e-9);
        // 株価が下がり配当が増えているので、利回りは過去の上位にいる
        assert_eq!(ind.valuation, Valuation::Cheap);
        assert_eq!(
            ind.dividend_every_year, None,
            "3期しかないので5年の判定はできない"
        );
        assert_eq!(ind.no_quarterly_loss, None);
        assert_eq!(ind.data_years, 3);
        // 株価 900 円前後、BPS 1200、EPS 110 → PBR 0.75、PER 8 でバリュー
        assert_eq!(ind.is_value, Some(true));
    }

    #[test]
    fn compute_without_price() {
        assert_eq!(
            compute(&MarketData::default(), None, &[]),
            Err(IndicatorError::NoPrice)
        );
    }
}
