use std::{borrow::Cow, sync::Arc};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use openidconnect::{
    core::{
        CoreAuthenticationFlow, CoreClient, CoreGenderClaim, CoreIdToken, CoreIdTokenClaims,
        CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreProviderMetadata,
        CoreTokenType,
    },
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EmptyAdditionalClaims,
    EmptyExtraTokenFields, IdTokenFields, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, Scope, StandardTokenResponse,
};
use serde::Deserialize;
use serde_json::json;

use crate::app_state::AppState;

/// Type returned by the token endpoint for a Core OIDC client.
type CoreTokenResponse = StandardTokenResponse<
    IdTokenFields<
        EmptyAdditionalClaims,
        EmptyExtraTokenFields,
        CoreGenderClaim,
        CoreJweContentEncryptionAlgorithm,
        CoreJwsSigningAlgorithm,
    >,
    CoreTokenType,
>;

// ── Config / startup ─────────────────────────────────────────────────────────

pub struct OidcConfig {
    /// Provider metadata fetched once at startup (contains auth URL, token URL, JWKS, …).
    /// The `CoreClient` is reconstructed per-request so we avoid openidconnect 4.x
    /// typestate issues: `CoreClient` has all endpoints `EndpointNotSet`, while
    /// `from_provider_metadata` returns a type with auth/token endpoints set.
    /// The methods `authorize_url` and `exchange_code` are only available on that
    /// configured type, so we create the client locally inside each handler.
    pub provider_metadata: CoreProviderMetadata,
    pub client_id: ClientId,
    pub client_secret: Option<ClientSecret>,
    pub redirect_url: RedirectUrl,
    pub http_client: reqwest::Client,
    pub default_role: String,
}

pub async fn build_oidc_config(
    config: &crate::config::Config,
) -> anyhow::Result<Option<Arc<OidcConfig>>> {
    let issuer = match &config.oidc_issuer_url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => return Ok(None),
    };

    let client_id = config
        .oidc_client_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("OIDC_CLIENT_ID is required when OIDC_ISSUER_URL is set"))?
        .to_string();

    let redirect_uri = config
        .oidc_redirect_uri
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("OIDC_REDIRECT_URI is required when OIDC_ISSUER_URL is set")
        })?
        .to_string();

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let issuer_url =
        IssuerUrl::new(issuer).map_err(|e| anyhow::anyhow!("invalid OIDC_ISSUER_URL: {e}"))?;

    let provider_metadata = CoreProviderMetadata::discover_async(issuer_url, &http_client)
        .await
        .map_err(|e| anyhow::anyhow!("OIDC discovery failed: {e}"))?;

    let client_secret = config
        .oidc_client_secret
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| ClientSecret::new(s.to_string()));

    let redirect_url = RedirectUrl::new(redirect_uri)
        .map_err(|e| anyhow::anyhow!("invalid OIDC_REDIRECT_URI: {e}"))?;

    tracing::info!("OIDC configured");

    Ok(Some(Arc::new(OidcConfig {
        provider_metadata,
        client_id: ClientId::new(client_id),
        client_secret,
        redirect_url,
        http_client,
        default_role: config.oidc_default_role.clone(),
    })))
}

// ── GET /api/auth/config ──────────────────────────────────────────────────────

pub async fn auth_config_handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({ "oidc": state.oidc_config.is_some() }))
}

// ── GET /api/auth/oidc/login ──────────────────────────────────────────────────

pub async fn oidc_login_handler(State(state): State<AppState>) -> Response {
    let Some(oidc) = &state.oidc_config else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "OIDC not configured"})),
        )
            .into_response();
    };

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let client = CoreClient::from_provider_metadata(
        oidc.provider_metadata.clone(),
        oidc.client_id.clone(),
        oidc.client_secret.clone(),
    );

    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("openid".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .set_redirect_uri(Cow::Borrowed(&oidc.redirect_url))
        .url();

    // Pack flow state into one HttpOnly cookie:  csrf_state.nonce.pkce_verifier
    // All three values are URL-safe base64 (no dots), so '.' is a safe separator.
    let cookie_val = format!(
        "{}.{}.{}",
        csrf_token.secret(),
        nonce.secret(),
        pkce_verifier.secret(),
    );
    let cookie = format!(
        "oidc_flow={cookie_val}; Path=/api/auth/oidc; HttpOnly; SameSite=Lax; Max-Age=300"
    );

    axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", auth_url.to_string())
        .header("Set-Cookie", cookie)
        .body(axum::body::Body::empty())
        .unwrap()
}

// ── GET /api/auth/oidc/callback ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub async fn oidc_callback_handler(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Some(err) = &params.error {
        tracing::warn!("OIDC callback: IDP returned error={err}");
        return redirect_error();
    }

    let Some(oidc) = &state.oidc_config else {
        return redirect_error();
    };

    // Read oidc_flow cookie
    let flow = match extract_cookie(&headers, "oidc_flow") {
        Some(v) => v,
        None => {
            tracing::warn!("OIDC callback: missing oidc_flow cookie");
            return redirect_error();
        }
    };

    let parts: Vec<&str> = flow.splitn(3, '.').collect();
    if parts.len() != 3 {
        tracing::warn!("OIDC callback: malformed oidc_flow cookie");
        return redirect_error();
    }
    let (cookie_state, cookie_nonce, cookie_verifier) = (parts[0], parts[1], parts[2]);

    // Verify CSRF state
    let query_state = match &params.state {
        Some(s) => s.as_str(),
        None => return redirect_error(),
    };
    if query_state != cookie_state {
        tracing::warn!("OIDC callback: CSRF state mismatch");
        return redirect_error();
    }

    let code = match &params.code {
        Some(c) => c.clone(),
        None => return redirect_error(),
    };

    let client = CoreClient::from_provider_metadata(
        oidc.provider_metadata.clone(),
        oidc.client_id.clone(),
        oidc.client_secret.clone(),
    );

    // exchange_code returns Result<CodeTokenRequest, ConfigurationError> in oauth2 5.0.
    let code_request = match client.exchange_code(AuthorizationCode::new(code)) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("OIDC: exchange_code setup failed: {e}");
            return redirect_error();
        }
    };

    let token_response: CoreTokenResponse = match code_request
        .set_pkce_verifier(PkceCodeVerifier::new(cookie_verifier.to_string()))
        .set_redirect_uri(Cow::Borrowed(&oidc.redirect_url))
        .request_async(&oidc.http_client)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("OIDC token exchange failed: {e}");
            return redirect_error();
        }
    };

    // Verify ID token and extract claims
    let id_token: &CoreIdToken = match token_response.extra_fields().id_token() {
        Some(t) => t,
        None => {
            tracing::error!("OIDC: no id_token in token response");
            return redirect_error();
        }
    };

    let nonce = Nonce::new(cookie_nonce.to_string());
    let verifier = client.id_token_verifier();
    let claims: &CoreIdTokenClaims = match id_token.claims(&verifier, &nonce) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("OIDC: ID token verification failed: {e}");
            return redirect_error();
        }
    };

    let sub = claims.subject().to_string();
    let email = claims.email().map(|e| e.to_string());
    let preferred_username = claims.preferred_username().map(|u| u.to_string());

    // Find or provision the Tuskar user
    let (user_id, _role) = match find_or_provision(
        &state.db_pool,
        &sub,
        email.as_deref(),
        preferred_username.as_deref(),
        &oidc.default_role,
    )
    .await
    {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("OIDC user provisioning failed: {e}");
            return redirect_error();
        }
    };

    // Create session
    let token = uuid::Uuid::new_v4().to_string();
    if let Err(e) = sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at) \
         VALUES (?, ?, datetime('now', '+24 hours'))",
    )
    .bind(&token)
    .bind(&user_id)
    .execute(&state.db_pool)
    .await
    {
        tracing::error!("OIDC: session insert failed: {e}");
        return redirect_error();
    }

    tracing::info!(user_id, "OIDC login success");

    let secure = if state.config.cookie_secure { "; Secure" } else { "" };
    let session_cookie = format!(
        "tuskar_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400{secure}"
    );
    let clear_flow =
        "oidc_flow=; Path=/api/auth/oidc; HttpOnly; SameSite=Lax; Max-Age=0".to_string();

    axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", "/")
        .header("Set-Cookie", session_cookie)
        .header("Set-Cookie", clear_flow)
        .body(axum::body::Body::empty())
        .unwrap()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn redirect_error() -> Response {
    axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header("Location", "/login?sso_error=1")
        .body(axum::body::Body::empty())
        .unwrap()
}

fn extract_cookie(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    let header = headers.get("cookie")?.to_str().ok()?;
    let prefix = format!("{name}=");
    for part in header.split(';') {
        if let Some(val) = part.trim().strip_prefix(&prefix) {
            let val = val.trim().to_string();
            if !val.is_empty() {
                return Some(val);
            }
        }
    }
    None
}

async fn find_or_provision(
    pool: &sqlx::SqlitePool,
    sub: &str,
    email: Option<&str>,
    preferred_username: Option<&str>,
    default_role: &str,
) -> anyhow::Result<(String, String)> {
    // 1. Look up by oidc_sub
    if let Some((id, role)) = sqlx::query_as::<_, (String, String)>(
        "SELECT id, role FROM users WHERE oidc_sub = ?",
    )
    .bind(sub)
    .fetch_optional(pool)
    .await?
    {
        return Ok((id, role));
    }

    // 2. Link an existing user that shares the same verified email
    if let Some(email) = email {
        if let Some((id, role)) = sqlx::query_as::<_, (String, String)>(
            "SELECT id, role FROM users WHERE email = ? AND oidc_sub IS NULL",
        )
        .bind(email)
        .fetch_optional(pool)
        .await?
        {
            sqlx::query(
                "UPDATE users SET oidc_sub = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?",
            )
            .bind(sub)
            .bind(&id)
            .execute(pool)
            .await?;
            return Ok((id, role));
        }
    }

    // 3. Provision a new user
    let base = preferred_username
        .or_else(|| email.and_then(|e| e.split('@').next()))
        .unwrap_or("sso_user");
    let base: String = base
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .take(32)
        .collect();
    let base = if base.is_empty() {
        "sso_user".to_string()
    } else {
        base
    };

    let username = unique_username(pool, &base).await?;
    let id = uuid::Uuid::new_v4().to_string();

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, role, email, oidc_sub) \
         VALUES (?, ?, '*', ?, ?, ?)",
    )
    .bind(&id)
    .bind(&username)
    .bind(default_role)
    .bind(email)
    .bind(sub)
    .execute(pool)
    .await?;

    tracing::info!(username, role = default_role, "provisioned new OIDC user");
    Ok((id, default_role.to_string()))
}

async fn unique_username(pool: &sqlx::SqlitePool, base: &str) -> anyhow::Result<String> {
    let (exists,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM users WHERE username = ? COLLATE NOCASE")
            .bind(base)
            .fetch_one(pool)
            .await?;
    if exists == 0 {
        return Ok(base.to_string());
    }
    for n in 2i32..=99 {
        let candidate = format!("{base}{n}");
        let (exists,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM users WHERE username = ? COLLATE NOCASE")
                .bind(&candidate)
                .fetch_one(pool)
                .await?;
        if exists == 0 {
            return Ok(candidate);
        }
    }
    let suffix = uuid::Uuid::new_v4()
        .to_string()
        .split('-')
        .next()
        .unwrap_or("x")
        .to_string();
    Ok(format!("{base}_{suffix}"))
}
