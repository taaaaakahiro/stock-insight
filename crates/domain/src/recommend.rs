//! AIレコメンド（F7）の純粋な部分: ルールで候補を絞り、3段階の評価と順位を決め、AI が書いた理由を検証すること。
//! 数字の計算と評価はここ（と indicators / analysis / dividend_policy）で済ませ、AI には理由の文章だけを任せる。

use crate::portfolio::{PAYOUT_WARN, PBR_WARN};

/// 画面に出す（AI に理由を書かせる）銘柄数
pub const PICK_COUNT: usize = 10;
/// 壊れた答えを捨てたあと、保存するのに最低限ほしい銘柄数
pub const MIN_PICKS: usize = 5;
/// これ以上の配当利回りは候補にしない。特別配当や株価の急落で一時的に高く見えていることが多く、
/// 「増配が期待できて割安」とは別の話になるため（2026-10-22 の本番では 10.4% の銘柄が1位になった）
pub const MAX_YIELD: f64 = 0.08;
/// 理由の長さの上限（文字数）。長すぎる答えは壊れているとみなす
pub const MAX_REASON_CHARS: usize = 200;
/// 方針との乖離（増配の余地）がこれ以上なら「大きい」とみなし、強く推奨の材料にする（0.2 = 方針どおりなら配当 +20%）
pub const STRONG_UPSIDE: f64 = 0.2;

/// 候補の判定に使う指標（一覧の1行から必要な分だけ）。判定できない値は None
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Candidate {
    pub code: String,
    pub dividend_yield: Option<f64>,
    pub is_value: Option<bool>,
    /// 利回りの過去5年比で割安（valuation = cheap）
    pub yield_cheap: bool,
    pub dividend_every_year: Option<bool>,
    pub no_loss_years: Option<bool>,
    pub no_quarterly_loss: Option<bool>,
    pub profit_uptrend: Option<bool>,
    pub payout_ratio: Option<f64>,
    pub pbr: Option<f64>,
    pub dividend_cut: Option<bool>,
    pub consecutive_increases: Option<i64>,
    pub progressive_dividend: Option<bool>,
    /// 会社の配当方針（DOE・配当性向の下限・目標）との乖離: 方針どおりなら配当が何 % 増えるか。方針がないか満たしていれば None
    pub policy_upside: Option<f64>,
    /// その乖離が「下限割れ」（会社が守ると言った線を下回っている）
    pub policy_floor_breach: bool,
    /// 直近の四半期単独の営業利益の前年同期比と、利益が一過性の疑い
    pub op_growth_q: Option<f64>,
    pub profit_one_off: Option<bool>,
}

/// 3段階の評価。乖離が大きく業績の質も良ければ強く推奨、一律に「おすすめ」としない
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    /// 注目: 割安で増配も期待できるが、一過性の疑いがあるか、業績の判定材料が足りない
    Watch,
    /// 推奨: 業績の質を満たし、増配の根拠（乖離・連続増配・累進配当）がある
    Recommend,
    /// 強く推奨: 方針との乖離が大きい（下限割れ、または余地 STRONG_UPSIDE 以上）＋ 業績の質 ＋ 割安
    Strong,
}

impl Grade {
    pub fn as_str(self) -> &'static str {
        match self {
            Grade::Strong => "strong",
            Grade::Recommend => "recommend",
            Grade::Watch => "watch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "strong" => Grade::Strong,
            "recommend" => Grade::Recommend,
            "watch" => Grade::Watch,
            _ => return None,
        })
    }
}

impl Candidate {
    /// ルールに合うか: 利回りが分かっていて MAX_YIELD 未満、バリュー株、または「5年連続で配当あり かつ 通期で赤字なし」。
    /// 保有していれば売りサイン（配当性向50%超・PBR1.5倍超。portfolio と同じ基準）が出る銘柄と、減配した銘柄はすすめない
    pub fn is_eligible(&self) -> bool {
        let yield_known = self
            .dividend_yield
            .is_some_and(|y| y > 0.0 && y < MAX_YIELD);
        let steady = self.dividend_every_year == Some(true) && self.no_loss_years == Some(true);
        let sell_signal = self.payout_ratio.is_some_and(|p| p > PAYOUT_WARN)
            || self.pbr.is_some_and(|p| p > PBR_WARN);
        yield_known
            && (self.is_value == Some(true) || steady)
            && !sell_signal
            && self.dividend_cut != Some(true)
    }

    /// 満たしている条件の数（同じ評価の中で上から取るための目安）
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

    /// 業績の質: 通期で赤字がなく、一過性の疑いがなく、増益（直近の四半期の営業利益が前年同期比プラス、または増益傾向）
    pub fn solid_earnings(&self) -> bool {
        let growing =
            self.op_growth_q.is_some_and(|g| g > 0.0) || self.profit_uptrend == Some(true);
        self.no_loss_years == Some(true) && self.profit_one_off != Some(true) && growing
    }

    /// 割安: バリュー株、または利回りの過去5年比で割安
    pub fn cheap(&self) -> bool {
        self.is_value == Some(true) || self.yield_cheap
    }

    /// 方針との乖離が大きい: 下限割れ、または増配の余地が STRONG_UPSIDE 以上
    pub fn big_gap(&self) -> bool {
        self.policy_floor_breach || self.policy_upside.is_some_and(|u| u >= STRONG_UPSIDE)
    }

    /// 増配の根拠: 方針との乖離がある、2年以上の連続増配、累進配当の方針
    pub fn raise_basis(&self) -> bool {
        self.policy_upside.is_some_and(|u| u > 0.0)
            || self.consecutive_increases.is_some_and(|n| n >= 2)
            || self.progressive_dividend == Some(true)
    }

    pub fn grade(&self) -> Grade {
        if self.big_gap() && self.solid_earnings() && self.cheap() {
            Grade::Strong
        } else if self.raise_basis() && self.solid_earnings() {
            Grade::Recommend
        } else {
            Grade::Watch
        }
    }
}

/// ルールで候補を絞り、順位をつける。評価の高い順、同じ評価なら下限割れ → 増配の余地の大きい順 → 条件を多く満たす順
/// → 利回りの高い順 → 銘柄コード順
pub fn select_candidates(rows: &[Candidate]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = rows.iter().filter(|c| c.is_eligible()).cloned().collect();
    out.sort_by(|a, b| {
        b.grade()
            .cmp(&a.grade())
            .then_with(|| b.policy_floor_breach.cmp(&a.policy_floor_breach))
            .then_with(|| {
                b.policy_upside
                    .unwrap_or(0.0)
                    .total_cmp(&a.policy_upside.unwrap_or(0.0))
            })
            .then_with(|| b.score().cmp(&a.score()))
            .then_with(|| {
                b.dividend_yield
                    .unwrap_or(0.0)
                    .total_cmp(&a.dividend_yield.unwrap_or(0.0))
            })
            .then_with(|| a.code.cmp(&b.code))
    });
    out
}

/// 上位 PICK_COUNT 件（AI に理由を書かせ、画面に出す分）
pub fn select_picks(rows: &[Candidate]) -> Vec<Candidate> {
    let mut out = select_candidates(rows);
    out.truncate(PICK_COUNT);
    out
}

/// AI が返した1件（検証前）: 銘柄コードと理由
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
    pub grade: Grade,
    pub reason: String,
}

/// AI の答えのうち、捨てた1件の理由
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickError {
    /// こちらが選んだ銘柄の理由が返ってこなかった
    MissingReason(String),
    /// 理由が空、または長すぎる
    BadReason(String),
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::MissingReason(c) => write!(f, "no reason for {c}"),
            PickError::BadReason(c) => write!(f, "bad reason for {c}"),
        }
    }
}

/// 検証の結果。picks はこちらで決めた順位順（1始まりで詰める）、dropped は捨てた分
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

/// 理由文に出てくる指標の名前。値（数字）を伴わずに出てきたら、渡していない項目に触れている
const METRIC_NAMES: [&str; 9] = [
    "配当性向",
    "配当利回り",
    "利回り",
    "PBR",
    "PER",
    "DOE",
    "営業利益",
    "売上高",
    "連続増配",
];

/// 理由文から、値を伴わずに指標の名前だけが出てくる節（「配当性向はなし」など）を取り除く。
/// AI には良い点（値つき）しか渡していないので、値のない指標の話は渡していない項目への言及になる。
/// 文の終わりの節を取り除いたときは、残った文が「〜しており」で終わらないよう言い切りに直す
pub fn tidy_reason(reason: &str) -> String {
    let mentions_without_value = |clause: &str| {
        METRIC_NAMES.iter().any(|m| clause.contains(m))
            && !clause
                .chars()
                .any(|c| c.is_ascii_digit() || ('０'..='９').contains(&c))
    };
    let mut out = String::new();
    let mut rest = reason;
    while !rest.is_empty() {
        let (sentence, terminated) = match rest.find('。') {
            Some(i) => {
                let s = &rest[..i];
                rest = &rest[i + '。'.len_utf8()..];
                (s, true)
            }
            None => {
                let s = rest;
                rest = "";
                (s, false)
            }
        };
        let clauses: Vec<&str> = sentence.split('、').collect();
        let kept: Vec<&str> = clauses
            .iter()
            .copied()
            .filter(|c| !mentions_without_value(c))
            .collect();
        if kept.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let mut s = kept.join("、");
        if kept.last() != clauses.last() {
            for (from, to) in [
                ("しており", "している"),
                ("ており", "ている"),
                ("であり", "である"),
            ] {
                if let Some(head) = s.strip_suffix(from) {
                    s = format!("{head}{to}");
                    break;
                }
            }
        }
        out.push_str(&s);
        if terminated {
            out.push('。');
        }
    }
    out
}

/// AI が書いた理由を、こちらで決めた順位の銘柄（`picks`）に当てる。理由が返ってこない・空・長すぎる銘柄は
/// その1件だけ捨て、残りを順位順に詰める。候補にない銘柄の理由は無視する。残りが MIN_PICKS 未満なら保存しない
pub fn validate_picks(raw: &[RawPick], picks: &[Candidate]) -> Result<Validated, TooFewPicks> {
    let mut out: Vec<Pick> = Vec::with_capacity(picks.len());
    let mut dropped = Vec::new();
    for c in picks {
        let Some(p) = raw.iter().find(|p| p.code.trim() == c.code) else {
            dropped.push(PickError::MissingReason(c.code.clone()));
            continue;
        };
        let reason = tidy_reason(&p.reason.split_whitespace().collect::<Vec<_>>().join(" "));
        if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
            dropped.push(PickError::BadReason(c.code.clone()));
            continue;
        }
        out.push(Pick {
            rank: out.len() as u32 + 1,
            code: c.code.clone(),
            grade: c.grade(),
            reason,
        });
    }
    if out.len() < MIN_PICKS {
        return Err(TooFewPicks {
            kept: out.len(),
            dropped,
        });
    }
    Ok(Validated {
        picks: out,
        dropped,
    })
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
            ..Default::default()
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
        let mut cut = cand("1012", Some(0.04), true, true, true);
        cut.dividend_cut = Some(true);
        assert!(!cut.is_eligible(), "減配した銘柄はすすめない");
        assert!(
            !cand("1009", Some(MAX_YIELD), true, true, true).is_eligible(),
            "利回りが高すぎる銘柄は特別配当や株価の急落を疑い、候補にしない"
        );
        let unknown = Candidate {
            code: "1007".into(),
            dividend_yield: Some(0.03),
            ..Default::default()
        };
        assert!(!unknown.is_eligible(), "5年分の判定がなければ候補にしない");
    }

    /// 業績の質を満たす候補（通期黒字・直近四半期 +10%・一過性なし）
    fn solid(code: &str) -> Candidate {
        Candidate {
            op_growth_q: Some(0.1),
            profit_one_off: Some(false),
            ..cand(code, Some(0.035), true, true, true)
        }
    }

    #[test]
    fn grades_by_gap_earnings_and_valuation() {
        // 下限割れ ＋ 業績の質 ＋ 割安（バリュー）→ 強く推奨
        let breach = Candidate {
            policy_upside: Some(0.1),
            policy_floor_breach: true,
            ..solid("2001")
        };
        assert_eq!(breach.grade(), Grade::Strong);
        // 目標までの余地が 20% 以上 ＋ 質 ＋ 割安 → 強く推奨
        let wide = Candidate {
            policy_upside: Some(0.25),
            ..solid("2002")
        };
        assert_eq!(wide.grade(), Grade::Strong);
        // 余地が小さい（10%）→ 推奨
        let narrow = Candidate {
            policy_upside: Some(0.1),
            ..solid("2003")
        };
        assert_eq!(narrow.grade(), Grade::Recommend);
        // 乖離はないが 3年連続増配 → 推奨
        let streak = Candidate {
            consecutive_increases: Some(3),
            ..solid("2004")
        };
        assert_eq!(streak.grade(), Grade::Recommend);
        // 累進配当の方針 → 推奨。1年だけの増配は根拠にしない
        let progressive = Candidate {
            progressive_dividend: Some(true),
            ..solid("2005")
        };
        assert_eq!(progressive.grade(), Grade::Recommend);
        let one_year = Candidate {
            consecutive_increases: Some(1),
            ..solid("2006")
        };
        assert_eq!(one_year.grade(), Grade::Watch, "増配の根拠がない");
        // 乖離が大きくても一過性の疑いがあれば注目どまり
        let one_off = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            profit_one_off: Some(true),
            ..solid("2007")
        };
        assert_eq!(one_off.grade(), Grade::Watch);
        // 直近四半期が減益でも増益傾向があれば質を満たす
        let trend = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            op_growth_q: Some(-0.05),
            profit_uptrend: Some(true),
            ..solid("2008")
        };
        assert_eq!(trend.grade(), Grade::Strong);
        // 乖離が大きく質も良いが割安でない（バリューでも利回り割安でもない）→ 推奨
        let fair = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            is_value: Some(false),
            yield_cheap: false,
            ..solid("2009")
        };
        assert_eq!(fair.grade(), Grade::Recommend);
        let yield_cheap = Candidate {
            yield_cheap: true,
            ..fair.clone()
        };
        assert_eq!(
            yield_cheap.grade(),
            Grade::Strong,
            "利回りの過去5年比で割安でもよい"
        );
        // 業績の判定材料がなければ注目
        let unknown = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            op_growth_q: None,
            profit_uptrend: None,
            ..solid("2010")
        };
        assert_eq!(unknown.grade(), Grade::Watch);
    }

    #[test]
    fn selection_orders_by_grade_then_gap_then_score_and_caps() {
        let mut rows = vec![
            cand("3001", Some(0.02), true, false, false), // 注目、1条件
            Candidate {
                policy_upside: Some(0.1),
                ..solid("3002")
            }, // 推奨、余地 10%
            Candidate {
                policy_upside: Some(0.3),
                ..solid("3003")
            }, // 強く推奨、余地 30%
            Candidate {
                policy_upside: Some(0.1),
                policy_floor_breach: true,
                ..solid("3004")
            }, // 強く推奨、下限割れ（余地は小さいが先）
            Candidate {
                consecutive_increases: Some(3),
                ..solid("3005")
            }, // 推奨、乖離なし
            cand("3006", Some(0.05), true, false, false), // 注目、1条件、利回り高い
            cand("3007", Some(0.09), false, false, false), // 対象外
        ];
        for i in 0..20 {
            rows.push(cand(
                &format!("{}", 4000 + i),
                Some(0.01),
                true,
                false,
                false,
            ));
        }
        let all = select_candidates(&rows);
        let head: Vec<&str> = all.iter().take(6).map(|c| c.code.as_str()).collect();
        assert_eq!(head, vec!["3004", "3003", "3002", "3005", "3006", "3001"]);
        assert!(all.iter().all(|c| c.code != "3007"));
        let picks = select_picks(&rows);
        assert_eq!(picks.len(), PICK_COUNT);
        assert_eq!(picks[0].grade(), Grade::Strong);
    }

    #[test]
    fn grade_round_trips_through_text() {
        for g in [Grade::Strong, Grade::Recommend, Grade::Watch] {
            assert_eq!(Grade::parse(g.as_str()), Some(g));
        }
        assert_eq!(Grade::parse("great"), None);
        assert!(Grade::Strong > Grade::Recommend && Grade::Recommend > Grade::Watch);
    }

    fn raw(code: &str, reason: &str) -> RawPick {
        RawPick {
            code: code.into(),
            reason: reason.into(),
        }
    }

    fn ten_picks() -> Vec<Candidate> {
        (0..10)
            .map(|i| cand(&format!("{}", 4000 + i), Some(0.03), true, true, true))
            .collect()
    }

    #[test]
    fn tidy_reason_drops_metrics_mentioned_without_values() {
        assert_eq!(
            tidy_reason(
                "配当利回りが過去5年で上位1%の高さで割安、PBR 0.77倍で純資産から見て割安、3年連続で増配しており、配当性向はなし。"
            ),
            "配当利回りが過去5年で上位1%の高さで割安、PBR 0.77倍で純資産から見て割安、3年連続で増配している。",
            "2026-10-22 の本番で出た文。最後の節を取り、言い切りに直す"
        );
        assert_eq!(
            tidy_reason("PBR 0.80倍で割安。配当性向は不明。前期に増配した。"),
            "PBR 0.80倍で割安。前期に増配した。",
            "文ごと取り除く"
        );
        let ok = "4年連続で増配しており、配当性向 22.4%で増配の余地がある。利益が増える傾向。";
        assert_eq!(
            tidy_reason(ok),
            ok,
            "値のある指標と、指標でない良い点は残す"
        );
        assert_eq!(
            tidy_reason("配当性向はなし。"),
            "",
            "全部取れたら空（検証で捨てる）"
        );
    }

    #[test]
    fn reasons_are_matched_to_our_ranking() {
        let c = ten_picks();
        // AI の並びは関係なく、こちらの順位で並ぶ
        let mut ans: Vec<RawPick> = c
            .iter()
            .rev()
            .map(|x| raw(&format!(" {} ", x.code), "  割安で\n増配が続く  "))
            .collect();
        ans.push(raw("9999", "候補にない銘柄の理由は無視する"));
        let v = validate_picks(&ans, &c).unwrap();
        assert!(v.dropped.is_empty());
        let codes: Vec<&str> = v.picks.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(codes[0], "4000", "前後の空白は取って突き合わせる");
        assert_eq!(codes[9], "4009");
        assert_eq!(v.picks[0].rank, 1);
        assert_eq!(v.picks[9].rank, 10);
        assert_eq!(v.picks[0].grade, Grade::Watch);
        assert_eq!(
            v.picks[0].reason, "割安で 増配が続く",
            "改行や連続した空白は1つにする"
        );
    }

    #[test]
    fn drops_only_the_broken_reasons_and_renumbers() {
        let c = ten_picks();
        let mut ans: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        ans.remove(1); // 4001 の理由がない
        ans[4].reason = " \n ".into(); // 4005 の理由が空
        ans[6].reason = "あ".repeat(MAX_REASON_CHARS + 1); // 4007 が長すぎる
        let v = validate_picks(&ans, &c).unwrap();
        let codes: Vec<&str> = v.picks.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["4000", "4002", "4003", "4004", "4006", "4008", "4009"]
        );
        let ranks: Vec<u32> = v.picks.iter().map(|p| p.rank).collect();
        assert_eq!(
            ranks,
            vec![1, 2, 3, 4, 5, 6, 7],
            "捨てた分は詰めて順位にする"
        );
        assert_eq!(
            v.dropped,
            vec![
                PickError::MissingReason("4001".into()),
                PickError::BadReason("4005".into()),
                PickError::BadReason("4007".into()),
            ]
        );
    }

    #[test]
    fn needs_at_least_five_reasons() {
        let c = ten_picks();
        let all: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        assert_eq!(
            validate_picks(&all[..MIN_PICKS], &c).unwrap().picks.len(),
            5
        );
        assert_eq!(
            validate_picks(&all[..MIN_PICKS - 1], &c).unwrap_err().kept,
            4
        );
        assert_eq!(
            validate_picks(&[], &c),
            Err(TooFewPicks {
                kept: 0,
                dropped: c
                    .iter()
                    .map(|x| PickError::MissingReason(x.code.clone()))
                    .collect()
            })
        );
    }
}
