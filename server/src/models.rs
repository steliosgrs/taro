//! Domain rows (sqlx FromRow) and request/response DTOs.
//!
//! POC choice: ids and timestamps are `String` (uuid v7 string / RFC3339) so the
//! SQLite layer needs no extra sqlx type features. Stronger typing (Uuid /
//! DateTime) is a later refinement noted in the design.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ----- valid run statuses -----------------------------------------------------
pub const RUN_RUNNING: &str = "running";
pub const TERMINAL_STATUSES: [&str; 3] = ["finished", "failed", "killed"];

pub fn is_valid_status(s: &str) -> bool {
    s == RUN_RUNNING || TERMINAL_STATUSES.contains(&s)
}

// ----- rows -------------------------------------------------------------------
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Experiment {
    pub id: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Run {
    pub id: String,
    pub experiment_id: String,
    pub name: Option<String>,
    pub status: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

// ----- experiments DTOs -------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct CreateExperiment {
    pub name: String,
}

// ----- runs DTOs --------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct CreateRun {
    /// Experiment name; get-or-created.
    pub experiment: String,
    pub name: Option<String>,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct CreateRunResponse {
    pub run_id: String,
    pub experiment_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRun {
    /// New status; must be one of the terminal statuses for a finalize.
    pub status: String,
    /// Optional explicit end time; defaults to now() for terminal statuses.
    pub ended_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunDetail {
    #[serde(flatten)]
    pub run: Run,
    pub params: HashMap<String, String>,
    pub tags: HashMap<String, String>,
}

// ----- scalar metrics DTOs (M2) ----------------------------------------------
#[derive(Debug, Deserialize)]
pub struct ScalarMetricInput {
    pub key: String,
    pub step: i64,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct LogMetricsRequest {
    pub metrics: Vec<ScalarMetricInput>,
}

#[derive(Debug, Serialize)]
pub struct LogMetricsResponse {
    pub accepted: usize,
}

/// A scalar metric row as read back (used to build grouped series responses).
#[derive(Debug, sqlx::FromRow)]
pub struct MetricRow {
    pub key: String,
    pub step: i64,
    pub value: f64,
    pub ts: String,
}

// ----- curve metrics DTOs (M3 — the differentiator) ---------------------------
/// One named line in a multi-line curve (e.g. per-class PR), sharing the
/// record's `x`.
#[derive(Debug, Deserialize, Serialize)]
pub struct CurveSeries {
    pub name: String,
    pub y: Vec<f64>,
}

/// Curve payload: one shared `x`, plus **either** a single `y` **or** multiple
/// `series`. Optional `labels` name a categorical/index x-axis. Validated for
/// structure only (equal lengths, finite, non-empty) — never ML correctness.
#[derive(Debug, Deserialize, Serialize)]
pub struct CurveData {
    pub x: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<Vec<CurveSeries>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct CurveInput {
    pub key: String,
    pub step: i64,
    /// Open enum (`pr | roc | per_class | generic_xy`); never rejected.
    pub curve_type: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub data: CurveData,
}

#[derive(Debug, Deserialize)]
pub struct LogCurvesRequest {
    pub curves: Vec<CurveInput>,
}

/// A curve row as read back. `data` is the raw JSON text column; handlers parse
/// it back into nested JSON for responses. `run_id` is needed by `/curves/compare`.
#[derive(Debug, sqlx::FromRow)]
pub struct CurveRow {
    pub run_id: String,
    pub key: String,
    pub step: i64,
    pub curve_type: String,
    pub x_label: Option<String>,
    pub y_label: Option<String>,
    pub data: String,
    pub ts: String,
}
