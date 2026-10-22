//! アプリケーション層: データ取り込みのユースケースと、外部との境界（ポート）。
//! ポートはRust標準のトレイト内 async fn で定義し、async-trait は使わない。
//! （Wasmでは Send を要求しないため、ジェネリクスで受け取る）
#![allow(async_fn_in_trait)]

use domain::{Indicators, StockCode};

pub mod auth;
pub mod daily;
pub mod detail;
pub mod edinet;
pub mod fins;
pub mod master;
pub mod portfolio;
pub mod recommend;
pub mod screening;

#[derive(Debug, Clone, PartialEq)]
pub struct PortError(pub String);

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 1銘柄分の保存単位
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub code: StockCode,
    pub run_id: String,
    pub fetched_at: i64,
    pub indicators: Indicators,
    /// 指標に使った財務の開示番号と開示日（なければ None）
    pub fin_doc_id: Option<String>,
    pub fin_submitted: Option<domain::Date>,
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
    /// 複数銘柄をまとめて保存する（D1 への要求を減らすため）
    async fn save_snapshots(&self, snaps: &[Snapshot]) -> Result<(), PortError>;
}

pub trait Clock {
    fn now_unix(&self) -> i64;
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
