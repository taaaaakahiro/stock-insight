//! ドメイン層: 銘柄・株価・配当・財務と、指標計算の純粋関数。
//! 外部クレートには依存しない。

pub mod access;
pub mod analysis;
pub mod calendar;
pub mod disclosure;
pub mod dividend_evidence;
pub mod dividend_policy;
pub mod indicators;
pub mod model;
pub mod portfolio;
pub mod price_history;
pub mod recommend;
pub mod reit;
pub mod screening;
pub mod split;
pub mod stock_master;
pub mod theme;

pub use indicators::*;
pub use model::*;

pub mod recommendation_evaluation;

pub mod financial_quality;
