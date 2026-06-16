//! Shared application state handed to every handler.

use crate::blob::BlobStore;
use crate::store::Store;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    /// Data access behind the storage seam (swappable: SQLite → Postgres).
    pub store: Arc<dyn Store>,
    /// Optional static bearer token enforced by the auth middleware.
    pub api_key: Option<String>,
    /// Where artifact bytes are persisted (swappable: LocalFs → S3).
    pub blob: Arc<dyn BlobStore>,
}
