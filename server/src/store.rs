//! Storage seam: the `Store` trait abstracts all data access so the engine is
//! swappable (SQLite now → Postgres later) without touching handlers. `AppState`
//! holds an `Arc<dyn Store>`; handlers call `st.store.*` and never see sqlx.
//!
//! Two impls today: `SqliteStore` (POC default) and `PgStore` (productionization,
//! M8). Methods return `sqlx::Error` — backend-agnostic in sqlx — so both slot in
//! behind the same trait; `db::build_store` picks one by URL scheme. The two impls
//! differ only in placeholder style (`?` vs `$1`) and a couple of optional-filter
//! queries; behaviour and the `FromRow` models are shared, which is what the M7
//! engine-generic suite re-runs against Postgres to prove parity.

use crate::models::{
    Artifact, CurveInput, CurveRow, Experiment, MetricRow, Run, ScalarMetricInput,
};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::{PgPool, SqlitePool};
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

#[async_trait]
pub trait Store: Send + Sync {
    // experiments
    async fn get_or_create_experiment(&self, name: &str) -> Result<Experiment, sqlx::Error>;
    async fn list_experiments(&self) -> Result<Vec<Experiment>, sqlx::Error>;
    async fn get_experiment(&self, id: &str) -> Result<Option<Experiment>, sqlx::Error>;

    // runs
    async fn create_run(
        &self,
        experiment_id: &str,
        name: Option<&str>,
        params: &HashMap<String, serde_json::Value>,
        tags: &HashMap<String, String>,
    ) -> Result<Run, sqlx::Error>;
    async fn get_run(&self, id: &str) -> Result<Option<Run>, sqlx::Error>;
    /// Read a run's key/value side-table (`table` is a fixed literal "param"/"tag").
    async fn get_run_kv(
        &self,
        table: &str,
        run_id: &str,
    ) -> Result<HashMap<String, String>, sqlx::Error>;
    async fn update_run_status(
        &self,
        id: &str,
        status: &str,
        ended_at: Option<&str>,
    ) -> Result<Option<Run>, sqlx::Error>;

    // scalar metrics
    async fn insert_scalar_metrics(
        &self,
        run_id: &str,
        metrics: &[ScalarMetricInput],
    ) -> Result<usize, sqlx::Error>;
    async fn get_scalar_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
    ) -> Result<Vec<MetricRow>, sqlx::Error>;

    // curve metrics
    async fn insert_curve_metrics(
        &self,
        run_id: &str,
        curves: &[CurveInput],
    ) -> Result<usize, sqlx::Error>;
    async fn get_curve_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
        step: Option<i64>,
    ) -> Result<Vec<CurveRow>, sqlx::Error>;
    async fn get_curve_one(
        &self,
        run_id: &str,
        key: &str,
        step: Option<i64>,
    ) -> Result<Option<CurveRow>, sqlx::Error>;

    // artifacts
    async fn insert_artifact(
        &self,
        run_id: &str,
        name: &str,
        uri: &str,
        media_type: Option<&str>,
        size_bytes: Option<i64>,
    ) -> Result<Artifact, sqlx::Error>;
    async fn get_artifacts(&self, run_id: &str) -> Result<Vec<Artifact>, sqlx::Error>;
}

/// Columns read back for any curve query (keep in sync with `CurveRow`).
const CURVE_COLS: &str = "run_id, key, step, curve_type, x_label, y_label, data, ts";

/// SQLite-backed `Store`.
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Store for SqliteStore {
    // ----- experiments --------------------------------------------------------
    async fn get_or_create_experiment(&self, name: &str) -> Result<Experiment, sqlx::Error> {
        if let Some(exp) = sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
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
        .execute(&self.pool)
        .await?;

        // Re-read so we return the winner's row regardless of who inserted.
        sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment WHERE name = ?",
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await
    }

    async fn list_experiments(&self) -> Result<Vec<Experiment>, sqlx::Error> {
        sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn get_experiment(&self, id: &str) -> Result<Option<Experiment>, sqlx::Error> {
        sqlx::query_as::<_, Experiment>("SELECT id, name, created_at FROM experiment WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    // ----- runs ---------------------------------------------------------------
    async fn create_run(
        &self,
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

        let mut tx = self.pool.begin().await?;

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

    async fn get_run(&self, id: &str) -> Result<Option<Run>, sqlx::Error> {
        sqlx::query_as::<_, Run>(
            "SELECT id, experiment_id, name, status, started_at, ended_at FROM run WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn get_run_kv(
        &self,
        table: &str,
        run_id: &str,
    ) -> Result<HashMap<String, String>, sqlx::Error> {
        // `table` is a fixed internal literal ("param"/"tag"), never user input.
        let sql = format!("SELECT key, value FROM {table} WHERE run_id = ?");
        let rows: Vec<(String, String)> =
            sqlx::query_as(&sql).bind(run_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }

    async fn update_run_status(
        &self,
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
            .execute(&self.pool)
            .await?
            .rows_affected();

        if affected == 0 {
            return Ok(None);
        }
        self.get_run(id).await
    }

    // ----- scalar metrics -----------------------------------------------------
    async fn insert_scalar_metrics(
        &self,
        run_id: &str,
        metrics: &[ScalarMetricInput],
    ) -> Result<usize, sqlx::Error> {
        let ts = now();
        let mut tx = self.pool.begin().await?;
        for m in metrics {
            sqlx::query(
                "INSERT INTO scalar_metric (run_id, key, step, value, ts) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(run_id)
            .bind(&m.key)
            .bind(m.step)
            .bind(m.value)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(metrics.len())
    }

    async fn get_scalar_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
    ) -> Result<Vec<MetricRow>, sqlx::Error> {
        match key {
            Some(k) => sqlx::query_as::<_, MetricRow>(
                "SELECT key, step, value, ts FROM scalar_metric
                 WHERE run_id = ? AND key = ? ORDER BY step ASC, id ASC",
            )
            .bind(run_id)
            .bind(k)
            .fetch_all(&self.pool)
            .await,
            None => sqlx::query_as::<_, MetricRow>(
                "SELECT key, step, value, ts FROM scalar_metric
                 WHERE run_id = ? ORDER BY key ASC, step ASC, id ASC",
            )
            .bind(run_id)
            .fetch_all(&self.pool)
            .await,
        }
    }

    // ----- curve metrics ------------------------------------------------------
    async fn insert_curve_metrics(
        &self,
        run_id: &str,
        curves: &[CurveInput],
    ) -> Result<usize, sqlx::Error> {
        let ts = now();
        let mut tx = self.pool.begin().await?;
        for c in curves {
            // Validated data serializes infallibly in practice; surface any
            // failure as an Encode error so the storage trait stays sqlx-only.
            let data_json = serde_json::to_string(&c.data)
                .map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
            sqlx::query(
                "INSERT INTO curve_metric
                     (run_id, key, step, curve_type, x_label, y_label, data, ts)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(run_id)
            .bind(&c.key)
            .bind(c.step)
            .bind(&c.curve_type)
            .bind(&c.x_label)
            .bind(&c.y_label)
            .bind(&data_json)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(curves.len())
    }

    async fn get_curve_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
        step: Option<i64>,
    ) -> Result<Vec<CurveRow>, sqlx::Error> {
        // `(? IS NULL OR col = ?)` makes each filter optional with a single query.
        let sql = format!(
            "SELECT {CURVE_COLS} FROM curve_metric
             WHERE run_id = ?
               AND (? IS NULL OR key = ?)
               AND (? IS NULL OR step = ?)
             ORDER BY key ASC, step ASC, id ASC"
        );
        sqlx::query_as::<_, CurveRow>(&sql)
            .bind(run_id)
            .bind(key)
            .bind(key)
            .bind(step)
            .bind(step)
            .fetch_all(&self.pool)
            .await
    }

    async fn get_curve_one(
        &self,
        run_id: &str,
        key: &str,
        step: Option<i64>,
    ) -> Result<Option<CurveRow>, sqlx::Error> {
        let sql = format!(
            "SELECT {CURVE_COLS} FROM curve_metric
             WHERE run_id = ? AND key = ? AND (? IS NULL OR step = ?)
             ORDER BY step DESC, id DESC
             LIMIT 1"
        );
        sqlx::query_as::<_, CurveRow>(&sql)
            .bind(run_id)
            .bind(key)
            .bind(step)
            .bind(step)
            .fetch_optional(&self.pool)
            .await
    }

    // ----- artifacts ----------------------------------------------------------
    async fn insert_artifact(
        &self,
        run_id: &str,
        name: &str,
        uri: &str,
        media_type: Option<&str>,
        size_bytes: Option<i64>,
    ) -> Result<Artifact, sqlx::Error> {
        let art = Artifact {
            id: new_id(),
            run_id: run_id.to_string(),
            name: name.to_string(),
            uri: uri.to_string(),
            media_type: media_type.map(|s| s.to_string()),
            size_bytes,
            created_at: now(),
        };
        sqlx::query(
            "INSERT INTO artifact (id, run_id, name, uri, media_type, size_bytes, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&art.id)
        .bind(&art.run_id)
        .bind(&art.name)
        .bind(&art.uri)
        .bind(&art.media_type)
        .bind(art.size_bytes)
        .bind(&art.created_at)
        .execute(&self.pool)
        .await?;
        Ok(art)
    }

    async fn get_artifacts(&self, run_id: &str) -> Result<Vec<Artifact>, sqlx::Error> {
        sqlx::query_as::<_, Artifact>(
            "SELECT id, run_id, name, uri, media_type, size_bytes, created_at
             FROM artifact WHERE run_id = ? ORDER BY created_at ASC, id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
    }
}

/// Postgres-backed `Store` (M8). A near-mirror of `SqliteStore`: same logic and
/// shared `FromRow` models, only `$N` placeholders and PG-typed optional filters
/// (`$n::text/$n::bigint IS NULL`) so a NULL param's type is unambiguous.
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Store for PgStore {
    // ----- experiments --------------------------------------------------------
    async fn get_or_create_experiment(&self, name: &str) -> Result<Experiment, sqlx::Error> {
        if let Some(exp) = sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?
        {
            return Ok(exp);
        }

        let exp = Experiment {
            id: new_id(),
            name: name.to_string(),
            created_at: now(),
        };
        sqlx::query(
            "INSERT INTO experiment (id, name, created_at) VALUES ($1, $2, $3)
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(&exp.id)
        .bind(&exp.name)
        .bind(&exp.created_at)
        .execute(&self.pool)
        .await?;

        sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment WHERE name = $1",
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await
    }

    async fn list_experiments(&self) -> Result<Vec<Experiment>, sqlx::Error> {
        sqlx::query_as::<_, Experiment>(
            "SELECT id, name, created_at FROM experiment ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn get_experiment(&self, id: &str) -> Result<Option<Experiment>, sqlx::Error> {
        sqlx::query_as::<_, Experiment>("SELECT id, name, created_at FROM experiment WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    // ----- runs ---------------------------------------------------------------
    async fn create_run(
        &self,
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

        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO run (id, experiment_id, name, status, started_at, ended_at)
             VALUES ($1, $2, $3, $4, $5, NULL)",
        )
        .bind(&run.id)
        .bind(&run.experiment_id)
        .bind(&run.name)
        .bind(&run.status)
        .bind(&run.started_at)
        .execute(&mut *tx)
        .await?;

        for (k, v) in params {
            sqlx::query("INSERT INTO param (run_id, key, value) VALUES ($1, $2, $3)")
                .bind(&run.id)
                .bind(k)
                .bind(stringify(v))
                .execute(&mut *tx)
                .await?;
        }
        for (k, v) in tags {
            sqlx::query("INSERT INTO tag (run_id, key, value) VALUES ($1, $2, $3)")
                .bind(&run.id)
                .bind(k)
                .bind(v)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        Ok(run)
    }

    async fn get_run(&self, id: &str) -> Result<Option<Run>, sqlx::Error> {
        sqlx::query_as::<_, Run>(
            "SELECT id, experiment_id, name, status, started_at, ended_at FROM run WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
    }

    async fn get_run_kv(
        &self,
        table: &str,
        run_id: &str,
    ) -> Result<HashMap<String, String>, sqlx::Error> {
        // `table` is a fixed internal literal ("param"/"tag"), never user input.
        let sql = format!("SELECT key, value FROM {table} WHERE run_id = $1");
        let rows: Vec<(String, String)> =
            sqlx::query_as(&sql).bind(run_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().collect())
    }

    async fn update_run_status(
        &self,
        id: &str,
        status: &str,
        ended_at: Option<&str>,
    ) -> Result<Option<Run>, sqlx::Error> {
        let end_value: Option<String> = match (status, ended_at) {
            (_, Some(e)) => Some(e.to_string()),
            (s, None) if crate::models::TERMINAL_STATUSES.contains(&s) => Some(now()),
            _ => None,
        };

        let affected = sqlx::query("UPDATE run SET status = $1, ended_at = $2 WHERE id = $3")
            .bind(status)
            .bind(&end_value)
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected();

        if affected == 0 {
            return Ok(None);
        }
        self.get_run(id).await
    }

    // ----- scalar metrics -----------------------------------------------------
    async fn insert_scalar_metrics(
        &self,
        run_id: &str,
        metrics: &[ScalarMetricInput],
    ) -> Result<usize, sqlx::Error> {
        let ts = now();
        let mut tx = self.pool.begin().await?;
        for m in metrics {
            sqlx::query(
                "INSERT INTO scalar_metric (run_id, key, step, value, ts)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(run_id)
            .bind(&m.key)
            .bind(m.step)
            .bind(m.value)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(metrics.len())
    }

    async fn get_scalar_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
    ) -> Result<Vec<MetricRow>, sqlx::Error> {
        match key {
            Some(k) => sqlx::query_as::<_, MetricRow>(
                "SELECT key, step, value, ts FROM scalar_metric
                 WHERE run_id = $1 AND key = $2 ORDER BY step ASC, id ASC",
            )
            .bind(run_id)
            .bind(k)
            .fetch_all(&self.pool)
            .await,
            None => sqlx::query_as::<_, MetricRow>(
                "SELECT key, step, value, ts FROM scalar_metric
                 WHERE run_id = $1 ORDER BY key ASC, step ASC, id ASC",
            )
            .bind(run_id)
            .fetch_all(&self.pool)
            .await,
        }
    }

    // ----- curve metrics ------------------------------------------------------
    async fn insert_curve_metrics(
        &self,
        run_id: &str,
        curves: &[CurveInput],
    ) -> Result<usize, sqlx::Error> {
        let ts = now();
        let mut tx = self.pool.begin().await?;
        for c in curves {
            let data_json = serde_json::to_string(&c.data)
                .map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
            sqlx::query(
                "INSERT INTO curve_metric
                     (run_id, key, step, curve_type, x_label, y_label, data, ts)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(run_id)
            .bind(&c.key)
            .bind(c.step)
            .bind(&c.curve_type)
            .bind(&c.x_label)
            .bind(&c.y_label)
            .bind(&data_json)
            .bind(&ts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(curves.len())
    }

    async fn get_curve_metrics(
        &self,
        run_id: &str,
        key: Option<&str>,
        step: Option<i64>,
    ) -> Result<Vec<CurveRow>, sqlx::Error> {
        // Casts give each NULL param an unambiguous type ($2/$3 reused).
        let sql = format!(
            "SELECT {CURVE_COLS} FROM curve_metric
             WHERE run_id = $1
               AND ($2::text IS NULL OR key = $2::text)
               AND ($3::bigint IS NULL OR step = $3::bigint)
             ORDER BY key ASC, step ASC, id ASC"
        );
        sqlx::query_as::<_, CurveRow>(&sql)
            .bind(run_id)
            .bind(key)
            .bind(step)
            .fetch_all(&self.pool)
            .await
    }

    async fn get_curve_one(
        &self,
        run_id: &str,
        key: &str,
        step: Option<i64>,
    ) -> Result<Option<CurveRow>, sqlx::Error> {
        let sql = format!(
            "SELECT {CURVE_COLS} FROM curve_metric
             WHERE run_id = $1 AND key = $2 AND ($3::bigint IS NULL OR step = $3::bigint)
             ORDER BY step DESC, id DESC
             LIMIT 1"
        );
        sqlx::query_as::<_, CurveRow>(&sql)
            .bind(run_id)
            .bind(key)
            .bind(step)
            .fetch_optional(&self.pool)
            .await
    }

    // ----- artifacts ----------------------------------------------------------
    async fn insert_artifact(
        &self,
        run_id: &str,
        name: &str,
        uri: &str,
        media_type: Option<&str>,
        size_bytes: Option<i64>,
    ) -> Result<Artifact, sqlx::Error> {
        let art = Artifact {
            id: new_id(),
            run_id: run_id.to_string(),
            name: name.to_string(),
            uri: uri.to_string(),
            media_type: media_type.map(|s| s.to_string()),
            size_bytes,
            created_at: now(),
        };
        sqlx::query(
            "INSERT INTO artifact (id, run_id, name, uri, media_type, size_bytes, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&art.id)
        .bind(&art.run_id)
        .bind(&art.name)
        .bind(&art.uri)
        .bind(&art.media_type)
        .bind(art.size_bytes)
        .bind(&art.created_at)
        .execute(&self.pool)
        .await?;
        Ok(art)
    }

    async fn get_artifacts(&self, run_id: &str) -> Result<Vec<Artifact>, sqlx::Error> {
        sqlx::query_as::<_, Artifact>(
            "SELECT id, run_id, name, uri, media_type, size_bytes, created_at
             FROM artifact WHERE run_id = $1 ORDER BY created_at ASC, id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await
    }
}
