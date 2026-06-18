//! Database connection + migration bootstrap.
//!
//! The engine is chosen by URL scheme: `postgres://` / `postgresql://` → Postgres,
//! anything else → SQLite. [`build_store`] is the single place that branches and
//! returns a backend-agnostic `Arc<dyn Store>`; handlers never see the concrete pool.

use crate::store::{PgStore, SqliteStore, Store};
use sqlx::postgres::PgPoolOptions;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{PgPool, SqlitePool};
use std::str::FromStr;
use std::sync::Arc;

/// True for a Postgres connection string.
pub fn is_postgres(database_url: &str) -> bool {
    database_url.starts_with("postgres://") || database_url.starts_with("postgresql://")
}

/// Connect to whichever engine the URL names, run its migrations, and return the
/// matching `Store` behind the trait object. The one M7→M8 swap seam.
pub async fn build_store(database_url: &str) -> anyhow::Result<Arc<dyn Store>> {
    let store: Arc<dyn Store> = if is_postgres(database_url) {
        Arc::new(PgStore::new(connect_pg(database_url).await?))
    } else {
        Arc::new(SqliteStore::new(connect(database_url).await?))
    };
    Ok(store)
}

/// Connect to SQLite (creating the file if missing) and run pending migrations.
pub async fn connect(database_url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    // Migrations are embedded at compile time from ./migrations.
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

/// Connect to Postgres and run the PG-dialect migrations.
pub async fn connect_pg(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    sqlx::migrate!("./migrations_pg").run(&pool).await?;

    Ok(pool)
}
