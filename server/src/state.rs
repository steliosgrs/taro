//! Shared application state handed to every handler.

use crate::blob::BlobStore;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    /// Optional static bearer token enforced by the auth middleware.
    pub api_key: Option<String>,
    /// Where artifact bytes are persisted (swappable: LocalFs → S3).
    pub blob: Arc<dyn BlobStore>,
}
