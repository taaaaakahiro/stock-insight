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

/// 配当（権利落ち日と1株あたり金額）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dividend {
    pub ex_date: Date,
    pub amount: f64,
}

/// 株価・配当の取得結果
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

/// 財務の取得結果（新しい期が先頭）
#[derive(Debug, Clone, Default)]
pub struct FinancialData {
    pub doc_id: String,
    pub submitted: Option<Date>,
    pub years: Vec<AnnualFinancial>,
}
