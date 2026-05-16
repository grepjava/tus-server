use std::{
    collections::HashMap,
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::app_state::AppState;

#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub api_key_hash: String,
    pub storage_prefix: String,
    pub max_upload_bytes: Option<i64>,
}

/// In-memory cache of active contexts, keyed by slug (lower-cased).
#[derive(Clone)]
pub struct ContextCache(Arc<RwLock<HashMap<String, Arc<ContextConfig>>>>);

impl ContextCache {
    pub fn new() -> Self {
        ContextCache(Arc::new(RwLock::new(HashMap::new())))
    }

    pub async fn load_all(&self, pool: &SqlitePool) -> anyhow::Result<()> {
        let rows: Vec<(String, String, String, String, String, Option<i64>)> =
            sqlx::query_as(
                "SELECT id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes \
                 FROM contexts",
            )
            .fetch_all(pool)
            .await?;

        let mut map = self.0.write().await;
        map.clear();
        for (id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes) in rows {
            let cfg = Arc::new(ContextConfig {
                id,
                slug: slug.clone(),
                display_name,
                api_key_hash,
                storage_prefix,
                max_upload_bytes,
            });
            map.insert(slug.to_lowercase(), cfg);
        }
        Ok(())
    }

    pub async fn get(&self, slug: &str) -> Option<Arc<ContextConfig>> {
        self.0.read().await.get(&slug.to_lowercase()).cloned()
    }

    pub async fn insert(&self, cfg: ContextConfig) {
        let slug = cfg.slug.to_lowercase();
        self.0.write().await.insert(slug, Arc::new(cfg));
    }

    pub async fn remove(&self, slug: &str) {
        self.0.write().await.remove(&slug.to_lowercase());
    }

    pub async fn all(&self) -> Vec<Arc<ContextConfig>> {
        self.0.read().await.values().cloned().collect()
    }

    /// Reverse lookup: find the slug for a context UUID. O(n) — contexts are few.
    pub async fn slug_for_id(&self, id: &str) -> Option<String> {
        self.0.read().await.values()
            .find(|c| c.id == id)
            .map(|c| c.slug.clone())
    }
}

/// Axum extension injected by `context_auth_middleware`.
#[derive(Clone)]
pub struct RequestContext(pub Arc<ContextConfig>);

/// Middleware: extract `{context}` path param, look up context, validate API key.
/// Must be applied only to routes that include `/{context}/` in their path.
pub async fn context_auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // URI path looks like "/<slug>/files" or "/<slug>/files/<id>".
    let path = req.uri().path();
    let slug = path
        .trim_start_matches('/')
        .split('/')
        .next()
        .map(|s| s.to_string());

    let slug = match slug {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (StatusCode::NOT_FOUND, "context not found").into_response();
        }
    };

    let ctx = match state.context_cache.get(&slug).await {
        Some(c) => c,
        None => {
            // Try refreshing from DB in case it was just created.
            if let Err(e) = state.context_cache.load_all(&state.db_pool).await {
                tracing::error!("context cache refresh failed: {e}");
            }
            match state.context_cache.get(&slug).await {
                Some(c) => c,
                None => {
                    return (StatusCode::NOT_FOUND, "context not found").into_response();
                }
            }
        }
    };

    // Validate API key: Authorization: Bearer <key>
    let provided_key = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.trim().to_string());

    let key = match provided_key {
        Some(k) => k,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                [("WWW-Authenticate", "Bearer")],
                "missing API key",
            )
                .into_response();
        }
    };

    if !verify_api_key(&key, &ctx.api_key_hash) {
        return (
            StatusCode::UNAUTHORIZED,
            [("WWW-Authenticate", "Bearer")],
            "invalid API key",
        )
            .into_response();
    }

    req.extensions_mut().insert(RequestContext(ctx));
    next.run(req).await
}

/// Hash an API key for storage (SHA-256 hex).
pub fn hash_api_key(key: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(key.as_bytes());
    format!("{:x}", h.finalize())
}

fn verify_api_key(provided: &str, stored_hash: &str) -> bool {
    hash_api_key(provided) == stored_hash
}
