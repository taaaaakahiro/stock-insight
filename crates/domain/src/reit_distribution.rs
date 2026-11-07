//! 1口分配金を、株価の時点で分かっていた決算期ごとの金額から年額にする。

use crate::{Date, Dividend};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastPeriod {
    pub start: Option<Date>,
    pub end: Date,
    pub amount: f64,
}

/// J-Quants の1開示に含まれる、主な対象期の補足情報ともう一つの予想。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ForecastDetails {
    pub start: Option<Date>,
    pub other: Option<ForecastPeriod>,
    pub disclosed_time: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Forecast {
    pub period: ForecastPeriod,
    pub disclosed: Date,
    pub disclosed_time: Option<String>,
    pub doc_id: String,
}

#[derive(Debug, Clone)]
pub struct Actual {
    pub end: Date,
    pub disclosed: Date,
    pub amount: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DistributionHistory {
    pub actuals: Vec<Actual>,
    pub forecasts: Vec<Forecast>,
}

fn year_from(day: Date, offset: i32) -> Date {
    let date = Date::new(day.y + offset, day.m, day.d);
    // うるう日の翌年・前年は2月末まで。
    if Date::from_days(date.to_days()).m != day.m {
        date.add_days(-1)
    } else {
        date
    }
}

fn annual_frequency(days: i64) -> Option<u32> {
    // 通常の決算周期だけを採用する。月の日数差は許容するが、変則決算をそのまま繰り返さない。
    [1, 2, 3, 4, 6, 12]
        .into_iter()
        .find(|&n| (days - 365 / i64::from(n)).abs() <= 5)
}

impl DistributionHistory {
    pub fn at(&self, day: Date) -> Option<Dividend> {
        let known_actuals: BTreeMap<_, _> = self
            .actuals
            .iter()
            .filter(|a| a.disclosed <= day && a.end <= day)
            .map(|a| (a.end, a.amount))
            .collect();
        let mut forecasts = BTreeMap::<Date, &Forecast>::new();
        let until = year_from(day, 1);
        for f in &self.forecasts {
            let end = f.period.end;
            if f.disclosed > day || end <= day || end > until {
                continue;
            }
            let old = forecasts.entry(end).or_insert(f);
            if (f.disclosed, &f.disclosed_time, &f.doc_id)
                > (old.disclosed, &old.disclosed_time, &old.doc_id)
            {
                *old = f;
            }
        }
        let forecast = match forecasts.len() {
            0 => None,
            1 => {
                let period = &forecasts.values().next()?.period;
                let frequency = match period.start {
                    Some(start) => annual_frequency(period.end.to_days() - start.to_days() + 1),
                    None => {
                        let mut ends = known_actuals.keys().rev();
                        ends.next().zip(ends.next()).and_then(|(last, previous)| {
                            annual_frequency(last.to_days() - previous.to_days())
                        })
                    }
                };
                frequency.map(|n| period.amount * f64::from(n))
            }
            _ => Some(forecasts.values().map(|f| f.period.amount).sum()),
        };
        let (annual, forecast) = match forecast {
            Some(amount) => (amount, true),
            None => {
                // 直近の開示済み決算期末を起点にする。次の期の発表待ちで
                // 前年同期の実績だけが先に抜け、分配金が半減するのを防ぐ。
                let latest = *known_actuals.last_key_value()?.0;
                if latest <= year_from(day, -1) {
                    return None;
                }
                let since = if latest.m == 2 && latest.add_days(1).m == 3 {
                    Date::new(latest.y - 1, 3, 1).add_days(-1)
                } else {
                    year_from(latest, -1)
                };
                let mut actuals = known_actuals
                    .range((std::ops::Bound::Excluded(since), std::ops::Bound::Unbounded));
                let first = (*actuals.next()?.1)?;
                let sum = actuals.try_fold(first, |sum, (_, amount)| amount.map(|a| sum + a))?;
                (sum, false)
            }
        };
        Some(Dividend {
            effective: day,
            annual,
            forecast,
            breakdown: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(value: &str) -> Date {
        Date::parse(value).unwrap()
    }

    fn forecast(start: &str, end: &str, disclosed: &str, amount: f64) -> Forecast {
        Forecast {
            period: ForecastPeriod {
                start: Some(d(start)),
                end: d(end),
                amount,
            },
            disclosed: d(disclosed),
            disclosed_time: None,
            doc_id: String::new(),
        }
    }

    #[test]
    fn latest_forecast_per_period_is_used_within_twelve_months() {
        let first = forecast("2026-04-01", "2026-09-30", "2026-05-01", 3000.0);
        let second = forecast("2026-10-01", "2027-03-31", "2026-06-01", 2700.0);
        let mut revision = forecast("2026-04-01", "2026-09-30", "2026-06-01", 3500.0);
        revision.disclosed_time = Some("16:00:00".into());
        let mut earlier = revision.clone();
        earlier.period.amount = 9999.0;
        earlier.disclosed_time = Some("15:00:00".into());
        earlier.doc_id = "larger-id".into();
        let outside = forecast("2027-04-01", "2027-09-30", "2026-06-01", 9999.0);
        let history = DistributionHistory {
            forecasts: vec![first, revision, earlier, second, outside],
            ..Default::default()
        };
        let at = |day| history.at(d(day)).map(|div| div.annual);
        assert_eq!(at("2026-04-30"), None);
        assert_eq!(at("2026-05-01"), Some(6000.0));
        assert_eq!(at("2026-06-01"), Some(6200.0)); // 3500 + 2700
        assert_eq!(at("2026-09-29"), Some(6200.0));
        assert_eq!(at("2026-09-30"), Some(12699.0)); // 入れ替わった次の2期
    }

    #[test]
    fn falls_back_to_reported_twelve_months_without_counting_a_third_february_period() {
        let actual = |end, disclosed, amount| Actual {
            end: d(end),
            disclosed: d(disclosed),
            amount: Some(amount),
        };
        let history = DistributionHistory {
            actuals: vec![
                actual("2024-02-29", "2024-04-15", 3100.0),
                actual("2024-08-31", "2024-10-15", 2800.0),
                actual("2025-02-28", "2025-04-15", 3200.0),
                actual("2025-08-31", "2025-10-15", 2900.0),
            ],
            forecasts: vec![forecast("2025-03-01", "2025-08-31", "2025-04-15", 0.0)],
        };
        let at = |day| history.at(d(day)).map(|div| (div.annual, div.forecast));
        assert_eq!(at("2025-08-30"), Some((0.0, true)));
        assert_eq!(at("2025-09-30"), Some((6000.0, false)));
        assert_eq!(at("2025-10-15"), Some((6100.0, false)));
        assert_eq!(at("2026-08-31"), None);
    }

    #[test]
    fn single_forecast_uses_the_issuers_cadence() {
        let mut f = forecast("2026-04-01", "2027-03-31", "2026-04-01", 3000.0);
        let day = d("2026-04-01");
        let mut history = DistributionHistory {
            forecasts: vec![f.clone()],
            ..Default::default()
        };
        assert_eq!(history.at(day).unwrap().annual, 3000.0);
        f.period.start = None; // 旧データには予想対象期の開始日がない
        f.period.end = d("2026-09-30");
        history.forecasts = vec![f];
        assert_eq!(history.at(day), None);
        history.actuals = ["2025-08-31", "2026-02-28"]
            .into_iter()
            .map(|end| Actual {
                end: d(end),
                disclosed: d(end).add_days(20),
                amount: Some(3100.0),
            })
            .collect();
        assert_eq!(history.at(day).unwrap().annual, 6000.0);
    }
}
