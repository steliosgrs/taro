//! M7 — experiment + run lifecycle (working; proves the harness).

mod common;
use axum::http::StatusCode;
use common::TestApp;

#[tokio::test]
async fn create_experiment_is_idempotent() {
    let app = TestApp::spawn().await;

    let (s1, e1) = app.post("/api/v1/experiments", serde_json::json!({"name": "exp"})).await;
    let (s2, e2) = app.post("/api/v1/experiments", serde_json::json!({"name": "exp"})).await;

    assert_eq!(s1, StatusCode::CREATED);
    assert_eq!(s2, StatusCode::CREATED);
    // get-or-create: same name → same row.
    assert_eq!(e1["id"], e2["id"]);
    assert_eq!(e1["name"], "exp");
}

#[tokio::test]
async fn empty_experiment_name_is_rejected() {
    let app = TestApp::spawn().await;
    let (status, _) = app.post("/api/v1/experiments", serde_json::json!({"name": "  "})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_lifecycle_start_detail_finalize() {
    let app = TestApp::spawn().await;

    // start
    let (status, run) = app
        .post(
            "/api/v1/runs",
            serde_json::json!({
                "experiment": "exp",
                "name": "r1",
                "params": {"lr0": 0.01},
                "tags": {"owner": "stelios"},
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(run["status"], "running");
    let run_id = run["run_id"].as_str().unwrap().to_string();

    // detail echoes params/tags (params stringified by the store)
    let (status, detail) = app.get(&format!("/api/v1/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["status"], "running");
    assert_eq!(detail["params"]["lr0"], "0.01");
    assert_eq!(detail["tags"]["owner"], "stelios");

    // finalize → terminal status sticks
    let (status, _) = app
        .patch(&format!("/api/v1/runs/{run_id}"), serde_json::json!({"status": "finished"}))
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, detail) = app.get(&format!("/api/v1/runs/{run_id}")).await;
    assert_eq!(detail["status"], "finished");
    assert!(detail["ended_at"].is_string());
}

#[tokio::test]
async fn invalid_status_is_rejected() {
    let app = TestApp::spawn().await;
    let (_, run) = app
        .post("/api/v1/runs", serde_json::json!({"experiment": "exp"}))
        .await;
    let run_id = run["run_id"].as_str().unwrap();

    let (status, _) = app
        .patch(&format!("/api/v1/runs/{run_id}"), serde_json::json!({"status": "bogus"}))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unknown_run_is_404() {
    let app = TestApp::spawn().await;
    let (status, _) = app.get("/api/v1/runs/does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
