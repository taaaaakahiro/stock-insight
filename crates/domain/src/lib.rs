//! ドメイン層: 銘柄・株価・配当・財務と、指標計算の純粋関数。
//! 外部クレートには依存しない。

pub mod indicators;
pub mod model;

pub use indicators::*;
pub use model::*;
