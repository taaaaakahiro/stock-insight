//! 株価の履歴（終値）の保存形式と更新。D1 の1行に "YYYYMMDD,終値;..." でまとめる。

use crate::{Date, PriceBar};

/// 保存しておく期間（日）。利回りの過去5年比（indicators::HISTORY_DAYS）に足りるよう、5年に1か月の余裕を足す
pub const KEEP_DAYS: i64 = crate::indicators::HISTORY_DAYS + 31;
const _: () = assert!(KEEP_DAYS >= crate::indicators::HISTORY_DAYS);

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

/// 指定日の終値。休場日は直前の終値を使うが、古い・未取得のデータで代用しない。
/// 当日分の取り込み前も、最新の収録日より後の日付なら未取得として待つ。
pub fn acquisition_close(bars: &[PriceBar], date: Date) -> Option<PriceBar> {
    let latest = bars.iter().map(|b| b.date).max()?;
    // 土日は金曜の収録があれば確定できる。平日の未収録は祝日か未取得か判別できないため待つ。
    let weekday = (date.to_days() + 3).rem_euclid(7);
    let required = if weekday >= 5 {
        date.add_days(-(weekday - 4))
    } else {
        date
    };
    if latest < required {
        return None;
    }
    bars.iter()
        .filter(|b| {
            b.date <= date
                && date.to_days() - b.date.to_days() <= 7
                && b.close.is_finite()
                && b.close > 0.0
        })
        .max_by_key(|b| b.date)
        .copied()
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
    fn acquisition_uses_close_and_previous_session_without_future_or_stale_prices() {
        let bars = vec![bar("2026-10-23", 100.0), bar("2026-10-26", 110.0)];
        assert_eq!(
            acquisition_close(&bars, Date::parse("2026-10-23").unwrap())
                .unwrap()
                .close,
            100.0
        );
        assert_eq!(
            acquisition_close(&bars, Date::parse("2026-10-24").unwrap())
                .unwrap()
                .date,
            Date::parse("2026-10-23").unwrap()
        );
        assert!(acquisition_close(&bars, Date::parse("2026-10-22").unwrap()).is_none());
        assert!(acquisition_close(&bars, Date::parse("2026-10-27").unwrap()).is_none());
        assert!(acquisition_close(&[], Date::parse("2026-10-23").unwrap()).is_none());
        assert!(acquisition_close(&bars[..1], Date::parse("2026-10-24").unwrap()).is_some());
        let holiday = vec![bar("2026-10-09", 90.0), bar("2026-10-13", 100.0)];
        assert_eq!(
            acquisition_close(&holiday, Date::parse("2026-10-12").unwrap())
                .unwrap()
                .close,
            90.0
        );
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

    #[test]
    fn merge_keeps_five_years() {
        let mut h = vec![bar("2021-05-01", 0.5), bar("2021-06-01", 1.0)];
        merge(&mut h, &[bar("2026-07-01", 2.0)]);
        assert_eq!(
            h,
            vec![bar("2021-06-01", 1.0), bar("2026-07-01", 2.0)],
            "利回りの過去5年比に使う5年分は残し、それより古い分だけ捨てる"
        );
    }
}
