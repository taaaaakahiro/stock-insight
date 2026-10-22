//! ポートフォリオ（保有銘柄）の売り時のサイン。取得価格は持たず、最新の指標だけで判定する。
//!
//! 売るかどうかは配当性向と PBR で決める（主のサイン）。ほかのサインは参考に出すだけ。

/// 配当性向がこれを超えたら、増配の余地が小さい
pub const PAYOUT_WARN: f64 = 0.5;
/// PBR がこれを超えたら、株価が純資産に比べて高い
pub const PBR_WARN: f64 = 1.5;
/// 1人が登録できる銘柄数
pub const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SellSignal {
    /// 配当性向が 50% 超（主）
    HighPayout,
    /// PBR が 1.5倍超（主）
    HighPbr,
    /// 最新の本決算が赤字（EPS が0以下）なのに配当を出している（主）
    LossDividend,
    /// 利回りが過去5年の下位20%（株価が上がって割高）
    Expensive,
    /// 減配した、または会社予想が減配（据え置きは含めない）
    DividendCut,
    /// 営業利益の傾きが下向き
    ProfitDowntrend,
    /// 直近5期または20四半期に赤字がある
    Loss,
}

impl SellSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            SellSignal::HighPayout => "high_payout",
            SellSignal::HighPbr => "high_pbr",
            SellSignal::LossDividend => "loss_dividend",
            SellSignal::Expensive => "expensive",
            SellSignal::DividendCut => "dividend_cut",
            SellSignal::ProfitDowntrend => "profit_downtrend",
            SellSignal::Loss => "loss",
        }
    }

    /// 売る判断に使う主のサインか（偽なら参考）
    pub fn is_primary(self) -> bool {
        matches!(
            self,
            SellSignal::HighPayout | SellSignal::HighPbr | SellSignal::LossDividend
        )
    }
}

/// 判定に使う指標（一覧の1行から必要な分だけ）。判定できない値は None で、サインは出さない
#[derive(Debug, Clone, Copy, Default)]
pub struct SignalInput<'a> {
    /// stock / reit / etf。配当性向・PBR・赤字配当は株式だけで見る（REIT は利益のほぼ全部を分配するため）
    pub kind: &'a str,
    pub payout_ratio: Option<f64>,
    pub pbr: Option<f64>,
    /// 利回りに使った1株配当（予想か実績）
    pub dividend: Option<f64>,
    /// 最新の本決算の EPS（数字のない行は飛ばしたもの）
    pub latest_eps: Option<f64>,
    pub valuation: &'a str,
    pub dividend_cut: Option<bool>,
    pub profit_uptrend: Option<bool>,
    pub no_loss_years: Option<bool>,
    pub no_quarterly_loss: Option<bool>,
}

/// 主のサインを先に、参考のサインを後に並べて返す
pub fn sell_signals(i: &SignalInput) -> Vec<SellSignal> {
    let mut out = vec![];
    if i.kind == "stock" {
        if i.payout_ratio.is_some_and(|p| p > PAYOUT_WARN) {
            out.push(SellSignal::HighPayout);
        }
        if i.pbr.is_some_and(|p| p > PBR_WARN) {
            out.push(SellSignal::HighPbr);
        }
        // 赤字だと配当性向は出ない（EPS で割れない）ので、HighPayout とは重ならない
        if i.latest_eps.is_some_and(|e| e <= 0.0) && i.dividend.is_some_and(|d| d > 0.0) {
            out.push(SellSignal::LossDividend);
        }
    }
    if i.valuation == crate::Valuation::Expensive.as_str() {
        out.push(SellSignal::Expensive);
    }
    if i.dividend_cut == Some(true) {
        out.push(SellSignal::DividendCut);
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
            kind: "stock",
            payout_ratio: Some(0.35),
            pbr: Some(0.9),
            dividend: Some(30.0),
            latest_eps: Some(85.0),
            valuation: "cheap",
            dividend_cut: Some(false),
            profit_uptrend: Some(true),
            no_loss_years: Some(true),
            no_quarterly_loss: Some(true),
        };
        assert!(sell_signals(&i).is_empty());
    }

    #[test]
    fn flags_each_condition_primary_first() {
        let i = SignalInput {
            kind: "stock",
            payout_ratio: Some(0.51),
            pbr: Some(1.6),
            dividend: Some(30.0),
            latest_eps: Some(58.0),
            valuation: "expensive",
            dividend_cut: Some(true),
            profit_uptrend: Some(false),
            no_loss_years: Some(true),
            no_quarterly_loss: Some(false),
        };
        let s = sell_signals(&i);
        assert_eq!(
            s,
            vec![
                SellSignal::HighPayout,
                SellSignal::HighPbr,
                SellSignal::Expensive,
                SellSignal::DividendCut,
                SellSignal::ProfitDowntrend,
                SellSignal::Loss,
            ]
        );
        let primary: Vec<_> = s.iter().filter(|s| s.is_primary()).collect();
        assert_eq!(primary, vec![&SellSignal::HighPayout, &SellSignal::HighPbr]);
    }

    #[test]
    fn loss_with_dividend_is_primary() {
        let i = SignalInput {
            kind: "stock",
            payout_ratio: None,
            dividend: Some(20.0),
            latest_eps: Some(-12.0),
            ..Default::default()
        };
        assert_eq!(sell_signals(&i), vec![SellSignal::LossDividend]);
        assert!(SellSignal::LossDividend.is_primary());

        let no_dividend = SignalInput {
            dividend: Some(0.0),
            ..i
        };
        assert!(
            sell_signals(&no_dividend).is_empty(),
            "無配の赤字は出さない"
        );
    }

    #[test]
    fn unknown_values_and_boundary_do_not_flag() {
        let i = SignalInput {
            kind: "stock",
            payout_ratio: Some(0.5),
            pbr: Some(1.5),
            valuation: "unknown",
            ..Default::default()
        };
        assert!(
            sell_signals(&i).is_empty(),
            "ちょうど50%・1.5倍と判定不能は出さない"
        );
    }

    #[test]
    fn reit_skips_payout_pbr_and_loss_dividend() {
        let i = SignalInput {
            kind: "reit",
            payout_ratio: Some(0.98),
            pbr: Some(1.8),
            dividend: Some(3000.0),
            latest_eps: Some(-100.0),
            valuation: "expensive",
            ..Default::default()
        };
        assert_eq!(sell_signals(&i), vec![SellSignal::Expensive]);
    }
}
