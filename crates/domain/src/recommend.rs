//! AIレコメンド（F7）の純粋な部分: ルールで候補を絞ることと、AI の答えを検証すること。
//! 数字の計算はここ（と indicators / analysis）で済ませ、AI には説明と優先順位付けだけを任せる。

/// AI に渡す候補の上限
pub const MAX_CANDIDATES: usize = 50;
/// AI が返す（画面に出す）銘柄数
pub const PICK_COUNT: usize = 10;
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
}

impl Candidate {
    /// ルールに合うか: 利回りが分かっていて、バリュー株、または「5年連続で配当あり かつ 通期で赤字なし」
    pub fn is_eligible(&self) -> bool {
        let yield_known = self.dividend_yield.is_some_and(|y| y > 0.0);
        let steady = self.dividend_every_year == Some(true) && self.no_loss_years == Some(true);
        yield_known && (self.is_value == Some(true) || steady)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickError {
    /// 件数が PICK_COUNT でない
    WrongCount(usize),
    /// 候補にない銘柄
    UnknownCode(String),
    /// 同じ銘柄が2回
    Duplicate(String),
    /// 理由が空、または長すぎる
    BadReason(String),
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::WrongCount(n) => write!(f, "expected {PICK_COUNT} picks, got {n}"),
            PickError::UnknownCode(c) => write!(f, "code {c} is not a candidate"),
            PickError::Duplicate(c) => write!(f, "code {c} appears twice"),
            PickError::BadReason(c) => write!(f, "bad reason for {c}"),
        }
    }
}

/// AI の答えを検証して順位を付ける。候補にない銘柄・重複・空の理由があれば保存しない
pub fn validate_picks(raw: &[RawPick], candidates: &[Candidate]) -> Result<Vec<Pick>, PickError> {
    if raw.len() != PICK_COUNT {
        return Err(PickError::WrongCount(raw.len()));
    }
    let mut out: Vec<Pick> = Vec::with_capacity(PICK_COUNT);
    for (i, p) in raw.iter().enumerate() {
        let code = p.code.trim();
        if !candidates.iter().any(|c| c.code == code) {
            return Err(PickError::UnknownCode(code.to_string()));
        }
        if out.iter().any(|q| q.code == code) {
            return Err(PickError::Duplicate(code.to_string()));
        }
        let reason: String = p.reason.split_whitespace().collect::<Vec<_>>().join(" ");
        if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            return Err(PickError::BadReason(code.to_string()));
        }
        out.push(Pick {
            rank: i as u32 + 1,
            code: code.to_string(),
            reason,
        });
    }
    Ok(out)
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
        let unknown = Candidate {
            code: "1007".into(),
            dividend_yield: Some(0.03),
            is_value: None,
            dividend_every_year: None,
            no_loss_years: None,
            no_quarterly_loss: None,
            profit_uptrend: None,
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
        let picks = validate_picks(&raw, &c).unwrap();
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
    fn rejects_broken_answers() {
        let c = ten_candidates();
        let ok: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();

        assert_eq!(validate_picks(&ok[..9], &c), Err(PickError::WrongCount(9)));

        let mut unknown = ok.clone();
        unknown[3].code = "9999".into();
        assert_eq!(
            validate_picks(&unknown, &c),
            Err(PickError::UnknownCode("9999".into()))
        );

        let mut dup = ok.clone();
        dup[5].code = "4000".into();
        assert_eq!(
            validate_picks(&dup, &c),
            Err(PickError::Duplicate("4000".into()))
        );

        let mut empty = ok.clone();
        empty[0].reason = " \n ".into();
        assert_eq!(
            validate_picks(&empty, &c),
            Err(PickError::BadReason("4000".into()))
        );

        let mut long = ok;
        long[1].reason = "あ".repeat(MAX_REASON_CHARS + 1);
        assert_eq!(
            validate_picks(&long, &c),
            Err(PickError::BadReason("4001".into()))
        );
    }
}
