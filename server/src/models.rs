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
