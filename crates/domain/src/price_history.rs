//! 株価の履歴（終値）の保存形式と更新。D1 の1行に "YYYYMMDD,終値;..." でまとめる。

use crate::{Date, PriceBar};

/// 保存しておく期間（日）。利回りの位置は最大10年だが、無料プランで取れるのは約2年なので3年あれば足りる
pub const KEEP_DAYS: i64 = 3 * 366;

pub fn encode(bars: &[PriceBar]) -> String {
    bars.iter()
        .map(|b| {
            format!(
                "{:04}{:02}{:02},{}",
                b.date.y,
                b.date.m,
                b.date.d,
                // 円の小数第1位までで十分（例: 176.2）
                (b.close * 10.0).round() / 10.0
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn decode(s: &str) -> Vec<PriceBar> {
    let mut bars: Vec<PriceBar> = s
        .split(';')
        .filter_map(|item| {
            let (d, c) = item.split_once(',')?;
            if d.len() != 8 {
                return None;
            }
            let date = Date::new(
                d[0..4].parse().ok()?,
                d[4..6].parse().ok()?,
                d[6..8].parse().ok()?,
            );
            let close: f64 = c.parse().ok()?;
            close.is_finite().then_some(PriceBar { date, close })
        })
        .collect();
    bars.sort_by_key(|b| b.date);
    bars
}

/// 新しい終値を足す（同じ日付は上書き）。古すぎる分は捨てる
pub fn merge(existing: &mut Vec<PriceBar>, new: &[PriceBar]) {
    for b in new {
        match existing.binary_search_by_key(&b.date, |x| x.date) {
            Ok(i) => existing[i] = *b,
            Err(i) => existing.insert(i, *b),
        }
    }
    if let Some(last) = existing.last().copied() {
        let from = last.date.to_days() - KEEP_DAYS;
        existing.retain(|b| b.date.to_days() >= from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(s: &str, close: f64) -> PriceBar {
        PriceBar {
            date: Date::parse(s).unwrap(),
            close,
        }
    }

    #[test]
    fn roundtrip() {
        let bars = vec![bar("2026-07-01", 3094.0), bar("2026-07-02", 176.1999969)];
        let s = encode(&bars);
        assert_eq!(s, "20260701,3094;20260702,176.2");
        assert_eq!(
            decode(&s),
            vec![bar("2026-07-01", 3094.0), bar("2026-07-02", 176.2)]
        );
        assert!(decode("").is_empty());
        assert_eq!(decode("bad;20260701,1"), vec![bar("2026-07-01", 1.0)]);
    }

    #[test]
    fn merge_inserts_in_order_and_overwrites() {
        let mut h = vec![bar("2026-07-01", 1.0), bar("2026-07-03", 3.0)];
        merge(&mut h, &[bar("2026-07-02", 2.0), bar("2026-07-03", 30.0)]);
        assert_eq!(
            h,
            vec![
                bar("2026-07-01", 1.0),
                bar("2026-07-02", 2.0),
                bar("2026-07-03", 30.0)
            ]
        );
    }

    #[test]
    fn merge_drops_old_bars() {
        let mut h = vec![bar("2020-01-01", 1.0)];
        merge(&mut h, &[bar("2026-07-01", 2.0)]);
        assert_eq!(h, vec![bar("2026-07-01", 2.0)]);
    }
}
