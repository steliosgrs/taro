//! Artifact endpoints (M5; streaming upload M9).
//!
//! `POST /runs/{id}/artifacts` accepts one of three bodies, picked by
//! `Content-Type`:
//!   - **raw stream** (any non-multipart, non-JSON type): the request body *is*
//!     the file bytes, streamed straight to the blob store. Name comes from the
//!     `?name=` query param, media type from `Content-Type`. This is the path the
//!     SDK uses — nothing is buffered whole in memory.
//!   - **`multipart/form-data`**: legacy upload (buffered per-field, then handed
//!     to the same streaming `put`). Kept for back-compat / ad-hoc clients.
//!   - **`application/json`**: register an existing URI (no bytes uploaded).
//!
//! Either way the DB stores only metadata; bytes never enter the DB. `GET` lists.

use crate::{blob::ByteStream, error::AppError, models::*, state::AppState};
use axum::{
    extract::{FromRequest, Multipart, Path, Query, Request, State},
    http::{header::CONTENT_TYPE, StatusCode},
    Json,
};
use bytes::Bytes;
use futures::{stream, TryStreamExt};
use serde::Deserialize;

#[derive(Deserialize)]
struct UploadParams {
    /// Artifact name for the streaming upload path (e.g. `best.pt`).
    name: Option<String>,
}

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

    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();

    let art = if content_type.starts_with("multipart/form-data") {
        upload_multipart(&st, &run, req).await?
    } else if content_type.starts_with("application/json") {
        register_uri(&st, &run_id, req).await?
    } else {
        upload_stream(&st, &run, &content_type, req).await?
    };
    Ok((StatusCode::CREATED, Json(art)))
}

/// Stream the raw request body to the blob store (the SDK's upload path).
async fn upload_stream(
    st: &AppState,
    run: &Run,
    content_type: &str,
    req: Request,
) -> Result<Artifact, AppError> {
    let params = Query::<UploadParams>::try_from_uri(req.uri())
        .map_err(|e| AppError::BadRequest(e.to_string()))?;
    let name = params
        .0
        .name
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("streamed upload requires ?name=".into()))?;
    let media_type = (!content_type.is_empty()).then(|| content_type.to_owned());

    // Body chunks flow request -> blob store without ever buffering whole.
    let body: ByteStream = Box::pin(req.into_body().into_data_stream().map_err(std::io::Error::other));
    let (uri, size) = st
        .blob
        .put(&run.experiment_id, &run.id, &name, body)
        .await
        .map_err(AppError::Other)?;

    Ok(st
        .store
        .insert_artifact(&run.id, &name, &uri, media_type.as_deref(), Some(size))
        .await?)
}

/// Legacy multipart upload: buffer the file part, then feed the same streaming
/// `put` via a one-shot stream so both paths share the blob-store contract.
async fn upload_multipart(st: &AppState, run: &Run, req: Request) -> Result<Artifact, AppError> {
    let mut mp =
        Multipart::from_request(req, st).await.map_err(|e| AppError::BadRequest(e.to_string()))?;

    let mut name_override: Option<String> = None;
    let mut file: Option<(String, Option<String>, Bytes)> = None; // name, media_type, bytes

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
        file = Some((fname.unwrap_or_else(|| "artifact.bin".into()), media, bytes));
    }

    let (fname, media, bytes) =
        file.ok_or_else(|| AppError::BadRequest("no file part in upload".into()))?;
    let name = name_override.unwrap_or(fname);

    let body: ByteStream = Box::pin(stream::once(async move { Ok(bytes) }));
    let (uri, size) = st
        .blob
        .put(&run.experiment_id, &run.id, &name, body)
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
