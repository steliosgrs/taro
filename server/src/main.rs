//! Taro server entrypoint (M1: health + experiment/run lifecycle, SQLite).

mod api;
mod auth;
mod blob;
mod config;
mod db;
mod error;
mod models;
mod state;
mod store;

use config::Config;
use state::AppState;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "taro_server=info,tower_http=info".into()),
        )
        .init();

    let cfg = Config::from_env();
    tracing::info!(database_url = %cfg.database_url, bind = %cfg.bind,
        auth = cfg.api_key.is_some(), blob_root = %cfg.blob_root.display(),
        "starting taro-server");

    let pool = db::connect(&cfg.database_url).await?;
    let state = AppState {
        store: Arc::new(store::SqliteStore::new(pool)),
        api_key: cfg.api_key,
        blob: Arc::new(blob::LocalFs::new(cfg.blob_root)),
    };

    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
