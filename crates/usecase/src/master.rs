//! 銘柄マスタの取り込み（JPX → stocks）。

use crate::{Clock, PortError};
use domain::stock_master::ListedStock;

pub trait StockMasterSource {
    /// (基準日 "YYYYMMDD", 銘柄)
    async fn fetch_master(&self) -> Result<(String, Vec<ListedStock>), PortError>;
}

pub trait StockMasterStore {
    async fn save_stocks(
        &self,
        as_of: &str,
        stocks: &[ListedStock],
        now: i64,
    ) -> Result<(), PortError>;
}

/// 取り込んだ件数を返す
pub async fn sync_master<S, T, C>(source: &S, store: &T, clock: &C) -> Result<usize, PortError>
where
    S: StockMasterSource,
    T: StockMasterStore,
    C: Clock,
{
    let (as_of, stocks) = source.fetch_master().await?;
    // JPX の一覧が壊れていたときに、マスタを空や数件で上書きしない
    if stocks.len() < 1000 {
        return Err(PortError(format!(
            "jpx: only {} stocks found; refusing to update the master",
            stocks.len()
        )));
    }
    store.save_stocks(&as_of, &stocks, clock.now_unix()).await?;
    Ok(stocks.len())
}
