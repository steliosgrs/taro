//! Curve metric endpoints (M3) — the differentiator: a metric value can be a
//! curve/vector, stored as structured data so N runs can be overlaid.

use crate::{error::AppError, models::*, repo, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CurveQuery {
    /// Optional: restrict to a single curve key.
    pub key: Option<String>,
    /// Optional: restrict to a single step.
    pub step: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CompareQuery {
    /// Comma-separated run ids to overlay.
    pub run_ids: String,
    pub key: String,
    /// Step number, or `latest` / omitted for the highest step per run.
    pub step: Option<String>,
}

/// A curve as returned to clients: `data` is nested JSON, not a string.
#[derive(Debug, Serialize)]
pub struct CurveOut {
    pub key: String,
    pub step: i64,
    pub curve_type: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub data: serde_json::Value,
    pub ts: String,
}

#[derive(Debug, Serialize)]
pub struct CurvesResponse {
    pub run_id: String,
    pub curves: Vec<CurveOut>,
}

#[derive(Debug, Serialize)]
pub struct CompareRun {
    pub run_id: String,
    pub run_name: Option<String>,
    pub step: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct CompareResponse {
    pub key: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub runs: Vec<CompareRun>,
}

/// POST /api/v1/runs/{id}/curves — log curve metric(s).
pub async fn log(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    Json(body): Json<LogCurvesRequest>,
) -> Result<(StatusCode, Json<LogMetricsResponse>), AppError> {
    let run = repo::get_run(&st.pool, &run_id)
        .await?
        .ok_or(AppError::NotFound)?;

    // Same invariant as scalars: only a running run accepts metrics.
    if run.status != RUN_RUNNING {
        return Err(AppError::BadRequest(format!(
            "run is '{}', not running; cannot log curves",
            run.status
        )));
    }

    for c in &body.curves {
        if c.key.trim().is_empty() {
            return Err(AppError::BadRequest("curve key is required".into()));
        }
        // curve_type is an open enum (never rejected) but must be present.
        if c.curve_type.trim().is_empty() {
            return Err(AppError::BadRequest("curve_type is required".into()));
        }
        validate_curve_data(&c.key, &c.data)?;
    }

    let accepted = repo::insert_curve_metrics(&st.pool, &run_id, &body.curves).await?;
    Ok((StatusCode::ACCEPTED, Json(LogMetricsResponse { accepted })))
}

/// GET /api/v1/runs/{id}/curves?key=&step= — a run's curves.
pub async fn list(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    Query(q): Query<CurveQuery>,
) -> Result<Json<CurvesResponse>, AppError> {
    repo::get_run(&st.pool, &run_id)
        .await?
        .ok_or(AppError::NotFound)?;

    let rows = repo::get_curve_metrics(&st.pool, &run_id, q.key.as_deref(), q.step).await?;
    let curves = rows.into_iter().map(curve_out).collect::<Result<_, _>>()?;
    Ok(Json(CurvesResponse { run_id, curves }))
}

/// GET /api/v1/curves/compare?run_ids=A,B&key=&step= — overlay N runs' curves
/// for one key. Returns comparable data, never an image. Runs missing the curve
/// are silently skipped so a partial overlay still renders.
pub async fn compare(
    State(st): State<AppState>,
    Query(q): Query<CompareQuery>,
) -> Result<Json<CompareResponse>, AppError> {
    let key = q.key.trim();
    if key.is_empty() {
        return Err(AppError::BadRequest("key is required".into()));
    }

    // "latest" (or omitted) -> None (max step per run); else a concrete step.
    let step: Option<i64> = match q.step.as_deref() {
        None | Some("latest") => None,
        Some(s) => Some(
            s.parse::<i64>()
                .map_err(|_| AppError::BadRequest(format!("invalid step '{s}'")))?,
        ),
    };

    let run_ids: Vec<&str> = q
        .run_ids
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if run_ids.is_empty() {
        return Err(AppError::BadRequest("run_ids is required".into()));
    }

    let mut x_label = None;
    let mut y_label = None;
    let mut runs = Vec::new();
    for rid in run_ids {
        let Some(row) = repo::get_curve_one(&st.pool, rid, key, step).await? else {
            continue; // run absent or has no curve for this key/step
        };
        // Axis labels come from the first matched curve (they agree across runs).
        if runs.is_empty() {
            x_label = row.x_label.clone();
            y_label = row.y_label.clone();
        }
        let run_name = repo::get_run(&st.pool, rid).await?.and_then(|r| r.name);
        runs.push(CompareRun {
            run_id: row.run_id,
            run_name,
            step: row.step,
            data: parse_data(&row.data)?,
        });
    }

    Ok(Json(CompareResponse {
        key: key.to_string(),
        x_label,
        y_label,
        runs,
    }))
}

// ----- helpers ----------------------------------------------------------------

fn all_finite(xs: &[f64]) -> bool {
    xs.iter().all(|v| v.is_finite())
}

/// Structure-only validation: equal lengths, non-empty, finite, exactly one of
/// `y`/`series`. Never judges ML correctness.
fn validate_curve_data(key: &str, d: &CurveData) -> Result<(), AppError> {
    let bad = |m: String| Err(AppError::BadRequest(format!("curve '{key}': {m}")));

    let n = d.x.len();
    if n == 0 {
        return bad("x is empty".into());
    }
    if !all_finite(&d.x) {
        return bad("x has non-finite values".into());
    }

    match (&d.y, &d.series) {
        (Some(_), Some(_)) => return bad("provide exactly one of 'y' or 'series'".into()),
        (None, None) => return bad("one of 'y' or 'series' is required".into()),
        (Some(y), None) => {
            if y.len() != n {
                return bad(format!("y length {} != x length {n}", y.len()));
            }
            if !all_finite(y) {
                return bad("y has non-finite values".into());
            }
        }
        (None, Some(series)) => {
            if series.is_empty() {
                return bad("series is empty".into());
            }
            for s in series {
                if s.name.trim().is_empty() {
                    return bad("a series is missing a name".into());
                }
                if s.y.len() != n {
                    return bad(format!("series '{}' length {} != x length {n}", s.name, s.y.len()));
                }
                if !all_finite(&s.y) {
                    return bad(format!("series '{}' has non-finite values", s.name));
                }
            }
        }
    }

    if let Some(labels) = &d.labels {
        if labels.len() != n {
            return bad(format!("labels length {} != x length {n}", labels.len()));
        }
    }
    Ok(())
}

/// Parse a stored `data` JSON string into nested JSON for a response.
fn parse_data(s: &str) -> Result<serde_json::Value, AppError> {
    serde_json::from_str(s).map_err(|e| AppError::Other(e.into()))
}

fn curve_out(row: CurveRow) -> Result<CurveOut, AppError> {
    Ok(CurveOut {
        data: parse_data(&row.data)?,
        key: row.key,
        step: row.step,
        curve_type: row.curve_type,
        x_label: row.x_label,
        y_label: row.y_label,
        ts: row.ts,
    })
}
