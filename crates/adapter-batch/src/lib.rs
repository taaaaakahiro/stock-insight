//! バッチ用のインフラ層: J-Quants・EDINET・JPX・D1 との接続。

pub mod ai_rest;
pub mod d1;
pub mod d1_recommend;
pub mod edinet;
pub mod http;
pub mod ixbrl;
pub mod jpx;
pub mod jquants;
pub mod raw;
pub mod sql;

pub mod recommendation_evaluation;

pub mod recommendation_report;

pub mod reit_metrics;
pub mod reit_rag;

pub mod stock_financial_quality;
