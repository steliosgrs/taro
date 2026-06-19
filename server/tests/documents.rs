//! Slice 1 — versioned-document registry (Config Registry).
//!
//! Engine-generic like the rest of the M7 suite: runs on SQLite by default, and
//! against Postgres when `TARO_TEST_DATABASE_URL` is set (parity check).

mod common;
use axum::http::StatusCode;
use common::TestApp;
use serde_json::json;

/// Create a `config` document and return its id.
async fn make_document(app: &TestApp, name: &str) -> String {
    let (status, doc) = app
        .post("/api/v1/documents", json!({"namespace": "config", "name": name}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    doc["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn publish_is_content_addressed_and_dedups() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "yolo-baseline").await;
    let url = format!("/api/v1/documents/{doc}/versions");

    // v1
    let (status, v1) = app.post(&url, json!({"body": {"lr0": 0.01, "epochs": 100}})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["deduped"], false);

    // Re-publish identical content (keys reordered → must canonicalize equal).
    let (_, dup) = app.post(&url, json!({"body": {"epochs": 100, "lr0": 0.01}})).await;
    assert_eq!(dup["deduped"], true);
    assert_eq!(dup["version"], 1);
    assert_eq!(dup["version_id"], v1["version_id"]);
    assert_eq!(dup["content_hash"], v1["content_hash"]);

    // Changed content → new version.
    let (_, v2) = app.post(&url, json!({"body": {"lr0": 0.02, "epochs": 100}})).await;
    assert_eq!(v2["version"], 2);
    assert_eq!(v2["deduped"], false);
    assert_ne!(v2["content_hash"], v1["content_hash"]);
}

#[tokio::test]
async fn document_detail_lists_versions_and_get_or_create_is_idempotent() {
    let app = TestApp::spawn().await;

    // get-or-create: same (namespace,name) → same row.
    let d1 = make_document(&app, "dup").await;
    let d2 = make_document(&app, "dup").await;
    assert_eq!(d1, d2);

    let url = format!("/api/v1/documents/{d1}/versions");
    app.post(&url, json!({"body": {"a": 1}})).await;
    app.post(&url, json!({"body": {"a": 2}})).await;

    let (status, detail) = app.get(&format!("/api/v1/documents/{d1}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["namespace"], "config");
    assert_eq!(detail["name"], "dup");
    let versions = detail["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], 1);
    assert_eq!(versions[1]["version"], 2);
    // Summaries carry no body.
    assert!(versions[0].get("body").is_none());
}

#[tokio::test]
async fn version_detail_returns_nested_body() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "cfg").await;
    let (_, v) = app
        .post(&format!("/api/v1/documents/{doc}/versions"), json!({"body": {"nested": {"x": 1}}}))
        .await;
    let vid = v["version_id"].as_str().unwrap();

    let (status, out) = app.get(&format!("/api/v1/versions/{vid}")).await;
    assert_eq!(status, StatusCode::OK);
    // body comes back as JSON, not a string.
    assert!(out["body"].is_object());
    assert_eq!(out["body"]["nested"]["x"], 1);
    assert!(out["content_hash"].is_string());
    assert!(out["parent_version_id"].is_null());
}

#[tokio::test]
async fn dedup_is_scoped_per_document() {
    let app = TestApp::spawn().await;
    let a = make_document(&app, "doc-a").await;
    let b = make_document(&app, "doc-b").await;

    let body = json!({"body": {"same": true}});
    let (_, va) = app.post(&format!("/api/v1/documents/{a}/versions"), body.clone()).await;
    let (_, vb) = app.post(&format!("/api/v1/documents/{b}/versions"), body).await;

    // Identical content under different documents → distinct versions (not shared),
    // each starting at version 1, but the same content_hash.
    assert_ne!(va["version_id"], vb["version_id"]);
    assert_eq!(va["version"], 1);
    assert_eq!(vb["version"], 1);
    assert_eq!(va["content_hash"], vb["content_hash"]);
    assert_eq!(vb["deduped"], false);
}

#[tokio::test]
async fn publish_validates_structure_and_parent() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "v").await;
    let url = format!("/api/v1/documents/{doc}/versions");

    // Non-object bodies are rejected (structure-only check).
    let (status, _) = app.post(&url, json!({"body": [1, 2, 3]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app.post(&url, json!({"body": 42})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Unknown parent version → 400.
    let (status, _) = app
        .post(&url, json!({"body": {"ok": 1}, "parent_version_id": "nope"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn parent_version_id_records_lineage() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "lineage").await;
    let url = format!("/api/v1/documents/{doc}/versions");

    let (_, base) = app.post(&url, json!({"body": {"v": 1}})).await;
    let base_id = base["version_id"].as_str().unwrap();

    let (_, child) = app
        .post(&url, json!({"body": {"v": 2}, "parent_version_id": base_id}))
        .await;
    let child_id = child["version_id"].as_str().unwrap();

    let (_, out) = app.get(&format!("/api/v1/versions/{child_id}")).await;
    assert_eq!(out["parent_version_id"], base_id);
}

#[tokio::test]
async fn run_links_config_inline_with_both_direction_provenance() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "inline").await;
    let (_, v) = app
        .post(&format!("/api/v1/documents/{doc}/versions"), json!({"body": {"lr0": 0.01}}))
        .await;
    let vid = v["version_id"].as_str().unwrap().to_string();

    // Inline link at run start.
    let (status, run) = app
        .post("/api/v1/runs", json!({"experiment": "exp", "config_version_id": vid}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let run_id = run["run_id"].as_str().unwrap();

    // Forward: the run's linked documents.
    let (status, docs) = app.get(&format!("/api/v1/runs/{run_id}/documents")).await;
    assert_eq!(status, StatusCode::OK);
    let docs = docs.as_array().unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0]["role"], "config");
    assert_eq!(docs[0]["id"], vid);
    assert_eq!(docs[0]["body"]["lr0"], 0.01);

    // Reverse: the version's runs.
    let (status, runs) = app.get(&format!("/api/v1/versions/{vid}/runs")).await;
    assert_eq!(status, StatusCode::OK);
    let runs = runs.as_array().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["id"], run_id);
}

#[tokio::test]
async fn run_link_endpoint_and_validation() {
    let app = TestApp::spawn().await;
    let doc = make_document(&app, "endpoint").await;
    let (_, v) = app
        .post(&format!("/api/v1/documents/{doc}/versions"), json!({"body": {"a": 1}}))
        .await;
    let vid = v["version_id"].as_str().unwrap().to_string();

    let (_, run) = app.post("/api/v1/runs", json!({"experiment": "exp"})).await;
    let run_id = run["run_id"].as_str().unwrap().to_string();
    let link_url = format!("/api/v1/runs/{run_id}/documents");

    // Bad version id → 400.
    let (status, _) = app.post(&link_url, json!({"version_id": "nope", "role": "config"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Empty role → 400.
    let (status, _) = app.post(&link_url, json!({"version_id": vid, "role": " "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Valid → 204, then listed; re-linking is idempotent (still one).
    let (status, _) = app.post(&link_url, json!({"version_id": vid, "role": "config"})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = app.post(&link_url, json!({"version_id": vid, "role": "config"})).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, docs) = app.get(&link_url).await;
    assert_eq!(docs.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn create_run_with_unknown_config_version_is_rejected() {
    let app = TestApp::spawn().await;
    let (status, _) = app
        .post("/api/v1/runs", json!({"experiment": "exp", "config_version_id": "ghost"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn list_documents_filters_by_namespace() {
    let app = TestApp::spawn().await;
    make_document(&app, "c1").await;
    app.post("/api/v1/documents", json!({"namespace": "dataset", "name": "d1"})).await;

    let (status, all) = app.get("/api/v1/documents").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all.as_array().unwrap().len(), 2);

    let (_, configs) = app.get("/api/v1/documents?namespace=config").await;
    let configs = configs.as_array().unwrap();
    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0]["namespace"], "config");
}

#[tokio::test]
async fn missing_document_and_version_are_404() {
    let app = TestApp::spawn().await;
    let (status, _) = app.get("/api/v1/documents/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get("/api/v1/versions/ghost").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn document_requires_namespace_and_name() {
    let app = TestApp::spawn().await;
    let (status, _) = app.post("/api/v1/documents", json!({"namespace": " ", "name": "x"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app.post("/api/v1/documents", json!({"namespace": "config", "name": ""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
