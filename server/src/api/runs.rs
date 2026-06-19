//! Run endpoints.

use crate::{error::AppError, models::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

/// Default page size for `GET /runs`, and the hard cap on a client-supplied limit.
const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;

/// POST /api/v1/runs — start a run (get-or-creates the experiment by name).
pub async fn create(
    State(st): State<AppState>,
    Json(body): Json<CreateRun>,
) -> Result<(StatusCode, Json<CreateRunResponse>), AppError> {
    let exp_name = body.experiment.trim();
    if exp_name.is_empty() {
        return Err(AppError::BadRequest("experiment is required".into()));
    }

    // Validate the inline config link up front so a bad id is a clean 400, not
    // an FK violation surfacing as a 500 inside create_run's transaction.
    if let Some(vid) = body.config_version_id.as_deref() {
        if st.store.get_version(vid).await?.is_none() {
            return Err(AppError::BadRequest(format!(
                "config_version_id '{vid}' not found"
            )));
        }
    }

    let exp = st.store.get_or_create_experiment(exp_name).await?;
    let run = st
        .store
        .create_run(
            &exp.id,
            body.name.as_deref(),
            &body.params,
            &body.tags,
            body.config_version_id.as_deref(),
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(CreateRunResponse {
            run_id: run.id,
            experiment_id: run.experiment_id,
            status: run.status,
        }),
    ))
}

/// GET /api/v1/runs — list runs newest-first; optional `?experiment_id=&status=&limit=`.
pub async fn list(
    State(st): State<AppState>,
    Query(q): Query<ListRunsQuery>,
) -> Result<Json<Vec<Run>>, AppError> {
    if let Some(s) = q.status.as_deref() {
        if !is_valid_status(s) {
            return Err(AppError::BadRequest(format!("invalid status '{s}'")));
        }
    }
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let runs = st
        .store
        .list_runs(q.experiment_id.as_deref(), q.status.as_deref(), limit)
        .await?;
    Ok(Json(runs))
}

/// GET /api/v1/runs/{id} — run detail incl. params and tags.
pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, AppError> {
    let run = st.store.get_run(&id).await?.ok_or(AppError::NotFound)?;
    let params = st.store.get_run_kv("param", &id).await?;
    let tags = st.store.get_run_kv("tag", &id).await?;
    Ok(Json(RunDetail { run, params, tags }))
}

/// PATCH /api/v1/runs/{id} — finalize / update status.
pub async fn patch(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateRun>,
) -> Result<Json<Run>, AppError> {
    if !is_valid_status(&body.status) {
        return Err(AppError::BadRequest(format!(
            "invalid status '{}'",
            body.status
        )));
    }
    st.store
        .update_run_status(&id, &body.status, body.ended_at.as_deref())
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}
