//! 会社の配当方針（有報に書かれた DOE・配当性向の下限・目標）と、今の配当の乖離。
//! 方針どおりに払えば配当がどれだけ増えるか（増配の余地）を出す。AI レコメンドの評価と、画面の表示に使う。

/// 方針の指標
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMetric {
    Doe,
    Payout,
}

/// 下限（守ると言った線）か目標か
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyLevel {
    Floor,
    Goal,
}

/// 方針と今の配当の乖離。`upside` は 方針の値 ÷ 今の値 − 1（0.2 = 方針どおりなら配当が 20% 増える）
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyGap {
    pub metric: PolicyMetric,
    pub level: PolicyLevel,
    /// 方針の値（比率）
    pub policy: f64,
    /// 今の値（比率）
    pub now: f64,
    pub upside: f64,
}

impl PolicyGap {
    /// API・画面で使う名前
    pub fn key(&self) -> &'static str {
        match (self.metric, self.level) {
            (PolicyMetric::Doe, PolicyLevel::Floor) => "doe_floor",
            (PolicyMetric::Doe, PolicyLevel::Goal) => "doe_goal",
            (PolicyMetric::Payout, PolicyLevel::Floor) => "payout_floor",
            (PolicyMetric::Payout, PolicyLevel::Goal) => "payout_goal",
        }
    }
}

/// 比べる値（比率は小数）。今の値は、会社予想の配当があれば予想で、なければ実績で出したもの
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PolicyInputs {
    pub doe_now: Option<f64>,
    pub doe_floor: Option<f64>,
    pub doe_goal: Option<f64>,
    pub payout_now: Option<f64>,
    pub payout_floor: Option<f64>,
    pub payout_goal: Option<f64>,
}

/// 指標1つ分の乖離。今の値が下限より低ければ下限との差、下限は守っていて目標より低ければ目標との差。
/// 今の値が 0 以下（無配・赤字）なら比べない
fn gap_of(
    metric: PolicyMetric,
    now: Option<f64>,
    floor: Option<f64>,
    goal: Option<f64>,
) -> Option<PolicyGap> {
    let now = now.filter(|v| *v > 0.0)?;
    let (level, policy) = match (floor, goal) {
        (Some(f), _) if now < f => (PolicyLevel::Floor, f),
        (_, Some(g)) if now < g => (PolicyLevel::Goal, g),
        _ => return None,
    };
    Some(PolicyGap {
        metric,
        level,
        policy,
        now,
        upside: policy / now - 1.0,
    })
}

/// DOE と配当性向の乖離のうち、いちばん重いもの。下限を割っているものを目標に届かないものより先にし、
/// 同じ段なら増配の余地が大きいほう。どちらも方針がないか、方針を満たしていれば None
pub fn dividend_upside(i: &PolicyInputs) -> Option<PolicyGap> {
    let gaps = [
        gap_of(PolicyMetric::Doe, i.doe_now, i.doe_floor, i.doe_goal),
        gap_of(
            PolicyMetric::Payout,
            i.payout_now,
            i.payout_floor,
            i.payout_goal,
        ),
    ];
    gaps.into_iter().flatten().max_by(|a, b| {
        let rank = |g: &PolicyGap| matches!(g.level, PolicyLevel::Floor) as u8;
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.upside.total_cmp(&b.upside))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn floor_breach_comes_before_goal_shortfall() {
        // DOE は下限 3.5% に対して 2.8%（下限割れ、余地 25%）、配当性向は目標 40% に対して 20%（余地 100%）
        let g = dividend_upside(&PolicyInputs {
            doe_now: Some(0.028),
            doe_floor: Some(0.035),
            doe_goal: Some(0.04),
            payout_now: Some(0.2),
            payout_floor: None,
            payout_goal: Some(0.4),
        })
        .unwrap();
        assert_eq!((g.metric, g.level), (PolicyMetric::Doe, PolicyLevel::Floor));
        assert_eq!(g.key(), "doe_floor");
        assert!(close(g.upside, 0.25), "{}", g.upside);
    }

    #[test]
    fn goal_is_used_once_the_floor_is_met() {
        let g = dividend_upside(&PolicyInputs {
            doe_now: Some(0.036),
            doe_floor: Some(0.035),
            doe_goal: Some(0.045),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.key(), "doe_goal");
        assert!(close(g.upside, 0.25), "{}", g.upside);
    }

    #[test]
    fn same_level_takes_the_larger_upside() {
        let g = dividend_upside(&PolicyInputs {
            doe_now: Some(0.03),
            doe_goal: Some(0.033),
            payout_now: Some(0.25),
            payout_goal: Some(0.35),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(g.key(), "payout_goal");
        assert!(close(g.upside, 0.4), "{}", g.upside);
    }

    #[test]
    fn no_gap_when_policy_is_met_or_missing() {
        assert_eq!(
            dividend_upside(&PolicyInputs {
                doe_now: Some(0.04),
                doe_floor: Some(0.035),
                doe_goal: Some(0.04),
                ..Default::default()
            }),
            None,
            "目標に届いていれば乖離なし"
        );
        assert_eq!(dividend_upside(&PolicyInputs::default()), None);
        assert_eq!(
            dividend_upside(&PolicyInputs {
                doe_now: Some(0.03),
                ..Default::default()
            }),
            None,
            "方針の数字がない"
        );
        assert_eq!(
            dividend_upside(&PolicyInputs {
                doe_now: Some(0.0),
                doe_floor: Some(0.03),
                ..Default::default()
            }),
            None,
            "無配は比べない"
        );
        assert_eq!(
            dividend_upside(&PolicyInputs {
                payout_now: Some(0.3),
                payout_floor: Some(0.3),
                ..Default::default()
            }),
            None,
            "ちょうど下限は割れていない"
        );
    }
}
