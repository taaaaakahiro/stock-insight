//! Worker 用のインフラ層: D1 の読み取り。

pub mod query;

#[cfg(target_arch = "wasm32")]
pub mod d1;
