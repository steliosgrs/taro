//! M7 — curve path + /curves/compare (the differentiator: a metric value can
//! be a curve, stored structured so N runs overlay).
//!
//! Wire shape (see src/api/curves.rs):
//! POST `{curves:[{key,step,curve_type,x_label?,y_label?,data:{x,y|series,labels?}}]}`
//!   → 202 `{accepted:n}`.
//! GET  `/runs/{id}/curves?key=&step=` → `{run_id, curves:[{key,step,curve_type,
//!   x_label,y_label,data,ts}]}` with `data` as nested JSON (not a string).
//! GET  `/curves/compare?run_ids=A,B&key=&step=` → `{key,x_label,y_label,
//!   runs:[{run_id,run_name,step,data}]}`; latest = max(step) per run; runs
//!   missing the curve are skipped. `step` asserted by ordering only, never
//!   units (Decision: opaque monotonic int).

mod common;
use axum::http::StatusCode;
use common::TestApp;

/// Start a fresh running run in `exp` (optionally named) and return its id.
async fn start_run(app: &TestApp, name: Option<&str>) -> String {
    let body = match name {
        Some(n) => serde_json::json!({"experiment": "exp", "name": n}),
        None => serde_json::json!({"experiment": "exp"}),
    };
    let (status, run) = app.post("/api/v1/runs", body).await;
    assert_eq!(status, StatusCode::CREATED);
    run["run_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn log_and_read_back() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app, None).await;

    let (status, body) = app
        .post(
            &format!("/api/v1/runs/{run_id}/curves"),
            serde_json::json!({
                "curves": [
                    {
                        "key": "pr_curve",
                        "step": 50,
                        "curve_type": "pr",
                        "x_label": "recall",
                        "y_label": "precision",
                        "data": {"x": [0.0, 0.5, 1.0], "y": [1.0, 0.8, 0.6]}
                    },
                    {
                        "key": "pr_per_class",
                        "step": 50,
                        "curve_type": "per_class",
                        "data": {
                            "x": [0.0, 1.0],
                            "series": [
                                {"name": "cat", "y": [1.0, 0.5]},
                                {"name": "dog", "y": [0.9, 0.4]}
                            ]
                        }
                    }
                ]
            }),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], 2);

    // Round-trip: GET all curves for the run.
    let (status, body) = app.get(&format!("/api/v1/runs/{run_id}/curves")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["run_id"], run_id);
    let curves = body["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 2);

    // ?key= narrows to one curve; data comes back as nested JSON, not a string.
    let (_, body) = app
        .get(&format!("/api/v1/runs/{run_id}/curves?key=pr_curve"))
        .await;
    let curves = body["curves"].as_array().unwrap();
    assert_eq!(curves.len(), 1);
    let c = &curves[0];
    assert_eq!(c["key"], "pr_curve");
    assert_eq!(c["curve_type"], "pr");
    assert_eq!(c["x_label"], "recall");
    assert!(c["data"].is_object());
    assert_eq!(c["data"]["x"], serde_json::json!([0.0, 0.5, 1.0]));
    assert_eq!(c["data"]["y"], serde_json::json!([1.0, 0.8, 0.6]));
    assert!(c["ts"].is_string());

    // The multi-series curve round-trips its nested structure too.
    let (_, body) = app
        .get(&format!("/api/v1/runs/{run_id}/curves?key=pr_per_class"))
        .await;
    let series = body["curves"][0]["data"]["series"].as_array().unwrap();
    assert_eq!(series.len(), 2);
    assert_eq!(series[0]["name"], "cat");
}

#[tokio::test]
async fn validation_rejects_malformed() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app, None).await;

    // Each case is a single malformed curve that must be rejected with 400.
    let cases = [
        // empty x
        serde_json::json!({"x": [], "y": []}),
        // both y and series
        serde_json::json!({"x": [0.0], "y": [1.0], "series": [{"name": "a", "y": [1.0]}]}),
        // neither y nor series
        serde_json::json!({"x": [0.0, 1.0]}),
        // y length mismatch
        serde_json::json!({"x": [0.0, 1.0], "y": [1.0]}),
        // labels length mismatch
        serde_json::json!({"x": [0.0, 1.0], "y": [1.0, 0.5], "labels": ["only-one"]}),
        // NOTE: the handler's non-finite guard isn't covered here — NaN/inf
        // aren't expressible in JSON, so serde_json rejects out-of-range numbers
        // (422) before the handler runs. The finite-check stays as defense in depth.
    ];

    for (i, data) in cases.iter().enumerate() {
        let (status, _) = app
            .post(
                &format!("/api/v1/runs/{run_id}/curves"),
                serde_json::json!({
                    "curves": [{"key": "k", "step": 0, "curve_type": "pr", "data": data}]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "case {i} should be rejected");
    }
}

#[tokio::test]
async fn compare_latest_overlays_runs() {
    let app = TestApp::spawn().await;
    let a = start_run(&app, Some("run-a")).await;
    let b = start_run(&app, Some("run-b")).await;

    // Run A logs the same key at two steps; latest = max(step) = 1.
    for (step, y) in [(0, 0.5), (1, 0.9)] {
        app.post(
            &format!("/api/v1/runs/{a}/curves"),
            serde_json::json!({
                "curves": [{
                    "key": "pr_curve", "step": step, "curve_type": "pr",
                    "x_label": "recall", "y_label": "precision",
                    "data": {"x": [0.0, 1.0], "y": [1.0, y]}
                }]
            }),
        )
        .await;
    }
    // Run B logs once.
    app.post(
        &format!("/api/v1/runs/{b}/curves"),
        serde_json::json!({
            "curves": [{
                "key": "pr_curve", "step": 0, "curve_type": "pr",
                "data": {"x": [0.0, 1.0], "y": [1.0, 0.3]}
            }]
        }),
    )
    .await;

    // Overlay A, B, plus a nonexistent run that must be silently skipped.
    let (status, body) = app
        .get(&format!(
            "/api/v1/curves/compare?run_ids={a},{b},ghost&key=pr_curve"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["key"], "pr_curve");
    // Axis labels carried from the first matched run.
    assert_eq!(body["x_label"], "recall");

    let runs = body["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2, "ghost run skipped, A and B overlaid");

    // A came back at its latest step with the corresponding data.
    let run_a = runs.iter().find(|r| r["run_id"] == a).unwrap();
    assert_eq!(run_a["step"], 1);
    assert_eq!(run_a["run_name"], "run-a");
    assert_eq!(run_a["data"]["y"], serde_json::json!([1.0, 0.9]));

    // Pinning an explicit step returns that step instead of the latest.
    let (_, body) = app
        .get(&format!("/api/v1/curves/compare?run_ids={a}&key=pr_curve&step=0"))
        .await;
    assert_eq!(body["runs"][0]["step"], 0);
    assert_eq!(body["runs"][0]["data"]["y"], serde_json::json!([1.0, 0.5]));
}

#[tokio::test]
async fn compare_bad_request() {
    let app = TestApp::spawn().await;
    let run_id = start_run(&app, None).await;

    // Empty run_ids → 400.
    let (status, _) = app
        .get("/api/v1/curves/compare?run_ids=&key=pr_curve")
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Missing key → 400.
    let (status, _) = app
        .get(&format!("/api/v1/curves/compare?run_ids={run_id}&key="))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Non-integer step → 400.
    let (status, _) = app
        .get(&format!(
            "/api/v1/curves/compare?run_ids={run_id}&key=pr_curve&step=abc"
        ))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
