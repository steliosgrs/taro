//! HTTP router assembly.

pub mod artifacts;
pub mod curves;
pub mod documents;
pub mod experiments;
pub mod health;
pub mod metrics;
pub mod runs;

use crate::{auth, state::AppState};
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> Router {
    // /api/v1 — protected by the auth stub.
    let api = Router::new()
        .route("/experiments", post(experiments::create).get(experiments::list))
        .route("/experiments/{id}", get(experiments::get))
        .route("/runs", post(runs::create).get(runs::list))
        .route("/runs/{id}", get(runs::get).patch(runs::patch))
        .route("/runs/{id}/metrics", post(metrics::log).get(metrics::list))
        .route("/runs/{id}/curves", post(curves::log).get(curves::list))
        .route("/runs/{id}/artifacts", post(artifacts::create).get(artifacts::list))
        .route("/runs/{id}/documents", post(documents::link_run).get(documents::list_run_documents))
        .route("/curves/compare", get(curves::compare))
        // versioned-document registry (Slice 1: Config Registry)
        .route("/documents", post(documents::create).get(documents::list))
        .route("/documents/{id}", get(documents::get))
        .route("/documents/{id}/versions", post(documents::publish))
        .route("/versions/{id}", get(documents::get_version))
        .route("/versions/{id}/runs", get(documents::version_runs))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_api_key,
        ));

    Router::new()
        .route("/health", get(health::health))
        .nest("/api/v1", api)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
