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

/// Start a run under `experiment`, returning `(run_id, experiment_id)`.
async fn start_run(app: &TestApp, experiment: &str) -> (String, String) {
    let (_, run) = app
        .post("/api/v1/runs", serde_json::json!({"experiment": experiment}))
        .await;
    (
        run["run_id"].as_str().unwrap().to_string(),
        run["experiment_id"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn list_runs_filters_orders_and_caps() {
    let app = TestApp::spawn().await;

    // Two runs under exp-a (a1 then a2), one under exp-b.
    let (a1, exp_a) = start_run(&app, "exp-a").await;
    let (a2, _) = start_run(&app, "exp-a").await;
    let (_b1, exp_b) = start_run(&app, "exp-b").await;
    assert_ne!(exp_a, exp_b);

    // Finalize a1 so we can filter by status.
    app.patch(&format!("/api/v1/runs/{a1}"), serde_json::json!({"status": "finished"}))
        .await;

    // No filter → all three, newest-first (b1, a2, a1).
    let (status, all) = app.get("/api/v1/runs").await;
    assert_eq!(status, StatusCode::OK);
    let rows = all.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["id"], serde_json::json!(_b1));
    assert_eq!(rows[2]["id"], serde_json::json!(a1));

    // Filter by experiment → only exp-a's two runs.
    let (_, by_exp) = app.get(&format!("/api/v1/runs?experiment_id={exp_a}")).await;
    let exp_rows = by_exp.as_array().unwrap();
    assert_eq!(exp_rows.len(), 2);
    assert!(exp_rows.iter().all(|r| r["experiment_id"] == serde_json::json!(exp_a)));

    // Filter by status → only the finished one (a1).
    let (_, finished) = app.get("/api/v1/runs?status=finished").await;
    let fin_rows = finished.as_array().unwrap();
    assert_eq!(fin_rows.len(), 1);
    assert_eq!(fin_rows[0]["id"], serde_json::json!(a1));

    // Stacked filters + limit.
    let (_, running_a) = app.get(&format!("/api/v1/runs?experiment_id={exp_a}&status=running")).await;
    assert_eq!(running_a.as_array().unwrap().len(), 1); // only a2
    let (_, capped) = app.get("/api/v1/runs?limit=2").await;
    assert_eq!(capped.as_array().unwrap().len(), 2);

    // Unused id binding kept readable.
    let _ = a2;
}

#[tokio::test]
async fn list_runs_rejects_bad_status() {
    let app = TestApp::spawn().await;
    let (status, _) = app.get("/api/v1/runs?status=bogus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
