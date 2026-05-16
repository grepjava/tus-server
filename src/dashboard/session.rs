use std::net::SocketAddr;

use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Response, StatusCode},
    middleware::Next,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::app_state::AppState;

pub async fn seed_admin_user(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    if count == 0 {
        let password = std::env::var("ADMIN_PASSWORD").map_err(|_| {
            anyhow::anyhow!(
                "ADMIN_PASSWORD is required on first startup (no users exist yet). \
                 Set it to a strong password in your .env or environment."
            )
        })?;
        if password.trim().is_empty() {
            anyhow::bail!("ADMIN_PASSWORD must not be empty");
        }
        let hash = tokio::task::spawn_blocking(move || {
            bcrypt::hash(&password, bcrypt::DEFAULT_COST)
        })
        .await??;
        sqlx::query(
            "INSERT INTO users (username, password_hash, role) VALUES ('admin', ?, 'admin')",
        )
        .bind(&hash)
        .execute(pool)
        .await?;
        tracing::info!("created default admin user (username: admin)");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionUser {
    pub id: String,
    pub username: String,
    pub role: String,
}

fn extract_session_token(req: &Request) -> Option<String> {
    let header = req.headers().get("cookie")?.to_str().ok()?;
    for part in header.split(';') {
        if let Some(val) = part.trim().strip_prefix("tuskar_session=") {
            let token = val.trim().to_string();
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

/// Extract the scheme+host origin from a URL string (e.g. "https://example.com:4000").
fn base_origin(base_url: &str) -> &str {
    // Strip trailing slash and path; keep scheme+host+port only.
    let without_scheme = base_url
        .find("://")
        .map(|i| &base_url[i + 3..])
        .unwrap_or(base_url);
    let host_end = without_scheme.find('/').unwrap_or(without_scheme.len());
    let scheme_end = base_url.find("://").map(|i| i + 3).unwrap_or(0);
    &base_url[..scheme_end + host_end]
}

pub async fn session_auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response<Body> {
    // CSRF: for state-changing methods, reject requests whose Origin header does not
    // match BASE_URL's origin. Absent Origin (e.g. direct API calls) is allowed.
    let method = request.method().as_str();
    if !matches!(method, "GET" | "HEAD" | "OPTIONS") {
        if let Some(origin) = request.headers().get("origin").and_then(|v| v.to_str().ok()) {
            let expected = base_origin(&state.config.base_url);
            if origin != expected {
                return (StatusCode::FORBIDDEN, Json(json!({"error": "invalid origin"}))).into_response();
            }
        }
    }

    let Some(token) = extract_session_token(&request) else {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    };

    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT u.id, u.username, u.role
         FROM sessions s
         JOIN users u ON s.user_id = u.id
         WHERE s.token = ? AND datetime(s.expires_at) > datetime('now')",
    )
    .bind(&token)
    .fetch_optional(&state.db_pool)
    .await;

    match row {
        Ok(Some((id, username, role))) => {
            request.extensions_mut().insert(SessionUser { id, username, role });
            next.run(request).await
        }
        _ => (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response(),
    }
}

// ── login / logout / me ───────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

pub async fn login_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> Response<Body> {
    let ip = crate::trusted_proxy::extract_client_ip(
        &headers,
        addr.ip(),
        &state.config.trusted_proxies,
    );

    if let Some(retry_after) = state.login_throttle.check(ip, &body.username) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "too many failed login attempts",
                "retry_after_secs": retry_after,
            })),
        )
            .into_response();
    }

    let row = sqlx::query_as::<_, (String, String, String)>(
        "SELECT id, password_hash, role FROM users WHERE username = ? COLLATE NOCASE",
    )
    .bind(&body.username)
    .fetch_optional(&state.db_pool)
    .await;

    let (user_id, hash, role) = match row {
        Ok(Some(r)) => r,
        _ => {
            // Run a dummy bcrypt verify to equalise response time whether or not
            // the username exists, preventing username enumeration via timing.
            let pw = body.password.clone();
            let dummy = state.dummy_hash.clone();
            let _ = tokio::task::spawn_blocking(move || bcrypt::verify(&pw, &*dummy)).await;
            state.login_throttle.record_failure(ip, &body.username);
            return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid credentials"}))).into_response();
        }
    };

    let password = body.password.clone();
    let valid = tokio::task::spawn_blocking(move || bcrypt::verify(&password, &hash))
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);

    if !valid {
        state.login_throttle.record_failure(ip, &body.username);
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "invalid credentials"}))).into_response();
    }

    state.login_throttle.record_success(ip, &body.username);

    let token = uuid::Uuid::new_v4().to_string();
    let result = sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at)
         VALUES (?, ?, datetime('now', '+24 hours'))",
    )
    .bind(&token)
    .bind(&user_id)
    .execute(&state.db_pool)
    .await;

    if result.is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    let secure = if state.config.cookie_secure { "; Secure" } else { "" };
    let cookie = format!(
        "tuskar_session={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400{}",
        token, secure
    );
    (
        StatusCode::OK,
        [("Set-Cookie", cookie.as_str())],
        Json(json!({"username": body.username, "role": role})),
    )
        .into_response()
}

pub async fn logout_handler(
    State(state): State<AppState>,
    request: Request,
) -> Response<Body> {
    if let Some(token) = extract_session_token(&request) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(&token)
            .execute(&state.db_pool)
            .await;
    }
    let secure = if state.config.cookie_secure { "; Secure" } else { "" };
    let clear = format!("tuskar_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}");
    (
        StatusCode::OK,
        [("Set-Cookie", clear.as_str())],
        Json(json!({"ok": true})),
    )
        .into_response()
}

pub async fn me_handler(
    axum::Extension(user): axum::Extension<SessionUser>,
) -> impl IntoResponse {
    Json(json!({
        "id":       user.id,
        "username": user.username,
        "role":     user.role,
    }))
}
