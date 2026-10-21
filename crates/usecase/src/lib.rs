//! アプリケーション層: データ取り込みのユースケースと、外部との境界（ポート）。
//! ポートはRust標準のトレイト内 async fn で定義し、async-trait は使わない。
//! （Wasmでは Send を要求しないため、ジェネリクスで受け取る）
#![allow(async_fn_in_trait)]

use domain::{compute, FinancialData, Indicators, MarketData, StockCode};

pub mod auth;
pub mod master;
pub mod screening;

#[derive(Debug, Clone, PartialEq)]
pub struct PortError(pub String);

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait PriceSource {
    async fn fetch_market(&self, code: &StockCode) -> Result<MarketData, PortError>;
}

pub trait FinancialSource {
    /// 最新の有価証券報告書から年次財務を返す。見つからなければ None。
    async fn fetch_financials(&self, code: &StockCode) -> Result<Option<FinancialData>, PortError>;
}

/// 1銘柄分の保存単位
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub code: StockCode,
    pub run_id: String,
    pub fetched_at: i64,
    pub indicators: Indicators,
    pub financials: Option<FinancialData>,
}

#[derive(Debug, Clone)]
pub struct BatchRun {
    pub run_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub target: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub status: &'static str,
}

pub trait SnapshotStore {
    async fn save_run(&self, run: &BatchRun) -> Result<(), PortError>;
    async fn save_snapshot(&self, snap: &Snapshot) -> Result<(), PortError>;
}

pub trait Clock {
    fn now_unix(&self) -> i64;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// 保存できた。財務が取れなかった場合は warning に理由を入れる
    Saved {
        warning: Option<String>,
    },
    Failed(String),
}

/// 1銘柄を取得 → 指標計算 → 保存する
pub async fn ingest_one<P, F, S, C>(
    code: &StockCode,
    run_id: &str,
    prices: &P,
    fins: &F,
    store: &S,
    clock: &C,
) -> Outcome
where
    P: PriceSource,
    F: FinancialSource,
    S: SnapshotStore,
    C: Clock,
{
    let market = match prices.fetch_market(code).await {
        Ok(m) => m,
        Err(e) => return Outcome::Failed(format!("price: {e}")),
    };
    let (financials, warning) = match fins.fetch_financials(code).await {
        Ok(Some(f)) => (Some(f), None),
        Ok(None) => (None, Some("financials: no annual report found".to_string())),
        Err(e) => (None, Some(format!("financials: {e}"))),
    };
    let indicators = match compute(&market, financials.as_ref()) {
        Ok(i) => i,
        Err(e) => return Outcome::Failed(format!("indicators: {e:?}")),
    };
    let snap = Snapshot {
        code: code.clone(),
        run_id: run_id.to_string(),
        fetched_at: clock.now_unix(),
        indicators,
        financials,
    };
    match store.save_snapshot(&snap).await {
        Ok(()) => Outcome::Saved { warning },
        Err(e) => Outcome::Failed(format!("store: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::block_on;
    use domain::{Date, PriceBar};
    use std::cell::RefCell;

    struct FakePrices(bool);
    impl PriceSource for FakePrices {
        async fn fetch_market(&self, _: &StockCode) -> Result<MarketData, PortError> {
            if !self.0 {
                return Err(PortError("timeout".into()));
            }
            Ok(MarketData {
                bars: vec![PriceBar {
                    date: Date::new(2026, 10, 20),
                    close: 1000.0,
                }],
                dividends: vec![],
            })
        }
    }
    struct NoFins;
    impl FinancialSource for NoFins {
        async fn fetch_financials(
            &self,
            _: &StockCode,
        ) -> Result<Option<FinancialData>, PortError> {
            Ok(None)
        }
    }
    #[derive(Default)]
    struct MemStore(RefCell<Vec<String>>);
    impl SnapshotStore for MemStore {
        async fn save_run(&self, _: &BatchRun) -> Result<(), PortError> {
            Ok(())
        }
        async fn save_snapshot(&self, s: &Snapshot) -> Result<(), PortError> {
            self.0.borrow_mut().push(s.code.as_str().to_string());
            Ok(())
        }
    }
    struct FixedClock;
    impl Clock for FixedClock {
        fn now_unix(&self) -> i64 {
            0
        }
    }

    #[test]
    fn saves_with_warning_when_no_financials() {
        let store = MemStore::default();
        let code = StockCode::parse("7203").unwrap();
        let out = block_on(ingest_one(
            &code,
            "r1",
            &FakePrices(true),
            &NoFins,
            &store,
            &FixedClock,
        ));
        assert!(matches!(out, Outcome::Saved { warning: Some(_) }));
        assert_eq!(store.0.borrow().as_slice(), ["7203"]);
    }

    #[test]
    fn price_failure_is_not_saved() {
        let store = MemStore::default();
        let code = StockCode::parse("7203").unwrap();
        let out = block_on(ingest_one(
            &code,
            "r1",
            &FakePrices(false),
            &NoFins,
            &store,
            &FixedClock,
        ));
        assert!(matches!(out, Outcome::Failed(_)));
        assert!(store.0.borrow().is_empty());
    }
}

#[cfg(test)]
mod tests_support {
    /// 外部ランタイムなしで Future を1回だけ進める（テスト用。フェイクは即時完了する）
    pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn noop(_: *const ()) {}
        fn clone(p: *const ()) -> RawWaker {
            RawWaker::new(p, &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("fake future pending"),
        }
    }
}
