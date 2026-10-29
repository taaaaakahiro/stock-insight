//! EDINETの当期配当について、本文中で合計と内訳が一致するものだけを採用する。
//! 記載がないことを「特別配当ゼロ」とはみなさない。
use crate::disclosure::normalize_text;

#[derive(Debug, Clone, PartialEq)]
pub struct DividendEvidence {
    pub total: f64,
    pub special: f64,
    pub commemorative: f64,
    pub phrase: String,
}

fn yen_after(text: &str, label: &str) -> Option<f64> {
    if text.matches(label).count() != 1 {
        return None;
    }
    let tail = text.split_once(label)?.1;
    let start = tail.find(|c: char| c.is_ascii_digit())?;
    if tail[..start].chars().count() > 12 {
        return None;
    }
    let tail = &tail[start..];
    let end = tail.find(|c: char| !c.is_ascii_digit() && c != '.' && c != ',')?;
    if !tail[end..].starts_with('円') {
        return None;
    }
    let amount: f64 = tail[..end].replace(',', "").parse().ok()?;
    // 円未満の表現を丸めて読まない。
    let after_yen = &tail[end + '円'.len_utf8()..];
    if after_yen.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    (amount.is_finite() && amount >= 0.0).then_some(amount)
}

pub fn extract(text: &str) -> Option<DividendEvidence> {
    let normalized = normalize_text(text);
    let mut candidates = Vec::new();
    for raw in normalized.split('。') {
        let s: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        if !["当期", "当事業年度"].iter().any(|w| s.contains(w))
            || [
                "前期",
                "前年",
                "翌期",
                "翌年度",
                "来期",
                "次期",
                "予想",
                "分割",
                "併合",
                "予定",
                "目標",
                "未定",
                "検討",
                "見送り",
                "中止",
                "撤回",
                "変更",
                "修正",
                "ではなく",
                "とはせず",
            ]
            .iter()
            .any(|w| s.contains(w))
        {
            continue;
        }
        let Some(ordinary) = yen_after(&s, "普通配当") else {
            continue;
        };
        let special = if s.contains("特別配当") {
            yen_after(&s, "特別配当")
        } else {
            Some(0.0)
        };
        let commemorative = if s.contains("記念配当") {
            yen_after(&s, "記念配当")
        } else {
            Some(0.0)
        };
        let (Some(special), Some(commemorative)) = (special, commemorative) else {
            continue;
        };
        let total = ["年間配当金", "年間配当", "期末配当金", "期末配当"]
            .iter()
            .find_map(|label| yen_after(&s, label));
        let Some(total) = total else {
            continue;
        };
        if (total - ordinary - special - commemorative).abs() > 1e-6 {
            continue;
        }
        candidates.push(DividendEvidence {
            total,
            special,
            commemorative,
            phrase: raw.trim().into(),
        });
    }
    let first = candidates.first()?;
    if candidates.iter().any(|c| {
        c.total != first.total
            || c.special != first.special
            || c.commemorative != first.commemorative
    }) {
        return None;
    }
    Some(first.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_current_period_breakdown() {
        let b = extract("当期の配当金は、１株当たりの期末配当金を８５８円（普通配当１９８円、特別配当６６０円）としました。").unwrap();
        assert_eq!((b.total, b.special, b.commemorative), (858.0, 660.0, 0.0));
        assert_eq!(
            extract("当事業年度の年間配当金は100円（普通配当80円、記念配当20円）です。")
                .unwrap()
                .commemorative,
            20.0
        );
    }
    #[test]
    fn never_guess_missing_ambiguous_or_future_amounts() {
        for text in [
            "当期の年間配当金は100円です。",
            "前期および当期の年間配当金は100円（普通配当80円、特別配当20円）です。",
            "当期の年間配当金は100円（普通配当80円、特別配当20円）の予想です。",
            "当期の年間配当金は100円（普通配当80円、特別配当10円）です。",
            "当期の年間配当金は100円50銭（普通配当80円、特別配当20円）です。",
            "当期の年間配当金は100円（普通配当80円、特別配当20円）。当期の年間配当金は200円（普通配当180円、特別配当20円）。",
        ] { assert!(extract(text).is_none(), "{text}"); }
    }
}
