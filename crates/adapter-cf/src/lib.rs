//! Worker 用のインフラ層: D1、Clerk（本人確認）、Workers AI との接続。
#![recursion_limit = "256"]

pub mod clerk_user;
pub mod jwt;
pub mod query;
pub mod recommend;

#[cfg(target_arch = "wasm32")]
pub mod ai;
#[cfg(target_arch = "wasm32")]
pub mod clerk;
#[cfg(target_arch = "wasm32")]
pub mod d1;
#[cfg(target_arch = "wasm32")]
pub mod d1_recommend;
#[cfg(target_arch = "wasm32")]
pub mod d1_themes;
#[cfg(target_arch = "wasm32")]
pub mod github;
#[cfg(target_arch = "wasm32")]
pub mod users;

pub mod stock_json;

#[cfg(target_arch = "wasm32")]
pub mod d1_reit_rag;
