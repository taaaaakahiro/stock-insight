//! ポートフォリオ（保有銘柄）の売り時のサイン。取得価格は持たず、最新の指標だけで判定する。

/// 配当性向がこれを超えたら、増配の余地が小さい
pub const PAYOUT_WARN: f64 = 0.5;
/// 1人が登録できる銘柄数
pub const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SellSignal {
    /// 配当性向が 50% 超
    HighPayout,
    /// 利回りが過去5年の下位20%（株価が上がって割高）
    Expensive,
    /// 前期から増配していない（据え置き・減配）
    NoIncrease,
    /// 営業利益の傾きが下向き
    ProfitDowntrend,
    /// 直近5期または20四半期に赤字がある
    Loss,
}

impl SellSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            SellSignal::HighPayout => "high_payout",
            SellSignal::Expensive => "expensive",
            SellSignal::NoIncrease => "no_increase",
            SellSignal::ProfitDowntrend => "profit_downtrend",
            SellSignal::Loss => "loss",
        }
    }
}

/// 判定に使う指標（一覧の1行から必要な分だけ）。判定できない値は None で、サインは出さない
#[derive(Debug, Clone, Copy, Default)]
pub struct SignalInput<'a> {
    pub payout_ratio: Option<f64>,
    pub valuation: &'a str,
    pub consecutive_increases: Option<i64>,
    pub profit_uptrend: Option<bool>,
    pub no_loss_years: Option<bool>,
    pub no_quarterly_loss: Option<bool>,
}

pub fn sell_signals(i: &SignalInput) -> Vec<SellSignal> {
    let mut out = vec![];
    if i.payout_ratio.is_some_and(|p| p > PAYOUT_WARN) {
        out.push(SellSignal::HighPayout);
    }
    if i.valuation == crate::Valuation::Expensive.as_str() {
        out.push(SellSignal::Expensive);
    }
    if i.consecutive_increases == Some(0) {
        out.push(SellSignal::NoIncrease);
    }
    if i.profit_uptrend == Some(false) {
        out.push(SellSignal::ProfitDowntrend);
    }
    if i.no_loss_years == Some(false) || i.no_quarterly_loss == Some(false) {
        out.push(SellSignal::Loss);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_stock_has_no_signal() {
        let i = SignalInput {
            payout_ratio: Some(0.35),
            valuation: "cheap",
            consecutive_increases: Some(3),
            profit_uptrend: Some(true),
            no_loss_years: Some(true),
            no_quarterly_loss: Some(true),
        };
        assert!(sell_signals(&i).is_empty());
    }

    #[test]
    fn flags_each_condition() {
        let i = SignalInput {
            payout_ratio: Some(0.51),
            valuation: "expensive",
            consecutive_increases: Some(0),
            profit_uptrend: Some(false),
            no_loss_years: Some(true),
            no_quarterly_loss: Some(false),
        };
        assert_eq!(
            sell_signals(&i),
            vec![
                SellSignal::HighPayout,
                SellSignal::Expensive,
                SellSignal::NoIncrease,
                SellSignal::ProfitDowntrend,
                SellSignal::Loss,
            ]
        );
    }

    #[test]
    fn unknown_values_and_boundary_do_not_flag() {
        let i = SignalInput {
            payout_ratio: Some(0.5),
            valuation: "unknown",
            ..Default::default()
        };
        assert!(sell_signals(&i).is_empty(), "ちょうど50%と判定不能は出さない");
    }
}
