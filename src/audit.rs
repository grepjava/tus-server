use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

use crate::app_state::AppState;

/// Resolve an audit actor string from the request headers.
/// Priority: API key bearer → session cookie username → "anonymous".
async fn resolve_actor(headers: &HeaderMap, pool: &sqlx::SqlitePool) -> String {
    if headers.contains_key("authorization") {
        return "api_key".to_string();
    }
    // Resolve session cookie to a username for precise attribution.
    let actor = async {
        let cookie = headers.get("cookie")?.to_str().ok()?;
        let token = cookie
            .split(';')
            .find_map(|p| p.trim().strip_prefix("tuskar_session=").map(str::to_string))?;
        let (username,): (String,) = sqlx::query_as(
            "SELECT u.username FROM sessions s \
             JOIN users u ON s.user_id = u.id \
             WHERE s.token = ? AND datetime(s.expires_at) > datetime('now')",
        )
        .bind(&token)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()?;
        Some(username)
    }
    .await;
    actor.unwrap_or_else(|| "anonymous".to_string())
}

pub async fn audit_middleware(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let source_ip = crate::trusted_proxy::extract_client_ip(
        req.headers(),
        addr.ip(),
        &state.config.trusted_proxies,
    )
    .to_string();

    let actor = resolve_actor(req.headers(), &state.db_pool).await;

    let upload_id = extract_upload_id(&path);

    let response = next.run(req).await;
    let status_code = response.status().as_u16() as i64;

    let pool = state.db_pool.clone();
    tokio::spawn(async move {
        let id = Uuid::new_v4().to_string();
        let _ = sqlx::query(
            "INSERT INTO audit_log \
             (id, request_id, actor, source_ip, method, path, upload_id, status_code) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&request_id)
        .bind(&actor)
        .bind(&source_ip)
        .bind(&method)
        .bind(&path)
        .bind(&upload_id)
        .bind(status_code)
        .execute(&pool)
        .await;
    });

    response
}

fn extract_upload_id(path: &str) -> Option<String> {
    for prefix in ["/files/", "/api/uploads/"] {
        if let Some(rest) = path.strip_prefix(prefix) {
            let id = rest.split('/').next()?;
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}
