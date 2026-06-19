//! Shared M7 integration-test harness.
//!
//! Engine-generic by design: the router is built over an `Arc<dyn Store>` via
//! [`TestApp::spawn`]. By default that's `SqliteStore` on a throwaway temp-file
//! DB. Set `TARO_TEST_DATABASE_URL` to a Postgres URL and the *same* suite runs
//! against `PgStore` instead — the M8 parity check:
//!
//! ```text
//! TARO_TEST_DATABASE_URL=postgres://taro:taro@localhost:5434/taro cargo test
//! ```
//!
//! Each `TestApp` is isolated: SQLite gets its own temp file; Postgres gets its
//! own uniquely-named schema (search_path), so concurrent tests don't collide.
//! (PG schemas are left behind — intended for a throwaway/ephemeral test DB.)
//!
//! `common` is compiled into each test binary separately, so helpers unused by
//! a given file would warn as dead code — allow it crate-wide for the harness.
#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use taro_server::{
    api,
    blob::LocalFs,
    db,
    state::AppState,
    store::{PgStore, SqliteStore, Store},
};
use tempfile::TempDir;
use tower::ServiceExt; // oneshot

pub struct TestApp {
    router: Router,
    // Held only to keep the temp dirs alive for the test's lifetime.
    _db_dir: TempDir,
    _blob_dir: TempDir,
}

/// A schema name unique across processes (pid) and within one (counter).
fn unique_schema() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("taro_test_{}_{}", std::process::id(), n)
}

/// The one place that picks a storage engine — SQLite by default, Postgres when
/// `TARO_TEST_DATABASE_URL` is set (M8 parity run).
async fn build_store(db_dir: &TempDir) -> Arc<dyn Store> {
    match std::env::var("TARO_TEST_DATABASE_URL") {
        Ok(base_url) if db::is_postgres(&base_url) => build_pg_store(&base_url).await,
        _ => {
            let path = db_dir.path().join("test.db");
            let url = format!("sqlite://{}", path.display());
            let pool = db::connect(&url).await.expect("connect + migrate temp db");
            Arc::new(SqliteStore::new(pool))
        }
    }
}

/// Build a `PgStore` confined to a fresh schema so each test is isolated.
async fn build_pg_store(base_url: &str) -> Arc<dyn Store> {
    let schema = unique_schema();
    let s = schema.clone();
    // Every pooled connection scopes itself to this test's schema.
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |conn, _meta| {
            let s = s.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO \"{s}\""))
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect(base_url)
        .await
        .expect("connect postgres");

    sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS \"{schema}\""))
        .execute(&pool)
        .await
        .expect("create test schema");
    sqlx::migrate!("./migrations_pg")
        .run(&pool)
        .await
        .expect("run pg migrations");

    Arc::new(PgStore::new(pool))
}

impl TestApp {
    /// Spawn the router with auth disabled (the POC default).
    pub async fn spawn() -> Self {
        Self::build(None).await
    }

    /// Spawn the router with a required bearer token.
    pub async fn spawn_with_auth(api_key: &str) -> Self {
        Self::build(Some(api_key.to_string())).await
    }

    async fn build(api_key: Option<String>) -> Self {
        let db_dir = tempfile::tempdir().expect("temp db dir");
        let blob_dir = tempfile::tempdir().expect("temp blob dir");
        let state = AppState {
            store: build_store(&db_dir).await,
            api_key,
            blob: Arc::new(LocalFs::new(blob_dir.path())),
        };
        Self {
            router: api::router(state),
            _db_dir: db_dir,
            _blob_dir: blob_dir,
        }
    }

    /// Send a request and return `(status, json_body)`. A non-JSON/empty body
    /// comes back as `serde_json::Value::Null`.
    pub async fn request(
        &self,
        method: &str,
        uri: &str,
        bearer: Option<&str>,
        json: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        let req = match json {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };

        let resp = self
            .router
            .clone()
            .oneshot(req)
            .await
            .expect("router oneshot");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    // --- Convenience verbs (no auth) ---------------------------------------

    pub async fn get(&self, uri: &str) -> (StatusCode, serde_json::Value) {
        self.request("GET", uri, None, None).await
    }

    pub async fn post(
        &self,
        uri: &str,
        json: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        self.request("POST", uri, None, Some(json)).await
    }

    pub async fn patch(
        &self,
        uri: &str,
        json: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        self.request("PATCH", uri, None, Some(json)).await
    }

    /// POST a `multipart/form-data` upload with one file part (`filename` +
    /// `bytes`, optional `media_type`) and an optional `name` text part. Mirrors
    /// what the SDK sends so the artifact upload path is exercised end-to-end.
    pub async fn post_multipart(
        &self,
        uri: &str,
        filename: &str,
        media_type: Option<&str>,
        bytes: &[u8],
        name_override: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        const BOUNDARY: &str = "TAROTESTBOUNDARY";
        let mut body: Vec<u8> = Vec::new();

        // File part — any field name other than "name" is treated as the file.
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
                .as_bytes(),
        );
        if let Some(ct) = media_type {
            body.extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");

        // Optional name override part.
        if let Some(name) = name_override {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(b"Content-Disposition: form-data; name=\"name\"\r\n\r\n");
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", format!("multipart/form-data; boundary={BOUNDARY}"))
            .body(Body::from(body))
            .unwrap();

        let resp = self.router.clone().oneshot(req).await.expect("router oneshot");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    /// POST a raw streaming upload (M9): the body *is* the file bytes; the name
    /// rides in `?name=` and the media type in `Content-Type`. Mirrors the SDK.
    pub async fn post_stream(
        &self,
        uri: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", media_type)
            .body(Body::from(bytes.to_vec()))
            .unwrap();

        let resp = self.router.clone().oneshot(req).await.expect("router oneshot");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }
}
