//! Recommendation policy v2. Missing facts are unknown, never zero.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FinancialQuality {
    pub cash: Option<f64>,
    pub debt: Option<f64>,
    pub liabilities: Option<f64>,
    pub assets: Option<f64>,
    pub equity: Option<f64>,
    pub operating_cf: Option<f64>,
    pub positive_operating_cf_3y: Option<bool>,
    pub investing_cf: Option<f64>,
    pub dividends_paid: Option<f64>,
    /// Original, bounded EDINET evidence including period, scope and citations.
    pub evidence_json: String,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QualityScore {
    pub value: u8,
    pub financial: u8,
    pub earnings: u8,
    pub dividend: u8,
}
impl QualityScore {
    pub fn total(self) -> u8 {
        self.value + self.financial + self.earnings + self.dividend
    }
}
pub fn general_company(sector: Option<&str>) -> bool {
    sector.is_some_and(|s| {
        !matches!(
            s,
            "銀行業"
                | "保険業"
                | "証券、商品先物取引業"
                | "その他金融業"
                | "7050"
                | "7100"
                | "7150"
                | "7200"
        )
    })
}
fn nonnegative(x: Option<f64>) -> Option<f64> {
    x.filter(|v| v.is_finite() && *v >= 0.0)
}
impl FinancialQuality {
    pub fn net_cash(&self) -> Option<f64> {
        Some(nonnegative(self.cash)? - nonnegative(self.debt)?)
    }
    pub fn financial_score(&self) -> u8 {
        let mut n = 0;
        if self.net_cash().is_some_and(|v| v > 0.0) {
            n += 2;
        }
        if matches!((nonnegative(self.cash), nonnegative(self.liabilities)), (Some(c),Some(l)) if c > l)
        {
            n += 1;
        }
        if let Some(a) = nonnegative(self.assets).filter(|v| *v > 0.0) {
            if nonnegative(self.debt).is_some_and(|d| d / a <= 0.2) {
                n += 1;
            }
            if nonnegative(self.equity).is_some_and(|e| e <= a && e / a >= 0.5) {
                n += 1;
            }
        }
        n.min(3)
    }
    pub fn positive_cf(&self) -> bool {
        self.operating_cf.is_some_and(|v| v.is_finite() && v > 0.0)
    }
    /// CFO + CFI is a cash surplus proxy, not capex-based free cash flow.
    pub fn cash_surplus(&self) -> Option<f64> {
        let c = self.operating_cf.filter(|v| v.is_finite())?;
        let i = self.investing_cf.filter(|v| v.is_finite() && *v <= 0.0)?;
        let v = c + i;
        v.is_finite().then_some(v)
    }
    pub fn covers_dividend(&self) -> bool {
        matches!((self.cash_surplus(), nonnegative(self.dividends_paid)), (Some(c),Some(d)) if d > 0.0 && c >= d)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_is_not_debt_free_and_rewards_are_capped() {
        let mut f = FinancialQuality {
            cash: Some(100.0),
            assets: Some(100.0),
            ..Default::default()
        };
        assert_eq!(f.net_cash(), None);
        assert_eq!(f.financial_score(), 0);
        f.debt = Some(0.0);
        f.liabilities = Some(10.0);
        f.equity = Some(90.0);
        assert_eq!(f.financial_score(), 3);
        f.cash = Some(f64::NAN);
        assert_eq!(f.net_cash(), None);
    }
    #[test]
    fn dividend_coverage_requires_cash_after_investment() {
        let mut f = FinancialQuality {
            operating_cf: Some(100.0),
            investing_cf: Some(-80.0),
            dividends_paid: Some(30.0),
            ..Default::default()
        };
        assert!(!f.covers_dividend());
        f.investing_cf = Some(-60.0);
        assert!(f.covers_dividend());
        f.investing_cf = Some(100.0);
        assert!(!f.covers_dividend());
        assert!(!general_company(Some("銀行業")));
        assert!(!general_company(None));
    }
}
