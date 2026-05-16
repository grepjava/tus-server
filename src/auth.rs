use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::app_state::AppState;


pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let Some(ref expected_key) = state.config.api_key else {
        return next.run(request).await;
    };

    // The global API key guards the legacy TUS endpoint (/files) and the metrics
    // endpoint (/metrics). Dashboard APIs use session auth. Context TUS paths use
    // per-context keys.
    let path = request.uri().path();
    if !path.starts_with("/files") && path != "/metrics" {
        return next.run(request).await;
    }

    let provided = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let authed = match provided {
        Some(token) => {
            let expected_hash = Sha256::digest(expected_key.as_bytes());
            let provided_hash = Sha256::digest(token.as_bytes());
            bool::from(expected_hash.as_slice().ct_eq(provided_hash.as_slice()))
        }
        None => false,
    };

    if authed {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Bearer")],
            "Unauthorized",
        )
            .into_response()
    }
}
