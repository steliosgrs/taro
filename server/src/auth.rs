//! Static bearer-token auth stub. If no api_key is configured, all requests pass.

use crate::{error::AppError, state::AppState};
use axum::{extract::State, extract::Request, middleware::Next, response::Response};

pub async fn require_api_key(
    State(st): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, AppError> {
    let Some(expected) = st.api_key.as_deref() else {
        return Ok(next.run(req).await); // auth disabled
    };

    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match presented {
        Some(token) if token == expected => Ok(next.run(req).await),
        _ => Err(AppError::Unauthorized),
    }
}
