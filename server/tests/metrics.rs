//! M7 — scalar metric path.
//!
//! Wire shape (see src/api/metrics.rs): POST `{metrics:[{key,step,value}]}` →
//! 202 `{accepted:n}`; GET → `{run_id, series:{key:[{step,value,ts}]}}` with
//! `series` sorted by key and each series ordered `step ASC, id ASC`; `?key=`
//! narrows to one series; logging to a finished run → 400 (immutability).

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
async fn bulk_log_accepts_and_counts() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    let (status, body) = app
        .post(
            &format!("/api/v1/runs/{run_id}/metrics"),
            serde_json::json!({
                "metrics": [
                    {"key": "loss", "step": 0, "value": 0.5},
                    {"key": "loss", "step": 1, "value": 0.4},
                    {"key": "acc", "step": 1, "value": 0.9},
                ]
            }),
        )
        .await;

    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], 3);
}

#[tokio::test]
async fn get_groups_and_orders() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    // Log out of order to prove the server sorts by step, not insertion order.
    app.post(
        &format!("/api/v1/runs/{run_id}/metrics"),
        serde_json::json!({
            "metrics": [
                {"key": "loss", "step": 2, "value": 0.3},
                {"key": "loss", "step": 0, "value": 0.5},
                {"key": "acc", "step": 0, "value": 0.8},
                {"key": "loss", "step": 1, "value": 0.4},
            ]
        }),
    )
    .await;

    let (status, body) = app.get(&format!("/api/v1/runs/{run_id}/metrics")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["run_id"], run_id);

    // Grouped by key.
    let loss = body["series"]["loss"].as_array().unwrap();
    let acc = body["series"]["acc"].as_array().unwrap();
    assert_eq!(loss.len(), 3);
    assert_eq!(acc.len(), 1);

    // Ordered step ASC within a key.
    let steps: Vec<i64> = loss.iter().map(|p| p["step"].as_i64().unwrap()).collect();
    assert_eq!(steps, vec![0, 1, 2]);
    assert_eq!(loss[0]["value"], 0.5);
    assert_eq!(loss[2]["value"], 0.3);

    // Server stamps a ts on each point.
    assert!(loss[0]["ts"].is_string());
}

#[tokio::test]
async fn key_filter() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    app.post(
        &format!("/api/v1/runs/{run_id}/metrics"),
        serde_json::json!({
            "metrics": [
                {"key": "loss", "step": 0, "value": 0.5},
                {"key": "acc", "step": 0, "value": 0.8},
            ]
        }),
    )
    .await;

    let (status, body) = app
        .get(&format!("/api/v1/runs/{run_id}/metrics?key=acc"))
        .await;
    assert_eq!(status, StatusCode::OK);

    // Only the requested series comes back.
    let series = body["series"].as_object().unwrap();
    assert_eq!(series.len(), 1);
    assert!(series.contains_key("acc"));
    assert!(!series.contains_key("loss"));
    assert_eq!(body["series"]["acc"][0]["value"], 0.8);
}

#[tokio::test]
async fn log_to_finished_run_is_rejected() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app).await;

    // Finish the run → it becomes immutable.
    let (status, _) = app
        .patch(
            &format!("/api/v1/runs/{run_id}"),
            serde_json::json!({"status": "finished"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = app
        .post(
            &format!("/api/v1/runs/{run_id}/metrics"),
            serde_json::json!({"metrics": [{"key": "loss", "step": 0, "value": 0.5}]}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
