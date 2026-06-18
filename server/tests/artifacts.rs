//! M7 — artifacts + blob store.
//!
//! Wire shape (see src/api/artifacts.rs): POST `/runs/{id}/artifacts` is either
//! a multipart upload (bytes → blob store, size derived from bytes) OR a JSON
//! body registering an existing URI (no bytes). Either way the DB stores only
//! metadata. GET lists. The blob layout strips a name to its basename, so a
//! traversal name (`../x`) is confined under the blob root.

mod common;
use axum::http::StatusCode;
use common::TestApp;

/// Start a fresh running run and return its id.
async fn start_run(app: &TestApp) -> String {
    let (status, run) = app
        .post("/api/v1/runs", serde_json::json!({"experiment": "exp"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    run["run_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn multipart_upload_persists() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    let bytes = b"\x00\x01weights-bytes\xff";
    let (status, art) = app
        .post_multipart(
            &format!("/api/v1/runs/{run_id}/artifacts"),
            "best.pt",
            Some("application/octet-stream"),
            bytes,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Metadata recorded; size derived from the uploaded bytes; uri points at the blob.
    assert_eq!(art["run_id"], run_id);
    assert_eq!(art["name"], "best.pt");
    assert_eq!(art["media_type"], "application/octet-stream");
    assert_eq!(art["size_bytes"], bytes.len() as i64);
    let uri = art["uri"].as_str().unwrap();
    assert!(uri.starts_with("file://"), "blob uri, got {uri}");

    // GET lists exactly the one artifact.
    let (status, list) = app.get(&format!("/api/v1/runs/{run_id}/artifacts")).await;
    assert_eq!(status, StatusCode::OK);
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "best.pt");
}

#[tokio::test]
async fn json_uri_register() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    // No bytes — just register an existing URI (e.g. an S3 object).
    let (status, art) = app
        .post(
            &format!("/api/v1/runs/{run_id}/artifacts"),
            serde_json::json!({
                "name": "dataset.tar",
                "uri": "s3://bucket/datasets/dataset.tar",
                "media_type": "application/x-tar",
                "size_bytes": 4096
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(art["uri"], "s3://bucket/datasets/dataset.tar");
    assert_eq!(art["size_bytes"], 4096);

    let (_, list) = app.get(&format!("/api/v1/runs/{run_id}/artifacts")).await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["name"], "dataset.tar");
}

#[tokio::test]
async fn path_traversal_is_confined() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    // A malicious name must not escape the blob root: LocalFs stores under the
    // basename only, so the stored uri ends in the basename, not a parent path.
    let (status, art) = app
        .post_multipart(
            &format!("/api/v1/runs/{run_id}/artifacts"),
            "../../../../etc/passwd",
            None,
            b"pwned",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let uri = art["uri"].as_str().unwrap();
    assert!(uri.ends_with("/passwd"), "stored at basename, got {uri}");
    assert!(!uri.contains("/etc/passwd"), "must not escape root, got {uri}");
}

#[tokio::test]
async fn upload_to_finished_run_is_rejected() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    let (status, _) = app
        .patch(
            &format!("/api/v1/runs/{run_id}"),
            serde_json::json!({"status": "finished"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Finished runs are immutable — even a valid URI register is rejected.
    let (status, _) = app
        .post(
            &format!("/api/v1/runs/{run_id}/artifacts"),
            serde_json::json!({"name": "late.txt", "uri": "s3://b/late.txt"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
