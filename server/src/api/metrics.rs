//! Scalar metric endpoints (M2).

use crate::{error::AppError, models::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
pub struct MetricQuery {
    /// Optional: restrict to a single metric key.
    pub key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Point {
    pub step: i64,
    pub value: f64,
    pub ts: String,
}

#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub run_id: String,
    /// metric key -> ordered points.
    pub series: BTreeMap<String, Vec<Point>>,
}

/// POST /api/v1/runs/{id}/metrics — bulk-log scalar metrics.
pub async fn log(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    Json(body): Json<LogMetricsRequest>,
) -> Result<(StatusCode, Json<LogMetricsResponse>), AppError> {
    let run = st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;

    // Invariant: only a running run accepts metrics.
    if run.status != RUN_RUNNING {
        return Err(AppError::BadRequest(format!(
            "run is '{}', not running; cannot log metrics",
            run.status
        )));
    }

    for m in &body.metrics {
        if m.key.trim().is_empty() {
            return Err(AppError::BadRequest("metric key is required".into()));
        }
        if !m.value.is_finite() {
            return Err(AppError::BadRequest(format!(
                "metric '{}' has non-finite value",
                m.key
            )));
        }
    }

    let accepted = st.store.insert_scalar_metrics(&run_id, &body.metrics).await?;
    Ok((StatusCode::ACCEPTED, Json(LogMetricsResponse { accepted })))
}

/// GET /api/v1/runs/{id}/metrics?key= — scalar series, grouped by key.
pub async fn list(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    Query(q): Query<MetricQuery>,
) -> Result<Json<MetricsResponse>, AppError> {
    // 404 if the run doesn't exist (vs. silently returning empty).
    st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;

    let rows = st.store.get_scalar_metrics(&run_id, q.key.as_deref()).await?;

    let mut series: BTreeMap<String, Vec<Point>> = BTreeMap::new();
    for r in rows {
        series.entry(r.key).or_default().push(Point {
            step: r.step,
            value: r.value,
            ts: r.ts,
        });
    }

    Ok(Json(MetricsResponse { run_id, series }))
}
