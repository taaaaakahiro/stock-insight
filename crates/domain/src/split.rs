//! 株式分割・併合。J-Quants の日足の AdjFactor（1:2 の分割なら権利落ち日の行に 0.5）を記録しておき、
//! 分割をまたぐ終値と1株当たりの値（配当・EPS・BPS）を、最新の株数の基準にそろえる。
//!
//! 終値は分割調整前、配当などは開示した時点の1株当たりなので、分割の後から次の本決算の開示までの間は
//! 「分割後の株価 ÷ 分割前の配当」になり、利回りが分割の比率だけ高く出てしまう。それを防ぐ。

use crate::{Date, PriceBar};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    /// 権利落ち日（この日の終値から分割後の株数）
    pub date: Date,
    /// AdjFactor。1:2 の分割なら 0.5、2:1 の併合なら 2.0
    pub factor: f64,
}

impl Split {
    /// 分割・併合として意味のある係数だけを受け付ける（1.0 は分割なし）
    pub fn new(date: Date, factor: f64) -> Option<Self> {
        (factor.is_finite() && factor > 0.0 && factor != 1.0).then_some(Self { date, factor })
    }
}

/// `date` より後に権利落ちした分割の係数をすべて掛けたもの。
/// `date` の時点の終値や、`date` に開示された1株当たりの値に掛けると、最新の株数の基準になる
pub fn factor_after(splits: &[Split], date: Date) -> f64 {
    splits
        .iter()
        .filter(|s| s.date > date)
        .map(|s| s.factor)
        .product()
}

/// 終値を最新の株数の基準にそろえる（分割より前の日の終値に係数を掛ける）
pub fn adjust_bars(bars: &mut [PriceBar], splits: &[Split]) {
    if splits.is_empty() {
        return;
    }
    for b in bars {
        b.close *= factor_after(splits, b.date);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn only_real_splits_are_kept() {
        assert!(Split::new(d("2026-09-29"), 1.0).is_none());
        assert!(Split::new(d("2026-09-29"), 0.0).is_none());
        assert!(Split::new(d("2026-09-29"), f64::NAN).is_none());
        assert_eq!(
            Split::new(d("2026-09-29"), 0.5),
            Some(Split {
                date: d("2026-09-29"),
                factor: 0.5
            })
        );
    }

    #[test]
    fn factor_counts_only_later_splits() {
        let s = [
            Split::new(d("2024-03-28"), 0.5).unwrap(),
            Split::new(d("2026-09-29"), 0.2).unwrap(),
        ];
        assert_eq!(factor_after(&s, d("2024-03-27")), 0.5 * 0.2);
        assert_eq!(
            factor_after(&s, d("2024-03-28")),
            0.2,
            "権利落ち日の終値はもう分割後"
        );
        assert_eq!(factor_after(&s, d("2026-09-29")), 1.0);
    }

    #[test]
    fn bars_line_up_across_a_split() {
        let mut bars = vec![
            PriceBar {
                date: d("2026-09-26"),
                close: 5000.0,
            },
            PriceBar {
                date: d("2026-09-29"),
                close: 1010.0,
            },
        ];
        adjust_bars(&mut bars, &[Split::new(d("2026-09-29"), 0.2).unwrap()]);
        assert_eq!(bars[0].close, 1000.0);
        assert_eq!(bars[1].close, 1010.0, "最新の終値は変わらない");
    }
}
