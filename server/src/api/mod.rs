//! HTTP router assembly.

pub mod experiments;
pub mod health;
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
        .route("/runs", post(runs::create))
        .route("/runs/{id}", get(runs::get).patch(runs::patch))
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
