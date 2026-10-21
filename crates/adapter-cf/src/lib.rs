//! Worker 用のインフラ層: D1、Clerk（本人確認）との接続。

pub mod clerk_user;
pub mod jwt;
pub mod query;

#[cfg(target_arch = "wasm32")]
pub mod clerk;
#[cfg(target_arch = "wasm32")]
pub mod d1;
#[cfg(target_arch = "wasm32")]
pub mod github;
#[cfg(target_arch = "wasm32")]
pub mod users;
