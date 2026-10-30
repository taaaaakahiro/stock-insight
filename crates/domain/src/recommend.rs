//! AIレコメンド（F7）の純粋な部分: ルールで候補を絞って3段階の評価を付け、上位 RERANK_POOL 件を
//! AI に渡して並べ替えと理由を受け取り、検証すること。候補の母集団と評価はルールで決めるが、
//! その中での最終的な選択と順位は AI の返答順を採用する。

use crate::portfolio::{PAYOUT_WARN, PBR_WARN};

/// 画面に出す銘柄数。AI は候補の中からこの件数まで選んで順位を付ける
pub const PICK_COUNT: usize = 10;
/// AI に並べ替えと理由を依頼する、ルール順位の上位候補数（選択肢の母集団）
pub const RERANK_POOL: usize = 20;
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
    pub quality_score: u8,
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
    /// 直近の四半期単独の営業利益の前年同期比、直近4四半期合計の営業利益の伸び、利益が一過性の疑い
    pub op_growth_q: Option<f64>,
    pub op_growth_ttm: Option<f64>,
    pub profit_one_off: Option<bool>,
    /// 管理者が登録した注目テーマに当たっている（有報の事業の内容にテーマの言葉がある）
    pub themed: bool,
}

/// 3段階の評価。乖離が大きく業績の質も良ければ強く推奨、一律に「おすすめ」としない
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Grade {
    /// 注目: 割安で増配も期待できるが、直近が減益か、一過性の疑いがあるか、業績の判定材料が足りない
    Watch,
    /// 推奨: 業績が明らかに悪くなく、増配の根拠（乖離・連続増配・累進配当）がある
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

    /// 通期で赤字がなく、一過性の疑いがない
    fn clean_earnings(&self) -> bool {
        self.no_loss_years == Some(true) && self.profit_one_off != Some(true)
    }

    /// 直近の勢い: 直近の四半期の営業利益が前年同期比プラス。四半期の値が分からないときだけ5年の増益傾向で代用する
    /// （直近が減益と分かっているのに、5年の傾きだけで増益とはみなさない）
    fn recent_growing(&self) -> bool {
        match self.op_growth_q {
            Some(g) => g > 0.0,
            None => self.profit_uptrend == Some(true),
        }
    }

    /// 1年を通して減益でない: 直近4四半期の合計が前年を下回らない（分からなければ問わない）
    fn year_not_down(&self) -> bool {
        self.op_growth_ttm.is_none_or(|t| t >= 0.0)
    }

    /// 業績の質（強く推奨の条件）: 赤字なし・一過性でない・直近の四半期が増益・1年を通しても減益でない
    pub fn solid_earnings(&self) -> bool {
        self.clean_earnings() && self.recent_growing() && self.year_not_down()
    }

    /// 業績が明らかに悪くない（推奨の条件）: 赤字なし・一過性でない・1年を通して減益でなく、
    /// 直近の四半期か4四半期の合計のどちらかは増益
    pub fn fair_earnings(&self) -> bool {
        self.clean_earnings()
            && self.year_not_down()
            && (self.recent_growing() || self.op_growth_ttm.is_some_and(|t| t > 0.0))
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
        } else if self.raise_basis() && self.fair_earnings() {
            Grade::Recommend
        } else {
            Grade::Watch
        }
    }
}

/// ルールで候補を絞り、順位をつける。評価の高い順、同じ評価なら注目テーマに当たる銘柄 → 下限割れ →
/// 増配の余地の大きい順 → 条件を多く満たす順 → 利回りの高い順 → 銘柄コード順。
/// テーマは評価を変えない（流行りだけで強く推奨にはしない）が、同じ評価の中では先に出す
pub fn select_candidates(rows: &[Candidate]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = rows.iter().filter(|c| c.is_eligible()).cloned().collect();
    out.sort_by(|a, b| {
        b.grade()
            .cmp(&a.grade())
            .then_with(|| b.quality_score.cmp(&a.quality_score))
            .then_with(|| b.themed.cmp(&a.themed))
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
    /// 候補（ルールで絞った母集団）にないコードを返した
    NotCandidate(String),
    /// 理由が空、または長すぎる
    BadReason(String),
}

impl std::fmt::Display for PickError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PickError::NotCandidate(c) => write!(f, "not a candidate: {c}"),
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

/// AI が返した並びをそのまま公開順位にする。候補（ルールで絞った `pool`）にないコード、理由が空・
/// 長すぎる分はその1件だけ捨て、残りを返答順に PICK_COUNT まで詰める。AI が返さなかった候補は
/// 「選ばれなかった」だけで落とした分には数えない。残りが MIN_PICKS 未満なら保存しない
pub fn validate_reranked(raw: &[RawPick], pool: &[Candidate]) -> Result<Validated, TooFewPicks> {
    let mut out: Vec<Pick> = Vec::with_capacity(PICK_COUNT);
    let mut dropped = Vec::new();
    for p in raw {
        let code = p.code.trim();
        if out.iter().any(|x| x.code == code) {
            continue; // 採用済みのコードの2回目以降は無視
        }
        let Some(c) = pool.iter().find(|c| c.code == code) else {
            dropped.push(PickError::NotCandidate(code.into()));
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
        if out.len() == PICK_COUNT {
            break;
        }
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
    fn financial_quality_orders_within_grade_without_promoting_grade() {
        let a = Candidate {
            code: "1111".into(),
            dividend_yield: Some(0.03),
            is_value: Some(true),
            ..Default::default()
        };
        let mut b = a.clone();
        b.code = "2222".into();
        b.quality_score = 10;
        assert_eq!(a.grade(), b.grade());
        assert_eq!(select_candidates(&[a, b])[0].code, "2222");
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

    /// 業績の質を満たす候補（通期黒字・直近四半期 +10%・4四半期合計 +5%・一過性なし）
    fn solid(code: &str) -> Candidate {
        Candidate {
            op_growth_q: Some(0.1),
            op_growth_ttm: Some(0.05),
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
        // 直近四半期が減益なら、5年の増益傾向があっても強く推奨にはしない。4四半期合計がプラスなら推奨
        let recent_down = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            op_growth_q: Some(-0.13),
            op_growth_ttm: Some(0.15),
            profit_uptrend: Some(true),
            ..solid("2008")
        };
        assert_eq!(recent_down.grade(), Grade::Recommend, "7241 のような形");
        // 直近も4四半期合計も減益なら、下限割れでも注目どまり
        let both_down = Candidate {
            op_growth_ttm: Some(-0.14),
            op_growth_q: Some(-0.24),
            ..recent_down.clone()
        };
        assert_eq!(both_down.grade(), Grade::Watch, "1377 のような形");
        // 直近が減益で4四半期合計が分からなければ、明らかに悪くないとは言えない
        let recent_down_only = Candidate {
            op_growth_ttm: None,
            op_growth_q: Some(-0.47),
            ..recent_down.clone()
        };
        assert_eq!(recent_down_only.grade(), Grade::Watch, "1798 のような形");
        // 四半期の値がなく5年の傾きは上向きでも、4四半期合計が減益なら注目
        let year_down = Candidate {
            op_growth_q: None,
            op_growth_ttm: Some(-0.19),
            profit_uptrend: Some(true),
            ..recent_down.clone()
        };
        assert_eq!(year_down.grade(), Grade::Watch, "9633 のような形");
        // 四半期の値がなければ5年の増益傾向で代用する（4四半期合計がプラス）
        let trend_only = Candidate {
            op_growth_q: None,
            op_growth_ttm: Some(0.9),
            profit_uptrend: Some(true),
            ..recent_down.clone()
        };
        assert_eq!(trend_only.grade(), Grade::Strong, "7022 のような形");
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
        // 業績の判定材料（四半期の値・4四半期合計・5年の傾き）がなければ注目
        let unknown = Candidate {
            policy_floor_breach: true,
            policy_upside: Some(0.3),
            op_growth_q: None,
            op_growth_ttm: None,
            profit_uptrend: None,
            ..solid("2010")
        };
        assert_eq!(unknown.grade(), Grade::Watch);
    }

    #[test]
    fn theme_moves_up_within_the_same_grade_only() {
        let themed_watch = Candidate {
            themed: true,
            ..cand("5001", Some(0.02), true, false, false)
        };
        let plain_recommend = Candidate {
            consecutive_increases: Some(3),
            ..solid("5002")
        };
        let themed_recommend = Candidate {
            themed: true,
            consecutive_increases: Some(2),
            ..solid("5003")
        };
        let all = select_candidates(&[themed_watch, plain_recommend, themed_recommend]);
        let codes: Vec<&str> = all.iter().map(|c| c.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["5003", "5002", "5001"],
            "テーマは同じ評価の中でだけ先"
        );
        assert_eq!(all[2].grade(), Grade::Watch, "テーマで評価は上がらない");
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

    /// AI に渡す母集団（ルール上位 RERANK_POOL 件に見立てた20候補）
    fn rerank_pool() -> Vec<Candidate> {
        (0..RERANK_POOL)
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
    fn ai_order_becomes_the_published_rank() {
        // ルール上位20件の母集団から、AI が返した順が公開順位になる
        let c = rerank_pool();
        let mut ans: Vec<RawPick> = vec![raw("9999", "候補にない銘柄は選べない")];
        ans.extend(
            c.iter()
                .take(PICK_COUNT)
                .rev()
                .map(|x| raw(&format!(" {} ", x.code), "  割安で\n増配が続く  ")),
        );
        let v = validate_reranked(&ans, &c).unwrap();
        assert_eq!(v.dropped, vec![PickError::NotCandidate("9999".into())]);
        let codes: Vec<&str> = v.picks.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(
            codes[0], "4009",
            "AI の返答順が公開順位。前後の空白は取って突き合わせる"
        );
        assert_eq!(codes[9], "4000");
        assert_eq!(v.picks[0].rank, 1);
        assert_eq!(v.picks[9].rank, 10);
        assert_eq!(v.picks[0].grade, Grade::Watch);
        assert_eq!(
            v.picks[0].reason, "割安で 増配が続く",
            "改行や連続した空白は1つにする"
        );
    }

    #[test]
    fn ai_selects_within_the_pool_and_drops_broken_reasons() {
        let c = rerank_pool();
        let mut ans: Vec<RawPick> = c
            .iter()
            .take(PICK_COUNT)
            .map(|x| raw(&x.code, "r"))
            .collect();
        ans[2].reason = " \n ".into(); // 4002 の理由が空
        ans[4].reason = "あ".repeat(MAX_REASON_CHARS + 1); // 4004 が長すぎる
        ans.push(raw(&c[12].code, "r")); // 後ろの順位の候補も選べる
        ans.push(raw("9999", "候補外"));
        ans.push(raw(&c[0].code, "2回目は無視"));
        let v = validate_reranked(&ans, &c).unwrap();
        let codes: Vec<&str> = v.picks.iter().map(|p| p.code.as_str()).collect();
        assert_eq!(
            codes,
            vec!["4000", "4001", "4003", "4005", "4006", "4007", "4008", "4009", "4012"]
        );
        let ranks: Vec<u32> = v.picks.iter().map(|p| p.rank).collect();
        assert_eq!(
            ranks,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9],
            "捨てた分は詰めて順位にする"
        );
        assert_eq!(
            v.dropped,
            vec![
                PickError::BadReason("4002".into()),
                PickError::BadReason("4004".into()),
                PickError::NotCandidate("9999".into()),
            ],
            "AI が返さなかった候補は落とした分に数えない"
        );
    }

    #[test]
    fn caps_the_picks_at_ten() {
        let c = rerank_pool();
        let ans: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        let v = validate_reranked(&ans, &c).unwrap();
        assert_eq!(v.picks.len(), PICK_COUNT, "11件目以降は採用しない");
        assert_eq!(v.picks[9].code, "4009");
    }

    #[test]
    fn needs_at_least_five_reasons() {
        let c = rerank_pool();
        let all: Vec<RawPick> = c.iter().map(|x| raw(&x.code, "r")).collect();
        assert_eq!(
            validate_reranked(&all[..MIN_PICKS], &c)
                .unwrap()
                .picks
                .len(),
            5
        );
        assert_eq!(
            validate_reranked(&all[..MIN_PICKS - 1], &c)
                .unwrap_err()
                .kept,
            4
        );
        assert_eq!(
            validate_reranked(&[], &c),
            Err(TooFewPicks {
                kept: 0,
                dropped: vec![]
            })
        );
    }
}
