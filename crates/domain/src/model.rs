/// 日付（YYYY-MM-DD を日数で扱うための最小実装）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Self {
        Self { y, m, d }
    }

    /// "YYYY-MM-DD" を解釈する
    pub fn parse(s: &str) -> Option<Self> {
        let mut it = s.get(0..10)?.split('-');
        let y = it.next()?.parse().ok()?;
        let m = it.next()?.parse().ok()?;
        let d = it.next()?.parse().ok()?;
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return None;
        }
        Some(Self { y, m, d })
    }

    /// UNIX時刻（秒）から日本時間の日付を得る
    pub fn from_unix_jst(ts: i64) -> Self {
        Self::from_days((ts + 9 * 3600).div_euclid(86_400))
    }

    /// 1970-01-01 からの日数（Howard Hinnant のアルゴリズム）
    pub fn to_days(self) -> i64 {
        let y = if self.m <= 2 { self.y - 1 } else { self.y } as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.m as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32;
        Self { y, m, d }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.to_days() + n)
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

/// 銘柄コード（4桁、英字入りの新コードも許容）
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StockCode(String);

impl StockCode {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let ok = s.len() == 4
            && s.chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase());
        ok.then(|| Self(s.to_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// EDINET の証券コード（5桁、末尾0）
    pub fn edinet_sec_code(&self) -> String {
        format!("{}0", self.0)
    }
}

/// 日次の終値
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceBar {
    pub date: Date,
    pub close: f64,
}

/// 年間配当。開示で分かった値（会社予想、なければ実績）を、開示日から次の開示まで使う
#[derive(Debug, Clone, PartialEq)]
pub struct Dividend {
    /// この日から有効（開示日）
    pub effective: Date,
    /// 1株（REIT・ETF は1口）当たりの年間配当
    pub annual: f64,
    /// 会社予想なら true、実績なら false
    pub forecast: bool,
    /// None は内訳未確認。ゼロとは区別する。
    pub breakdown: Option<DividendBreakdown>,
}

/// 年間配当の内訳。同じ開示・年度・株数基準の値だけを組み合わせる。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DividendBreakdown {
    pub special: f64,
    pub commemorative: f64,
    pub source: String,
    pub known_since: Option<Date>,
}

impl DividendBreakdown {
    pub fn ordinary(&self, total: f64) -> Option<f64> {
        let ordinary = total - self.special - self.commemorative;
        (total.is_finite()
            && self.special.is_finite()
            && self.commemorative.is_finite()
            && total >= 0.0
            && self.special >= 0.0
            && self.commemorative >= 0.0
            && ordinary >= -1e-9)
            .then_some(ordinary.max(0.0))
    }

    pub fn adjusted(&self, factor: f64) -> Self {
        Self {
            special: self.special * factor,
            commemorative: self.commemorative * factor,
            source: self.source.clone(),
            known_since: self.known_since,
        }
    }
}

impl Dividend {
    /// 利回り用配当。会社予想は特別・記念配当を含む年間総額を使う。
    pub fn yield_amount(&self) -> Option<f64> {
        if self.forecast {
            return (self.annual.is_finite() && self.annual >= 0.0).then_some(self.annual);
        }
        self.ordinary()
    }

    pub fn ordinary(&self) -> Option<f64> {
        match &self.breakdown {
            Some(b) => b.ordinary(self.annual),
            None => (self.annual.is_finite() && self.annual >= 0.0).then_some(self.annual),
        }
    }
}

/// 株価（終値）と、開示された年間配当の履歴
#[derive(Debug, Clone, Default)]
pub struct MarketData {
    pub bars: Vec<PriceBar>,
    pub dividends: Vec<Dividend>,
}

/// 年次の財務（1決算期分）。取れない項目は None。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnnualFinancial {
    pub period_end: Option<Date>,
    pub net_sales: Option<f64>,
    pub operating_income: Option<f64>,
    pub ordinary_income: Option<f64>,
    pub net_income: Option<f64>,
    pub equity: Option<f64>,
    pub eps: Option<f64>,
    pub bps: Option<f64>,
    pub dps: Option<f64>,
    pub operating_cf: Option<f64>,
}

impl AnnualFinancial {
    /// 期末のほかに数字が1つもない（配当予想・業績予想の修正が本決算として残った行など）
    pub fn has_no_figures(&self) -> bool {
        [
            self.net_sales,
            self.operating_income,
            self.ordinary_income,
            self.net_income,
            self.equity,
            self.eps,
            self.bps,
            self.dps,
            self.operating_cf,
        ]
        .iter()
        .all(Option::is_none)
    }
}

/// 財務の取得結果（新しい期が先頭）
#[derive(Debug, Clone, Default)]
pub struct FinancialData {
    pub doc_id: String,
    pub submitted: Option<Date>,
    pub years: Vec<AnnualFinancial>,
}
