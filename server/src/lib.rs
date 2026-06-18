//! Taro server library surface.
//!
//! `main.rs` is a thin binary over this crate; integration tests in `tests/`
//! depend on it too (they build the real router via `api::router`). Keeping the
//! modules here — rather than private to the bin — is what makes the storage
//! seam testable end-to-end.

pub mod api;
pub mod auth;
pub mod blob;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod state;
pub mod store;
