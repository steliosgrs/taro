//! Database connection + migration bootstrap.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

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
