//! Shared application state handed to every handler.

use sqlx::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Optional static bearer token enforced by the auth middleware.
    pub api_key: Option<String>,
}
