//! AIレコメンド（F7）の純粋な部分: ルールで候補を絞ることと、AI の答えを検証すること。
//! 数字の計算はここ（と indicators / analysis）で済ませ、AI には説明と優先順位付けだけを任せる。

use crate::portfolio::{PAYOUT_WARN, PBR_WARN};

/// AI に渡す候補の上限
pub const MAX_CANDIDATES: usize = 50;
/// AI に返させる（画面に出す）銘柄数
pub const PICK_COUNT: usize = 10;
/// 壊れた答えを捨てたあと、保存するのに最低限ほしい銘柄数
pub const MIN_PICKS: usize = 5;
/// これ以上の配当利回りは候補にしない。特別配当や株価の急落で一時的に高く見えていることが多く、
/// 「増配が期待できて割安」とは別の話になるため（2026-10-22 の本番では 10.4% の銘柄が1位になった）
pub const MAX_YIELD: f64 = 0.08;
/// 理由の長さの上限（文字数）。長すぎる答えは壊れているとみなす
pub const MAX_REASON_CHARS: usize = 200;

/// 候補の判定に使う指標（一覧の1行から必要な分だけ）。判定できない値は None
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub code: String,
    pub dividend_yield: Option<f64>,
    pub is_value: Option<bool>,
    pub dividend_every_year: Option<bool>,
    pub no_loss_years: Option<bool>,
    pub no_quarterly_loss: Option<bool>,
    pub profit_uptrend: Option<bool>,
    pub payout_ratio: Option<f64>,
    pub pbr: Option<f64>,
}

impl Candidate {
    /// ルールに合うか: 利回りが分かっていて MAX_YIELD 未満、バリュー株、または「5年連続で配当あり かつ 通期で赤字なし」。
    /// 保有していれば売りサイン（配当性向50%超・PBR1.5倍超。portfolio と同じ基準）が出る銘柄はすすめない
    pub fn is_eligible(&self) -> bool {
        let yield_known = self
            .dividend_yield
            .is_some_and(|y| y > 0.0 && y < MAX_YIELD);
        let steady = self.dividend_every_year == Some(true) && self.no_loss_years == Some(true);
        let sell_signal = self.payout_ratio.is_some_and(|p| p > PAYOUT_WARN)
            || self.pbr.is_some_and(|p| p > PBR_WARN);
        yield_known && (self.is_value == Some(true) || steady) && !sell_signal
    }

    /// 満たしている条件の数（候補が多いときに上から取るための目安）
    pub fn score(&self) -> u8 {
        [
            self.is_value,
            self.dividend_every_year,
            self.no_loss_years,
            self.no_quarterly_loss,
            self.profit_uptrend,
        ]
        .iter()
        .filter(|f| **f == Some(true))
        .count() as u8
    }
}

/// ルールで候補を絞る。条件を多く満たす順、同点なら利回りの高い順、さらに同点なら銘柄コード順で、
/// 上から MAX_CANDIDATES 件を返す
pub fn select_candidates(rows: &[Candidate]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = rows.iter().filter(|c| c.is_eligible()).cloned().collect();
    out.sort_by(|a, b| {
        b.score()
            .cmp(&a.score())
            .then_with(|| {
                b.dividend_yield
                    .unwrap_or(0.0)
                    .total_cmp(&a.dividend_yield.unwrap_or(0.0))
            })
            .then_with(|| a.code.cmp(&b.code))
    });
    out.truncate(MAX_CANDIDATES);
    out
}

/// AI が返した1件（検証前）
#[derive(Debug, Clone, PartialEq)]
pub struct RawPick {
    pub code: String,
    pub reason: String,
}

/// 検証済みの1件
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    /// 1始まり
    pub rank: u32,
    pub code: String,
    pub reason: String,
}

/// AI の答えのうち、捨てた1件の理由
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickError {
    /// 候補にない銘柄
    UnknownCode(String),
    /// 同じ銘柄が2回（2回目以降を捨てる）
    Duplicate(String),
    /// 理由が空、または長すぎる
    BadReason(String),
    /// PICK_COUNT を超えた分
    Extra(String),
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::UnknownCode(c) => write!(f, "code {c} is not a candidate"),
            PickError::Duplicate(c) => write!(f, "code {c} appears twice"),
            PickError::BadReason(c) => write!(f, "bad reason for {c}"),
            PickError::Extra(c) => write!(f, "code {c} is beyond {PICK_COUNT} picks"),
        }
    }
}

/// 検証の結果。picks は順位順（1始まりで詰める）、dropped は捨てた分
#[derive(Debug, Clone, PartialEq)]
pub struct Validated {
    pub picks: Vec<Pick>,
    pub dropped: Vec<PickError>,
}

/// 正しい答えが MIN_PICKS に届かず、保存しない
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TooFewPicks {
    pub kept: usize,
    pub dropped: Vec<PickError>,
}

impl std::fmt::Display for TooFewPicks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "only {} valid picks (need {MIN_PICKS})", self.kept)?;
        for d in &self.dropped {
            write!(f, "; {d}")?;
        }
        Ok(())
    }
}

/// AI の答えを検証して順位を付ける。候補にない銘柄・重複・空や長すぎる理由・PICK_COUNT を超えた分は
/// その1件だけ捨て、残りを AI の並び順のまま詰めて順位にする。残りが MIN_PICKS 未満なら保存しない
pub fn validate_picks(raw: &[RawPick], candidates: &[Candidate]) -> Result<Validated, TooFewPicks> {
    let mut picks: Vec<Pick> = Vec::with_capacity(PICK_COUNT);
    let mut dropped = Vec::new();
    for p in raw {
        let code = p.code.trim();
        if !candidates.iter().any(|c| c.code == code) {
            dropped.push(PickError::UnknownCode(code.to_string()));
            continue;
        }
        if picks.iter().any(|q| q.code == code) {
            dropped.push(PickError::Duplicate(code.to_string()));
            continue;
        }
        let reason: String = p.reason.split_whitespace().collect::<Vec<_>>().join(" ");
        if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            dropped.push(PickError::BadReason(code.to_string()));
            continue;
        }
        if picks.len() == PICK_COUNT {
            dropped.push(PickError::Extra(code.to_string()));
            continue;
        }
        picks.push(Pick {
            rank: picks.len() as u32 + 1,
            code: code.to_string(),
            reason,
        });
    }
    if picks.len() < MIN_PICKS {
        return Err(TooFewPicks {
            kept: picks.len(),
            dropped,
        });
    }
    Ok(Validated { picks, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(code: &str, y: Option<f64>, value: bool, every: bool, no_loss: bool) -> Candidate {
        Candidate {
            code: code.into(),
            dividend_yield: y,
            is_value: Some(value),
            dividend_every_year: Some(every),
            no_loss_years: Some(no_loss),
            no_quarterly_loss: None,
            profit_uptrend: None,
            payout_ratio: None,
            pbr: None,
        }
    }

    #[test]
    fn value_or_steady_dividend_is_eligible() {
        assert!(cand("1001", Some(0.03), true, false, false).is_eligible());
        assert!(cand("1002", Some(0.03), false, true, true).is_eligible());
        assert!(!cand("1003", Some(0.03), false, true, false).is_eligible());
        assert!(!cand("1004", Some(0.03), false, false, true).is_eligible());
        assert!(
            !cand("1005", None, true, true, true).is_eligible(),
            "利回りが分からなければ候補にしない"
        );
        assert!(!cand("1006", Some(0.0), true, true, true).is_eligible());
        assert!(cand("1008", Some(0.0799), true, true, true).is_eligible());
        let mut high_payout = cand("1010", Some(0.04), true, true, true);
        high_payout.payout_ratio = Some(0.51);
        assert!(
            !high_payout.is_eligible(),
            "配当性向50%超は売りサインなのですすめない"
        );
        high_payout.payout_ratio = Some(0.5);
        assert!(high_payout.is_eligible(), "ちょうど50%は売りサインではない");
        let mut expensive = cand("1011", Some(0.04), false, true, true);
        expensive.pbr = Some(1.6);
        assert!(!expensive.is_eligible(), "PBR1.5倍超は売りサイン");
        assert!(
            !cand("1009", Some(MAX_YIELD), true, true, true).is_eligible(),
            "利回りが高すぎる銘柄は特別配当や株価の急落を疑い、候補にしない"
        );
        let unknown = Candidate {
            code: "1007".into(),
            dividend_yield: Some(0.03),
            is_value: None,
            dividend_every_year: None,
            no_loss_years: None,
            no_quarterly_loss: None,
            profit_uptrend: None,
            payout_ratio: None,
            pbr: None,
        };
        assert!(!unknown.is_eligible(), "5年分の判定がなければ候補にしない");
    }

    #[test]
    fn selection_orders_by_score_then_yield_and_caps() {
        let mut rows = vec![
            cand("2001", Some(0.02), true, false, false),  // 1条件
            cand("2002", Some(0.05), false, true, true),   // 2条件
            cand("2003", Some(0.04), true, true, true),    // 3条件
            cand("2004", Some(0.05), true, false, false),  // 1条件、利回り高い
            cand("2005", Some(0.09), false, false, false), // 対象外
        ];
        for i in 0..60 {
            rows.push(cand(
                &format!("{}", 3000 + i),
                Some(0.01),
                true,
                false,
                false,
            ));
        }
        let picked = select_candidates(&rows);
        assert_eq!(picked.len(), MAX_CANDIDATES);
        let head: Vec<&str> = picked.iter().take(4).map(|c| c.code.as_str()).collect();
        assert_eq!(head, vec!["2003", "2002", "2004", "2001"]);
        assert!(picked.iter().all(|c| c.code != "2005"));
    }

    fn raw(code: &str, reason: &str) -> RawPick {
        RawPick {
            code: code.into(),
            reason: reason.into(),
        }
    }

    fn ten_candidates() -> Vec<Candidate> {
        (0..10)
            .map(|i| cand(&format!("{}", 4000 + i), Some(0.03), true, true, true))
            .collect()
    }

    #[test]
    fn validates_ten_distinct_candidates_with_reasons() {
        let c = ten_candidates();
        let raw: Vec<RawPick> = c
            .iter()
            .map(|x| raw(&format!(" {} ", x.code), "  割安で\n増配が続く  "))
            .collect();
        let v = validate_picks(&raw, &c).unwrap();
        assert!(v.dropped.is_empty());
        let picks = v.picks;
        assert_eq!(picks.len(), 10);
        assert_eq!(picks[0].rank, 1);
        assert_eq!(picks[9].rank, 10);
        assert_eq!(picks[0].code, "4000", "前後の空白は取る");
        assert_eq!(
            picks[0].reason, "割安で 増配が続く",
            "改行や連続した空白は1つにする"
        );
    }

    #[test]
    fn drops_only_the_broken_picks_and_renumbers() {
        let c = ten_candidates();
        let mut ans: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        ans[1].code = "9999".into(); // 候補にない
        ans[3].code = "4000".into(); // 重複（2回目を捨てる）
        ans[5].reason = " \n ".into(); // 理由が空
        ans[7].reason = "あ".repeat(MAX_REASON_CHARS + 1); // 理由が長すぎる
        let v = validate_picks(&ans, &c).unwrap();
        let codes: Vec<&str> = v.picks.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(codes, vec!["4000", "4002", "4004", "4006", "4008", "4009"]);
        let ranks: Vec<u32> = v.picks.iter().map(|p| p.rank).collect();
        assert_eq!(ranks, vec![1, 2, 3, 4, 5, 6], "捨てた分は詰めて順位にする");
        assert_eq!(
            v.dropped,
            vec![
                PickError::UnknownCode("9999".into()),
                PickError::Duplicate("4000".into()),
                PickError::BadReason("4005".into()),
                PickError::BadReason("4007".into()),
            ]
        );
    }

    #[test]
    fn keeps_at_most_ten_and_needs_at_least_five() {
        let c: Vec<Candidate> = (0..12)
            .map(|i| cand(&format!("{}", 4000 + i), Some(0.03), true, true, true))
            .collect();
        let all: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        let v = validate_picks(&all, &c).unwrap();
        assert_eq!(v.picks.len(), PICK_COUNT);
        assert_eq!(
            v.dropped,
            vec![
                PickError::Extra("4010".into()),
                PickError::Extra("4011".into())
            ]
        );

        assert_eq!(
            validate_picks(&all[..MIN_PICKS], &c).unwrap().picks.len(),
            5
        );

        let mut few = all[..MIN_PICKS].to_vec();
        few[0].code = "9999".into();
        assert_eq!(
            validate_picks(&few, &c),
            Err(TooFewPicks {
                kept: MIN_PICKS - 1,
                dropped: vec![PickError::UnknownCode("9999".into())],
            })
        );
        assert_eq!(
            validate_picks(&[], &c),
            Err(TooFewPicks {
                kept: 0,
                dropped: vec![]
            })
        );
    }
}
