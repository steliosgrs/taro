//! Versioned-document registry endpoints (Slice 1: Config Registry).
//!
//! A `document` is a named handle in an open-enum `namespace`; a `document_version`
//! is an immutable, content-addressed snapshot of it. The server stores the body as
//! **opaque JSON, validated for structure only** (it must be a JSON object) — it
//! never interprets the config/recipe. Publishing is content-addressed: re-posting
//! identical content returns the existing version (`deduped: true`) instead of a new
//! one. `run_document` links a version to a run under a `role`, giving provenance in
//! both directions (a run's configs, and a version's runs).

use crate::{error::AppError, models::*, state::AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
pub struct ListDocumentsQuery {
    pub namespace: Option<String>,
    pub name: Option<String>,
}

/// A version as returned to clients: `body` is nested JSON, not a string.
#[derive(Debug, Serialize)]
pub struct VersionOut {
    pub id: String,
    pub document_id: String,
    pub version: i64,
    pub content_hash: String,
    pub body: serde_json::Value,
    pub parent_version_id: Option<String>,
    pub created_at: String,
}

/// A version without its (potentially large) body — used in document listings.
#[derive(Debug, Serialize)]
pub struct VersionSummary {
    pub id: String,
    pub version: i64,
    pub content_hash: String,
    pub parent_version_id: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct DocumentDetail {
    #[serde(flatten)]
    pub document: Document,
    pub versions: Vec<VersionSummary>,
}

/// A version linked to a run, tagged with the role it was linked under.
#[derive(Debug, Serialize)]
pub struct RunDocumentOut {
    pub role: String,
    #[serde(flatten)]
    pub version: VersionOut,
}

// ----- documents --------------------------------------------------------------

/// POST /api/v1/documents — get-or-create a named handle in a namespace.
pub async fn create(
    State(st): State<AppState>,
    Json(body): Json<CreateDocument>,
) -> Result<(StatusCode, Json<Document>), AppError> {
    let namespace = body.namespace.trim();
    let name = body.name.trim();
    if namespace.is_empty() {
        return Err(AppError::BadRequest("namespace is required".into()));
    }
    if name.is_empty() {
        return Err(AppError::BadRequest("name is required".into()));
    }
    let doc = st.store.get_or_create_document(namespace, name).await?;
    Ok((StatusCode::CREATED, Json(doc)))
}

/// GET /api/v1/documents?namespace=&name= — list handles (both filters optional).
pub async fn list(
    State(st): State<AppState>,
    Query(q): Query<ListDocumentsQuery>,
) -> Result<Json<Vec<Document>>, AppError> {
    let docs = st
        .store
        .list_documents(q.namespace.as_deref(), q.name.as_deref())
        .await?;
    Ok(Json(docs))
}

/// GET /api/v1/documents/{id} — handle plus its version history (summaries).
pub async fn get(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<DocumentDetail>, AppError> {
    let document = st.store.get_document(&id).await?.ok_or(AppError::NotFound)?;
    let versions = st
        .store
        .list_versions(&id)
        .await?
        .into_iter()
        .map(|v| VersionSummary {
            id: v.id,
            version: v.version,
            content_hash: v.content_hash,
            parent_version_id: v.parent_version_id,
            created_at: v.created_at,
        })
        .collect();
    Ok(Json(DocumentDetail { document, versions }))
}

/// POST /api/v1/documents/{id}/versions — publish a version (content-addressed).
pub async fn publish(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<PublishVersion>,
) -> Result<(StatusCode, Json<PublishVersionResponse>), AppError> {
    st.store.get_document(&id).await?.ok_or(AppError::NotFound)?;

    // Structure-only: the body must be a JSON object. The server never inspects
    // its keys — meaning lives in the adapters that consume it.
    if !body.body.is_object() {
        return Err(AppError::BadRequest("body must be a JSON object".into()));
    }

    // Lineage edge must point at a real version when supplied.
    if let Some(pid) = body.parent_version_id.as_deref() {
        if st.store.get_version(pid).await?.is_none() {
            return Err(AppError::BadRequest(format!(
                "parent_version_id '{pid}' not found"
            )));
        }
    }

    let (canonical, hash) = canonical_and_hash(&body.body)?;
    let (version, deduped) = st
        .store
        .publish_version(&id, &hash, &canonical, body.parent_version_id.as_deref())
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(PublishVersionResponse {
            version_id: version.id,
            version: version.version,
            content_hash: version.content_hash,
            deduped,
        }),
    ))
}

// ----- versions ---------------------------------------------------------------

/// GET /api/v1/versions/{id} — a single version with its body as nested JSON.
pub async fn get_version(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<VersionOut>, AppError> {
    let v = st.store.get_version(&id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(version_out(v)?))
}

/// GET /api/v1/versions/{id}/runs — reverse lookup: runs launched from this version.
pub async fn version_runs(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<Run>>, AppError> {
    st.store.get_version(&id).await?.ok_or(AppError::NotFound)?;
    let runs = st.store.list_runs_for_version(&id).await?;
    Ok(Json(runs))
}

// ----- run links --------------------------------------------------------------

/// POST /api/v1/runs/{id}/documents — link a version to a run under a role.
pub async fn link_run(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
    Json(body): Json<LinkDocument>,
) -> Result<StatusCode, AppError> {
    st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;

    let role = body.role.trim();
    if role.is_empty() {
        return Err(AppError::BadRequest("role is required".into()));
    }
    if st.store.get_version(&body.version_id).await?.is_none() {
        return Err(AppError::BadRequest(format!(
            "version_id '{}' not found",
            body.version_id
        )));
    }

    st.store
        .link_run_document(&run_id, &body.version_id, role)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/runs/{id}/documents — versions this run was launched from.
pub async fn list_run_documents(
    State(st): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Vec<RunDocumentOut>>, AppError> {
    st.store.get_run(&run_id).await?.ok_or(AppError::NotFound)?;
    let links = st.store.list_run_documents(&run_id).await?;
    let out = links
        .into_iter()
        .map(|(role, v)| {
            Ok(RunDocumentOut {
                role,
                version: version_out(v)?,
            })
        })
        .collect::<Result<_, AppError>>()?;
    Ok(Json(out))
}

// ----- helpers ----------------------------------------------------------------

/// Canonicalize a JSON body (serde_json's default `Map` is a `BTreeMap`, so keys
/// serialize sorted at every level → stable bytes) and sha256 it to hex.
fn canonical_and_hash(body: &serde_json::Value) -> Result<(String, String), AppError> {
    let canonical = serde_json::to_string(body).map_err(|e| AppError::Other(e.into()))?;
    let digest = Sha256::digest(canonical.as_bytes());
    let hash = digest.iter().map(|b| format!("{b:02x}")).collect();
    Ok((canonical, hash))
}

fn version_out(v: DocumentVersion) -> Result<VersionOut, AppError> {
    let body = serde_json::from_str(&v.body).map_err(|e| AppError::Other(e.into()))?;
    Ok(VersionOut {
        id: v.id,
        document_id: v.document_id,
        version: v.version,
        content_hash: v.content_hash,
        body,
        parent_version_id: v.parent_version_id,
        created_at: v.created_at,
    })
}
