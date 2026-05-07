use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
};

use crate::app_state::AppState;

pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response<Body> {
    let Some(ref expected_key) = state.config.api_key else {
        return next.run(request).await;
    };

    // Health endpoint is always public
    if request.uri().path() == "/api/health" {
        return next.run(request).await;
    }

    let provided = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match provided {
        Some(token) if token == expected_key => next.run(request).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Bearer")],
            "Unauthorized",
        )
            .into_response(),
    }
}
