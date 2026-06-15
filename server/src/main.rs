//! Taro server entrypoint (M1: health + experiment/run lifecycle, SQLite).

mod api;
mod auth;
mod config;
mod db;
mod error;
mod models;
mod repo;
mod state;

use config::Config;
use state::AppState;

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
        auth = cfg.api_key.is_some(), "starting taro-server");

    let pool = db::connect(&cfg.database_url).await?;
    let state = AppState {
        pool,
        api_key: cfg.api_key,
    };

    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!("listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
