//! Data-access functions over the SQLite pool.
//!
//! Concrete functions for the POC; extracting these behind repository traits
//! (to swap SQLite ↔ Postgres) is a planned refinement, not needed for M1.

use crate::models::{Experiment, Run};
use chrono::Utc;
use sqlx::SqlitePool;
use std::collections::HashMap;
use uuid::Uuid;

fn new_id() -> String {
    Uuid::now_v7().to_string()
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

/// Stringify a JSON param value: bare string stays bare, everything else is its
/// JSON text (numbers, bools, arrays, objects).
fn stringify(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ----- experiments ------------------------------------------------------------
pub async fn get_or_create_experiment(
    pool: &SqlitePool,
    name: &str,
) -> Result<Experiment, sqlx::Error> {
    if let Some(exp) = sqlx::query_as::<_, Experiment>(
        "SELECT id, name, created_at FROM experiment WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?
    {
        return Ok(exp);
    }

    let exp = Experiment {
        id: new_id(),
        name: name.to_string(),
        created_at: now(),
    };
    // ON CONFLICT guards the race where two requests create the same name.
    sqlx::query(
        "INSERT INTO experiment (id, name, created_at) VALUES (?, ?, ?)
         ON CONFLICT(name) DO NOTHING",
    )
    .bind(&exp.id)
    .bind(&exp.name)
    .bind(&exp.created_at)
    .execute(pool)
    .await?;

    // Re-read so we return the winner's row regardless of who inserted.
    sqlx::query_as::<_, Experiment>("SELECT id, name, created_at FROM experiment WHERE name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
}

pub async fn list_experiments(pool: &SqlitePool) -> Result<Vec<Experiment>, sqlx::Error> {
    sqlx::query_as::<_, Experiment>(
        "SELECT id, name, created_at FROM experiment ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_experiment(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<Experiment>, sqlx::Error> {
    sqlx::query_as::<_, Experiment>("SELECT id, name, created_at FROM experiment WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
}

// ----- runs -------------------------------------------------------------------
pub async fn create_run(
    pool: &SqlitePool,
    experiment_id: &str,
    name: Option<&str>,
    params: &HashMap<String, serde_json::Value>,
    tags: &HashMap<String, String>,
) -> Result<Run, sqlx::Error> {
    let run = Run {
        id: new_id(),
        experiment_id: experiment_id.to_string(),
        name: name.map(|s| s.to_string()),
        status: crate::models::RUN_RUNNING.to_string(),
        started_at: now(),
        ended_at: None,
    };

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO run (id, experiment_id, name, status, started_at, ended_at)
         VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind(&run.id)
    .bind(&run.experiment_id)
    .bind(&run.name)
    .bind(&run.status)
    .bind(&run.started_at)
    .execute(&mut *tx)
    .await?;

    for (k, v) in params {
        sqlx::query("INSERT INTO param (run_id, key, value) VALUES (?, ?, ?)")
            .bind(&run.id)
            .bind(k)
            .bind(stringify(v))
            .execute(&mut *tx)
            .await?;
    }
    for (k, v) in tags {
        sqlx::query("INSERT INTO tag (run_id, key, value) VALUES (?, ?, ?)")
            .bind(&run.id)
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(run)
}

pub async fn get_run(pool: &SqlitePool, id: &str) -> Result<Option<Run>, sqlx::Error> {
    sqlx::query_as::<_, Run>(
        "SELECT id, experiment_id, name, status, started_at, ended_at FROM run WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_run_kv(
    pool: &SqlitePool,
    table: &str,
    run_id: &str,
) -> Result<HashMap<String, String>, sqlx::Error> {
    // `table` is a fixed internal literal ("param"/"tag"), never user input.
    let sql = format!("SELECT key, value FROM {table} WHERE run_id = ?");
    let rows: Vec<(String, String)> = sqlx::query_as(&sql)
        .bind(run_id)
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().collect())
}

/// Update run status (and end time). Returns the updated run, or None if absent.
pub async fn update_run_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    ended_at: Option<&str>,
) -> Result<Option<Run>, sqlx::Error> {
    let end_value: Option<String> = match (status, ended_at) {
        (_, Some(e)) => Some(e.to_string()),
        (s, None) if crate::models::TERMINAL_STATUSES.contains(&s) => Some(now()),
        _ => None,
    };

    let affected = sqlx::query("UPDATE run SET status = ?, ended_at = ? WHERE id = ?")
        .bind(status)
        .bind(&end_value)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();

    if affected == 0 {
        return Ok(None);
    }
    get_run(pool, id).await
}
