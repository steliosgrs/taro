//! Blob storage abstraction (M5).
//!
//! The DB only ever stores artifact *metadata* (name/uri/media_type/size); the
//! bytes live behind a `BlobStore`. POC ships `LocalFs`; an `S3`/`object_store`
//! impl is a later swap behind the same trait.

use async_trait::async_trait;
use std::path::{Path, PathBuf};

#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Persist `bytes` for an artifact and return its URI (e.g. `file:///…`).
    /// Layout: `{root}/{experiment_id}/{run_id}/{name}`.
    async fn put(
        &self,
        experiment_id: &str,
        run_id: &str,
        name: &str,
        bytes: &[u8],
    ) -> anyhow::Result<String>;
}

/// Local-filesystem store for the POC.
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl BlobStore for LocalFs {
    async fn put(
        &self,
        experiment_id: &str,
        run_id: &str,
        name: &str,
        bytes: &[u8],
    ) -> anyhow::Result<String> {
        // Guard against path traversal: store under the basename only.
        let file_name = Path::new(name)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid artifact name '{name}'"))?;

        let dir = self.root.join(experiment_id).join(run_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(file_name);
        tokio::fs::write(&path, bytes).await?;

        // Absolute path makes a portable file:// URI; fall back if canonicalize fails.
        let abs = tokio::fs::canonicalize(&path).await.unwrap_or(path);
        Ok(format!("file://{}", abs.display()))
    }
}
