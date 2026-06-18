//! Taro server entrypoint — a thin binary over the `taro_server` library crate.

use taro_server::{api, blob, config::Config, db, state::AppState};

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

    let state = AppState {
        store: db::build_store(&cfg.database_url).await?,
        api_key: cfg.api_key,
        blob: Arc::new(blob::LocalFs::new(cfg.blob_root)),
    };

    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
