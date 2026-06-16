//! Artifact endpoints (M5).
//!
//! `POST /runs/{id}/artifacts` accepts **either** a multipart upload (bytes →
//! blob store) **or** a JSON body registering an existing URI (no bytes). Either
//! way the DB stores only metadata; bytes never enter the DB. `GET` lists them.

use crate::{error::AppError, models::*, state::AppState};
use axum::{
    extract::{FromRequest, Multipart, Path, Request, State},
    http::{header::CONTENT_TYPE, StatusCode},
    Json,
};

/// POST /api/v1/runs/{id}/artifacts
pub async fn create(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    req: Request,
) -> Result<(StatusCode, Json<Artifact>), AppError> {
    let run = st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;

    // Same invariant as metrics/curves: only a running run accepts logging.
    if run.status != RUN_RUNNING {
        return Err(AppError::BadRequest(format!(
            "run is '{}', not running; cannot log artifacts",
            run.status
        )));
    }

    let is_multipart = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("multipart/form-data"))
        .unwrap_or(false);

    let art = if is_multipart {
        upload_multipart(&st, &run, req).await?
    } else {
        register_uri(&st, &run_id, req).await?
    };
    Ok((StatusCode::CREATED, Json(art)))
}

/// Stream the multipart body, store the file part's bytes, record metadata.
async fn upload_multipart(st: &AppState, run: &Run, req: Request) -> Result<Artifact, AppError> {
    let mut mp =
        Multipart::from_request(req, st).await.map_err(|e| AppError::BadRequest(e.to_string()))?;

    let mut name_override: Option<String> = None;
    let mut file: Option<(String, Option<String>, Vec<u8>)> = None; // name, media_type, bytes

    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?
    {
        if field.name() == Some("name") {
            name_override =
                Some(field.text().await.map_err(|e| AppError::BadRequest(e.to_string()))?);
            continue;
        }
        // Any other part is the file payload.
        let fname = field.file_name().map(String::from);
        let media = field.content_type().map(String::from);
        let bytes = field.bytes().await.map_err(|e| AppError::BadRequest(e.to_string()))?;
        file = Some((fname.unwrap_or_else(|| "artifact.bin".into()), media, bytes.to_vec()));
    }

    let (fname, media, bytes) =
        file.ok_or_else(|| AppError::BadRequest("no file part in upload".into()))?;
    let name = name_override.unwrap_or(fname);
    let size = bytes.len() as i64;

    let uri = st
        .blob
        .put(&run.experiment_id, &run.id, &name, &bytes)
        .await
        .map_err(AppError::Other)?;

    Ok(st
        .store
        .insert_artifact(&run.id, &name, &uri, media.as_deref(), Some(size))
        .await?)
}

/// Register an artifact that already lives at a URI (no bytes uploaded).
async fn register_uri(st: &AppState, run_id: &str, req: Request) -> Result<Artifact, AppError> {
    let Json(body): Json<ArtifactRegister> =
        Json::from_request(req, st).await.map_err(|e| AppError::BadRequest(e.to_string()))?;

    if body.name.trim().is_empty() || body.uri.trim().is_empty() {
        return Err(AppError::BadRequest("name and uri are required".into()));
    }
    Ok(st
        .store
        .insert_artifact(
            run_id,
            &body.name,
            &body.uri,
            body.media_type.as_deref(),
            body.size_bytes,
        )
        .await?)
}

/// GET /api/v1/runs/{id}/artifacts
pub async fn list(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Vec<Artifact>>, AppError> {
    st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(st.store.get_artifacts(&run_id).await?))
}
