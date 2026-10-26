//! ポートフォリオ（保有銘柄の登録と、売り時のサインつきの一覧）のユースケース。

use crate::screening::StockRow;
use crate::PortError;
use domain::portfolio::{sell_signals, SellSignal, SignalInput, MAX_ITEMS};
use domain::StockCode;

pub trait PortfolioRepository {
    /// 登録した銘柄コード（登録の古い順）
    async fn codes(&self, user_id: &str) -> Result<Vec<String>, PortError>;
    /// 登録した銘柄の最新指標（指標がまだない銘柄は含まない）
    async fn rows(&self, user_id: &str) -> Result<Vec<HeldStock>, PortError>;
    /// すでにあれば何もしない
    async fn add(&self, user_id: &str, code: &str, now: i64) -> Result<(), PortError>;
    async fn closing_price(
        &self,
        code: &str,
        date: &str,
    ) -> Result<Option<(String, f64)>, PortError>;
    async fn acquisition(
        &self,
        user_id: &str,
        code: &str,
        date: Option<&str>,
        price: Option<f64>,
        price_date: Option<&str>,
    ) -> Result<(), PortError>;
    async fn remove(&self, user_id: &str, code: &str) -> Result<(), PortError>;
}

/// 登録した銘柄の最新指標と、売りサインの判定にだけ使う値
#[derive(Debug, Clone, PartialEq)]
pub struct HeldStock {
    pub stock: StockRow,
    /// 最新の本決算の EPS（数字のない行は飛ばしたもの）。赤字なのに配当を出しているかを見る
    pub latest_eps: Option<f64>,
    pub acquisition_date: Option<String>,
    pub acquisition_price: Option<f64>,
    pub acquisition_price_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HoldingRow {
    pub stock: StockRow,
    /// 売る判断に使う主のサイン（配当性向・PBR・赤字配当）
    pub acquisition_date: Option<String>,
    pub acquisition_price: Option<f64>,
    pub acquisition_price_date: Option<String>,
    pub signals: Vec<SellSignal>,
    /// 参考のサイン
    pub reference: Vec<SellSignal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Portfolio {
    pub codes: Vec<String>,
    /// 主のサインの多い順、同数なら参考のサインの多い順（それも同数なら銘柄コード順）
    pub rows: Vec<HoldingRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortfolioError {
    BadCode,
    BadAcquisition,
    TooMany,
    Port(PortError),
}

impl From<PortError> for PortfolioError {
    fn from(e: PortError) -> Self {
        PortfolioError::Port(e)
    }
}

pub fn signals_for(r: &StockRow, latest_eps: Option<f64>) -> Vec<SellSignal> {
    sell_signals(&SignalInput {
        kind: &r.kind,
        payout_ratio: r.payout_ratio,
        pbr: r.pbr,
        dividend: r.dividend,
        latest_eps,
        valuation: &r.valuation,
        dividend_cut: r.dividend_cut,
        profit_uptrend: r.profit_uptrend,
        no_loss_years: r.no_loss_years,
        no_quarterly_loss: r.no_quarterly_loss,
    })
}

pub async fn list<R: PortfolioRepository>(repo: &R, user_id: &str) -> Result<Portfolio, PortError> {
    let codes = repo.codes(user_id).await?;
    let mut rows: Vec<HoldingRow> = repo
        .rows(user_id)
        .await?
        .into_iter()
        .map(|h| {
            let (signals, reference) = signals_for(&h.stock, h.latest_eps)
                .into_iter()
                .partition(|s| s.is_primary());
            HoldingRow {
                acquisition_date: h.acquisition_date,
                acquisition_price: h.acquisition_price,
                acquisition_price_date: h.acquisition_price_date,
                stock: h.stock,
                signals,
                reference,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.signals
            .len()
            .cmp(&a.signals.len())
            .then_with(|| b.reference.len().cmp(&a.reference.len()))
            .then_with(|| a.stock.code.cmp(&b.stock.code))
    });
    Ok(Portfolio { codes, rows })
}

pub async fn add<R: PortfolioRepository>(
    repo: &R,
    user_id: &str,
    code: &str,
    now: i64,
) -> Result<(), PortfolioError> {
    let code = StockCode::parse(code).ok_or(PortfolioError::BadCode)?;
    let codes = repo.codes(user_id).await?;
    if codes.iter().any(|c| c == code.as_str()) {
        return Ok(());
    }
    if codes.len() >= MAX_ITEMS {
        return Err(PortfolioError::TooMany);
    }
    Ok(repo.add(user_id, code.as_str(), now).await?)
}

pub async fn acquisition<R: PortfolioRepository>(
    repo: &R,
    user_id: &str,
    code: &str,
    date: Option<&str>,
    now: i64,
) -> Result<(), PortfolioError> {
    let code = StockCode::parse(code).ok_or(PortfolioError::BadCode)?;
    if let Some(value) = date {
        let d = domain::Date::parse(value).ok_or(PortfolioError::BadAcquisition)?;
        if value.len() != 10
            || d.to_string() != value
            || domain::Date::from_days(d.to_days()) != d
            || d > domain::Date::from_unix_jst(now)
        {
            return Err(PortfolioError::BadAcquisition);
        }
    }
    let closing = match date {
        Some(date) => repo.closing_price(code.as_str(), date).await?,
        None => None,
    };
    Ok(repo
        .acquisition(
            user_id,
            code.as_str(),
            date,
            closing.as_ref().map(|(_, price)| *price),
            closing.as_ref().map(|(date, _)| date.as_str()),
        )
        .await?)
}

pub async fn remove<R: PortfolioRepository>(
    repo: &R,
    user_id: &str,
    code: &str,
) -> Result<(), PortfolioError> {
    let code = StockCode::parse(code).ok_or(PortfolioError::BadCode)?;
    Ok(repo.remove(user_id, code.as_str()).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use std::cell::RefCell;

    type SavedAcquisition = (Option<String>, Option<f64>, Option<String>);

    #[derive(Default)]
    struct Fake {
        items: RefCell<Vec<(String, String)>>,
        rows: Vec<HeldStock>,
        saved: RefCell<Option<SavedAcquisition>>,
    }

    impl PortfolioRepository for Fake {
        async fn codes(&self, user_id: &str) -> Result<Vec<String>, PortError> {
            Ok(self
                .items
                .borrow()
                .iter()
                .filter(|(u, _)| u == user_id)
                .map(|(_, c)| c.clone())
                .collect())
        }
        async fn rows(&self, _user_id: &str) -> Result<Vec<HeldStock>, PortError> {
            Ok(self.rows.clone())
        }
        async fn add(&self, user_id: &str, code: &str, _now: i64) -> Result<(), PortError> {
            self.items
                .borrow_mut()
                .push((user_id.to_string(), code.to_string()));
            Ok(())
        }
        async fn closing_price(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<(String, f64)>, PortError> {
            Ok(Some(("2026-10-23".into(), 100.0)))
        }
        async fn acquisition(
            &self,
            _: &str,
            _: &str,
            date: Option<&str>,
            price: Option<f64>,
            price_date: Option<&str>,
        ) -> Result<(), PortError> {
            *self.saved.borrow_mut() = Some((
                date.map(str::to_owned),
                price,
                price_date.map(str::to_owned),
            ));
            Ok(())
        }
        async fn remove(&self, user_id: &str, code: &str) -> Result<(), PortError> {
            self.items
                .borrow_mut()
                .retain(|(u, c)| !(u == user_id && c == code));
            Ok(())
        }
    }

    fn held(code: &str, payout: f64, valuation: &str) -> HeldStock {
        HeldStock {
            stock: row(code, payout, valuation),
            latest_eps: Some(100.0),
            acquisition_date: None,
            acquisition_price: None,
            acquisition_price_date: None,
        }
    }

    fn row(code: &str, payout: f64, valuation: &str) -> StockRow {
        StockRow {
            code: code.into(),
            price: 1000.0,
            price_date: "2026-10-21".into(),
            dividend: Some(30.0),
            dividend_basis: Some("forecast".into()),
            dividend_yield: Some(0.03),
            yield_percentile: None,
            pbr: None,
            per: None,
            payout_ratio: Some(payout),
            doe: None,
            consecutive_increases: None,
            operating_margin: None,
            no_loss_years: None,
            profit_uptrend: None,
            valuation: valuation.into(),
            fetched_at: 0,
            name: None,
            kind: "stock".into(),
            market: None,
            sector33: None,
            size_class: None,
            dividend_every_year: None,
            no_quarterly_loss: None,
            is_value: None,
            data_years: None,
            sales_growth: None,
            op_growth: None,
            dividend_cut: None,
            progressive_dividend: None,
            no_dividend_cut: None,
            doe_floor: None,
            doe_goal: None,
            doe_forecast: None,
            payout_floor: None,
            payout_goal: None,
            payout_forecast: None,
            op_growth_q: None,
            net_growth_q: None,
            op_growth_ttm: None,
            profit_one_off: None,
            reit: None,
        }
    }

    #[test]
    fn acquisition_saves_automatic_close_and_clear_removes_it() {
        let repo = Fake::default();
        let now = domain::Date::parse("2026-10-24").unwrap().to_days() * 86400;
        block_on(acquisition(&repo, "u1", "7203", Some("2026-10-24"), now)).unwrap();
        assert_eq!(
            *repo.saved.borrow(),
            Some((
                Some("2026-10-24".into()),
                Some(100.0),
                Some("2026-10-23".into())
            ))
        );
        block_on(acquisition(&repo, "u1", "7203", None, now)).unwrap();
        assert_eq!(*repo.saved.borrow(), Some((None, None, None)));
    }

    #[test]
    fn acquisition_validates_dates() {
        let repo = Fake::default();
        let now = domain::Date::parse("2026-10-24").unwrap().to_days() * 86400;
        for date in ["2026-02-30", "2026-10-25", "2026-10-24junk", "bad"] {
            assert_eq!(
                block_on(acquisition(&repo, "u1", "7203", Some(date), now)),
                Err(PortfolioError::BadAcquisition)
            );
        }
        assert!(block_on(acquisition(&repo, "u1", "7203", Some("2024-02-29"), now)).is_ok());
        assert!(block_on(acquisition(&repo, "u1", "7203", None, now)).is_ok());
    }

    #[test]
    fn add_validates_and_ignores_duplicates() {
        let repo = Fake::default();
        assert_eq!(
            block_on(add(&repo, "u1", "72; DROP", 0)),
            Err(PortfolioError::BadCode)
        );
        block_on(add(&repo, "u1", "7203", 0)).unwrap();
        block_on(add(&repo, "u1", "7203", 0)).unwrap();
        assert_eq!(block_on(repo.codes("u1")).unwrap(), vec!["7203"]);
        block_on(remove(&repo, "u1", "7203")).unwrap();
        assert!(block_on(repo.codes("u1")).unwrap().is_empty());
    }

    #[test]
    fn add_rejects_over_limit() {
        let repo = Fake::default();
        for i in 0..MAX_ITEMS {
            block_on(add(&repo, "u1", &format!("{:04}", 1000 + i), 0)).unwrap();
        }
        assert_eq!(
            block_on(add(&repo, "u1", "9999", 0)),
            Err(PortfolioError::TooMany)
        );
    }

    #[test]
    fn list_puts_primary_signals_first() {
        let mut loss = held("1005", 0.0, "neutral");
        loss.stock.payout_ratio = None;
        loss.stock.pbr = Some(1.6);
        loss.latest_eps = Some(-20.0);
        let repo = Fake {
            rows: vec![
                held("1001", 0.3, "cheap"),
                held("1002", 0.6, "expensive"),
                held("1003", 0.6, "neutral"),
                held("1004", 0.3, "expensive"),
                loss,
            ],
            ..Default::default()
        };
        let p = block_on(list(&repo, "u1")).unwrap();
        let order: Vec<&str> = p.rows.iter().map(|r| r.stock.code.as_str()).collect();
        assert_eq!(order, vec!["1005", "1002", "1003", "1004", "1001"]);
        assert_eq!(
            p.rows[0].signals,
            vec![SellSignal::HighPbr, SellSignal::LossDividend]
        );
        assert_eq!(p.rows[1].signals, vec![SellSignal::HighPayout]);
        assert_eq!(p.rows[1].reference, vec![SellSignal::Expensive]);
        assert!(p.rows[3].signals.is_empty(), "割高だけなら参考");
    }
}
