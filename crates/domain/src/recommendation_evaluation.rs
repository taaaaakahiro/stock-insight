//! 推薦公開後の翌営業日終値から評価する。配当を含まない価格成績の第1版。
//! カレンダーは銘柄の観測日から作らず、市場営業日の独立した一覧を渡す。
use crate::{split::Split, Date, PriceBar};
use std::collections::BTreeMap;

pub const VERSION: &str = "next-close-price-v1";
pub const HORIZONS: [usize; 5] = [0, 21, 63, 126, 252];

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    pub status: &'static str,
    pub entry_date: Option<Date>,
    pub end_date: Option<Date>,
    pub entry_price: Option<f64>,
    pub end_price: Option<f64>,
    pub price_return: Option<f64>,
    pub max_drawdown: Option<f64>,
    pub missing_days: usize,
}

/// horizon=0 は最新（最大252営業日）、それ以外は期限別の確定結果。
/// カレンダーは公開日より後かつ as_of 以下の全市場営業日。昇順・重複なし。
pub fn evaluate(
    bars: &[PriceBar],
    splits: &[Split],
    calendar: &[Date],
    horizon: usize,
) -> Evaluation {
    let mut out = Evaluation {
        status: "pending",
        entry_date: calendar.first().copied(),
        end_date: None,
        entry_price: None,
        end_price: None,
        price_return: None,
        max_drawdown: None,
        missing_days: 0,
    };
    if calendar.is_empty() {
        return out;
    }
    let prices: BTreeMap<_, _> = bars
        .iter()
        .filter(|b| b.close.is_finite() && b.close > 0.0)
        .map(|b| (b.date, b.close))
        .collect();
    let entry = calendar[0];
    out.entry_price = prices.get(&entry).copied();
    if out.entry_price.is_none() {
        out.status = if calendar.len() >= 20 {
            "entry_unavailable"
        } else {
            "data_missing"
        };
        out.missing_days = 1;
        return out;
    }
    let end_index = if horizon == 0 {
        (calendar.len() - 1).min(252)
    } else {
        horizon
    };
    if end_index >= calendar.len() {
        return out;
    }
    let end = calendar[end_index];
    out.end_date = Some(end);
    out.end_price = prices.get(&end).copied();
    out.missing_days = calendar[..=end_index]
        .iter()
        .filter(|d| !prices.contains_key(d))
        .count();
    if out.missing_days > 0 {
        out.status = "data_missing";
        return out;
    }
    let mut peak = 1.0_f64;
    let mut worst = 0.0_f64;
    let mut value = 1.0;
    for day in &calendar[..=end_index] {
        // 1:2分割はfactor=0.5。分割日の終値は既に分割後なので株数を2倍にする。
        let factor: f64 = splits
            .iter()
            .filter(|s| s.date > entry && s.date <= *day)
            .map(|s| s.factor)
            .product();
        value = prices[day] / (out.entry_price.unwrap() * factor);
        if !value.is_finite() || value <= 0.0 {
            out.status = "data_missing";
            return out;
        }
        peak = peak.max(value);
        worst = worst.min(value / peak - 1.0);
    }
    out.price_return = Some(value - 1.0);
    out.max_drawdown = Some(worst);
    out.status = if horizon == 0 && end_index < 252 {
        "tracking"
    } else {
        "complete"
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(day: u32) -> Date {
        Date::new(2026, 10, day)
    }
    fn bars(values: &[(u32, f64)]) -> Vec<PriceBar> {
        values
            .iter()
            .map(|(day, close)| PriceBar {
                date: d(*day),
                close: *close,
            })
            .collect()
    }
    #[test]
    fn next_session_entry_and_exact_horizon() {
        let b = bars(&[(23, 90.0), (26, 100.0), (27, 120.0), (28, 108.0)]);
        let e = evaluate(&b, &[], &[d(26), d(27), d(28)], 2);
        assert_eq!(e.entry_price, Some(100.0));
        assert!((e.price_return.unwrap() - 0.08).abs() < 1e-12);
        assert!((e.max_drawdown.unwrap() + 0.1).abs() < 1e-12);
        assert_eq!(e.status, "complete");
        assert_eq!(evaluate(&b, &[], &[d(26), d(27)], 2).status, "pending");
    }
    #[test]
    fn splits_and_reverse_splits_preserve_value() {
        let b = bars(&[(26, 100.0), (27, 50.0), (28, 100.0)]);
        let splits = [
            Split::new(d(26), 0.1).unwrap(),
            Split::new(d(27), 0.5).unwrap(),
            Split::new(d(28), 2.0).unwrap(),
            Split::new(d(29), 0.1).unwrap(),
        ];
        let e = evaluate(&b, &splits, &[d(26), d(27), d(28)], 2);
        assert_eq!(e.price_return, Some(0.0));
        assert_eq!(e.max_drawdown, Some(0.0));
    }
    #[test]
    fn missing_days_are_not_shortened_or_zero_filled() {
        let b = bars(&[(26, 100.0), (28, 110.0)]);
        let e = evaluate(&b, &[], &[d(26), d(27), d(28)], 2);
        assert_eq!(e.status, "data_missing");
        assert_eq!(e.price_return, None);
        assert_eq!(e.missing_days, 1);
        let e = evaluate(&b, &[], &[d(27), d(28)], 0);
        assert_eq!(e.entry_date, Some(d(27)));
        assert_eq!(e.entry_price, None);
    }
    #[test]
    fn no_future_prices_or_unmatured_returns() {
        let b = bars(&[(26, 100.0), (27, 120.0)]);
        assert_eq!(evaluate(&b, &[], &[], 0).status, "pending");
        assert_eq!(evaluate(&b, &[], &[d(26)], 21).price_return, None);
        assert_eq!(evaluate(&b, &[], &[d(26)], 0).price_return, Some(0.0));
    }
}
