//! Experiment endpoints.

use crate::{error::AppError, models::*, state::AppState};
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
    let exp = st.store.get_or_create_experiment(name).await?;
    Ok((StatusCode::CREATED, Json(exp)))
}

/// GET /api/v1/experiments
pub async fn list(State(st): State<AppState>) -> Result<Json<Vec<Experiment>>, AppError> {
    Ok(Json(st.store.list_experiments().await?))
}

/// GET /api/v1/experiments/{id}
pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Experiment>, AppError> {
    st.store
        .get_experiment(&id)
        .await?
        .map(Json)
        .ok_or(AppError::NotFound)
}
