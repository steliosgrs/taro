//! Blob storage abstraction (M5; streaming upload M9).
//!
//! The DB only ever stores artifact *metadata* (name/uri/media_type/size); the
//! bytes live behind a `BlobStore`. POC ships `LocalFs`; an `S3`/`object_store`
//! impl is a later swap behind the same trait.
//!
//! `put` takes an owned **stream** of byte chunks rather than a `&[u8]`, so a
//! large checkpoint never has to sit in server memory — it flows from the
//! request body to disk (or, later, to an S3 multipart upload) chunk by chunk.
//! Size isn't known up front, so `put` *counts* the bytes it writes and returns
//! `(uri, size)`.

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tokio::io::AsyncWriteExt;

/// An owned, pollable stream of byte chunks (the artifact body).
pub type ByteStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Persist a streamed artifact body and return `(uri, bytes_written)`.
    /// Layout: `{root}/{experiment_id}/{run_id}/{name}`.
    async fn put(
        &self,
        experiment_id: &str,
        run_id: &str,
        name: &str,
        body: ByteStream,
    ) -> anyhow::Result<(String, i64)>;
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
        mut body: ByteStream,
    ) -> anyhow::Result<(String, i64)> {
        // Guard against path traversal: store under the basename only.
        let file_name = Path::new(name)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid artifact name '{name}'"))?;

        let dir = self.root.join(experiment_id).join(run_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(file_name);

        // Stream chunks straight to disk, tallying bytes as we go.
        let mut file = tokio::fs::File::create(&path).await?;
        let mut size: i64 = 0;
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            size += chunk.len() as i64;
        }
        file.flush().await?;

        // Absolute path makes a portable file:// URI; fall back if canonicalize fails.
        let abs = tokio::fs::canonicalize(&path).await.unwrap_or(path);
        Ok((format!("file://{}", abs.display()), size))
    }
}
