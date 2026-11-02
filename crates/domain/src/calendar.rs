//! 東証の営業日の判定。土日・祝日（内閣府の祝日一覧）・年末年始（12/31〜1/3）は休場。

use crate::Date;

/// 0=日曜 … 6=土曜
pub fn weekday(d: Date) -> u32 {
    (d.to_days() + 4).rem_euclid(7) as u32
}

pub fn is_trading_day(d: Date, holidays: &[Date]) -> bool {
    let w = weekday(d);
    if w == 0 || w == 6 {
        return false;
    }
    // 東証の年末年始の休業日
    if (d.m == 12 && d.d == 31) || (d.m == 1 && d.d <= 3) {
        return false;
    }
    !holidays.contains(&d)
}

/// GitHub の schedule は数時間遅れて翌日に起動することがあるので、この時刻（JST）より前の起動は前日の定期実行として扱う
pub const BATCH_DAY_CUTOFF_HOUR_JST: i64 = 9;

/// 定期バッチが対象とする日（JST）。18:30 の起動が翌 01:00 にずれても前日の分として判定する
pub fn batch_date(unix: i64) -> Date {
    Date::from_unix_jst(unix - BATCH_DAY_CUTOFF_HOUR_JST * 3600)
}

/// 内閣府「国民の祝日・休日」CSV（Shift_JIS）から日付だけを取り出す。
/// 1列目が "YYYY/M/D" の行だけを使う（見出しなど日本語の行は ASCII でないので読み飛ばす）
pub fn parse_holiday_csv(bytes: &[u8]) -> Vec<Date> {
    bytes
        .split(|&b| b == b'\n')
        .filter_map(|line| {
            let first = line.split(|&b| b == b',').next()?;
            let s = std::str::from_utf8(first).ok()?.trim();
            let mut it = s.split('/');
            let y = it.next()?.parse().ok()?;
            let m = it.next()?.parse().ok()?;
            let d = it.next()?.parse().ok()?;
            Some(Date::new(y, m, d))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn parses_cabinet_office_csv() {
        // 見出しは Shift_JIS の日本語（UTF-8 としては読めない）
        let mut csv = vec![0x8d, 0x91, 0x96, 0xaf, b',', 0x96, 0xbc, b'\r', b'\n'];
        csv.extend_from_slice(b"2026/1/1,x\r\n2026/10/12,y\r\n");
        assert_eq!(
            parse_holiday_csv(&csv),
            vec![d("2026-01-01"), d("2026-10-12")]
        );
    }

    #[test]
    fn trading_days() {
        let holidays = [d("2026-10-12"), d("2026-11-03")];
        assert!(is_trading_day(d("2026-10-21"), &holidays)); // 水曜
        assert!(!is_trading_day(d("2026-10-24"), &holidays)); // 土曜
        assert!(!is_trading_day(d("2026-10-25"), &holidays)); // 日曜
        assert!(!is_trading_day(d("2026-10-12"), &holidays)); // スポーツの日
        assert!(!is_trading_day(d("2026-12-31"), &holidays)); // 大納会の翌日
        assert!(!is_trading_day(d("2027-01-04"), &[d("2027-01-04")]));
        assert!(!is_trading_day(d("2027-01-01"), &holidays));
        assert!(is_trading_day(d("2026-12-30"), &holidays)); // 大納会
        assert_eq!(weekday(d("2026-10-21")), 3);
    }

    #[test]
    fn delayed_schedule_counts_as_previous_day() {
        // 2026-10-30（金）09:30 UTC = 18:30 JST
        let fri_1830 = 1_793_352_600;
        assert_eq!(Date::from_unix_jst(fri_1830), d("2026-10-30"));
        assert_eq!(batch_date(fri_1830), d("2026-10-30"));
        // 土曜 01:11 JST に遅れて起動しても金曜の分
        assert_eq!(batch_date(fri_1830 + 6 * 3600 + 41 * 60), d("2026-10-30"));
        // 土曜 08:59 JST までは金曜、09:00 からは土曜
        assert_eq!(batch_date(fri_1830 + 14 * 3600 + 29 * 60), d("2026-10-30"));
        assert_eq!(batch_date(fri_1830 + 14 * 3600 + 30 * 60), d("2026-10-31"));
    }
}
