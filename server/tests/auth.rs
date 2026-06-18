//! M7 — bearer-token auth stub (working).

mod common;
use axum::http::StatusCode;
use common::TestApp;

#[tokio::test]
async fn missing_and_wrong_token_are_401() {
    let app = TestApp::spawn_with_auth("s3cret").await;

    let (missing, _) = app.request("GET", "/api/v1/experiments", None, None).await;
    assert_eq!(missing, StatusCode::UNAUTHORIZED);

    let (wrong, _) = app
        .request("GET", "/api/v1/experiments", Some("nope"), None)
        .await;
    assert_eq!(wrong, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn correct_token_passes() {
    let app = TestApp::spawn_with_auth("s3cret").await;
    let (status, _) = app
        .request("GET", "/api/v1/experiments", Some("s3cret"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn health_is_unprotected() {
    let app = TestApp::spawn_with_auth("s3cret").await;
    let (status, _) = app.request("GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
}
