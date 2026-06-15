//! Run endpoints.

use crate::{error::AppError, models::*, repo, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

/// POST /api/v1/runs — start a run (get-or-creates the experiment by name).
pub async fn create(
    State(st): State<AppState>,
    Json(body): Json<CreateRun>,
) -> Result<(StatusCode, Json<CreateRunResponse>), AppError> {
    let exp_name = body.experiment.trim();
    if exp_name.is_empty() {
        return Err(AppError::BadRequest("experiment is required".into()));
    }

    let exp = repo::get_or_create_experiment(&st.pool, exp_name).await?;
    let run = repo::create_run(
        &st.pool,
        &exp.id,
        body.name.as_deref(),
        &body.params,
        &body.tags,
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

/// GET /api/v1/runs/{id} — run detail incl. params and tags.
pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, AppError> {
    let run = repo::get_run(&st.pool, &id).await?.ok_or(AppError::NotFound)?;
    let params = repo::get_run_kv(&st.pool, "param", &id).await?;
    let tags = repo::get_run_kv(&st.pool, "tag", &id).await?;
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
    repo::update_run_status(&st.pool, &id, &body.status, body.ended_at.as_deref())
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}
