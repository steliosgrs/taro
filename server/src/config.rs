//! Runtime configuration, sourced from environment variables with POC defaults.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    /// sqlx connection URL, e.g. `sqlite://taro.db`.
    pub database_url: String,
    /// Address the HTTP server binds to, e.g. `0.0.0.0:8080`.
    pub bind: String,
    /// Optional static bearer token. If `None`, auth is disabled (POC default).
    pub api_key: Option<String>,
    /// Filesystem root for artifact bytes (LocalFs blob store).
    pub blob_root: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("TARO_DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://taro.db".to_string()),
            bind: std::env::var("TARO_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string()),
            api_key: std::env::var("TARO_API_KEY").ok().filter(|s| !s.is_empty()),
            blob_root: std::env::var("TARO_BLOB_ROOT")
                .unwrap_or_else(|_| "./taro_blobs".to_string())
                .into(),
        }
    }
}
