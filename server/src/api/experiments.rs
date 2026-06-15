//! Experiment endpoints.

use crate::{error::AppError, models::*, repo, state::AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

/// POST /api/v1/experiments — get-or-create by name.
pub async fn create(
    State(st): State<AppState>,
    Json(body): Json<CreateExperiment>,
) -> Result<(StatusCode, Json<Experiment>), AppError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("experiment name is required".into()));
    }
    let exp = repo::get_or_create_experiment(&st.pool, name).await?;
    Ok((StatusCode::CREATED, Json(exp)))
}

/// GET /api/v1/experiments
pub async fn list(State(st): State<AppState>) -> Result<Json<Vec<Experiment>>, AppError> {
    Ok(Json(repo::list_experiments(&st.pool).await?))
}

/// GET /api/v1/experiments/{id}
pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Experiment>, AppError> {
    repo::get_experiment(&st.pool, &id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}
