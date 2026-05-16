use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Serde helper: absent field → `None`; `null` → `Some(None)`; value → `Some(Some(v))`.
/// Apply with `#[serde(default, deserialize_with = "de_opt_nullable")]`.
fn de_opt_nullable<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Some(Option::deserialize(de)?))
}

use crate::{
    app_state::AppState,
    context::{hash_api_key, ContextConfig},
    tus::TusError,
};
use super::session::SessionUser;

fn require_admin(user: &SessionUser) -> Result<(), TusError> {
    if user.role == "admin" { Ok(()) } else { Err(TusError::Forbidden) }
}

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ContextResponse {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub storage_prefix: String,
    pub max_upload_bytes: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct ContextCreatedResponse {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub storage_prefix: String,
    pub max_upload_bytes: Option<i64>,
    pub api_key: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(ctx: &ContextRow) -> ContextResponse {
    ContextResponse {
        id: ctx.id.clone(),
        slug: ctx.slug.clone(),
        display_name: ctx.display_name.clone(),
        storage_prefix: ctx.storage_prefix.clone(),
        max_upload_bytes: ctx.max_upload_bytes,
        created_at: ctx.created_at.clone(),
        updated_at: ctx.updated_at.clone(),
    }
}

// ── DB row ────────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ContextRow {
    id: String,
    slug: String,
    display_name: String,
    api_key_hash: String,
    storage_prefix: String,
    max_upload_bytes: Option<i64>,
    created_at: String,
    updated_at: String,
}

fn row_to_config(row: &ContextRow) -> ContextConfig {
    ContextConfig {
        id: row.id.clone(),
        slug: row.slug.clone(),
        display_name: row.display_name.clone(),
        api_key_hash: row.api_key_hash.clone(),
        storage_prefix: row.storage_prefix.clone(),
        max_upload_bytes: row.max_upload_bytes,
    }
}

// ── GET /api/contexts ─────────────────────────────────────────────────────────

pub async fn list_contexts(State(state): State<AppState>) -> impl IntoResponse {
    let rows: Result<Vec<ContextRow>, _> = sqlx::query_as(
        "SELECT id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, \
         created_at, updated_at FROM contexts ORDER BY created_at ASC",
    )
    .fetch_all(&state.db_pool)
    .await;

    match rows {
        Ok(rows) => Json(rows.iter().map(to_response).collect::<Vec<_>>()).into_response(),
        Err(e) => {
            tracing::error!("list_contexts: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── POST /api/contexts ────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateContextBody {
    pub slug: String,
    pub display_name: String,
    pub storage_prefix: Option<String>,
    pub max_upload_bytes: Option<i64>,
}

pub async fn create_context(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Json(body): Json<CreateContextBody>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;

    let slug = body.slug.trim().to_lowercase();
    if slug.is_empty() || slug.contains('/') || slug.starts_with("api") {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "invalid slug"})),
        )
            .into_response());
    }

    let id = Uuid::new_v4().to_string();
    let api_key = Uuid::new_v4().to_string() + "-" + &Uuid::new_v4().to_string();
    let api_key_hash = hash_api_key(&api_key);
    let storage_prefix = body.storage_prefix.unwrap_or_else(|| slug.clone());

    let row: Result<ContextRow, _> = sqlx::query_as(
        "INSERT INTO contexts (id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING \
         id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, created_at, updated_at",
    )
    .bind(&id)
    .bind(&slug)
    .bind(body.display_name.trim())
    .bind(&api_key_hash)
    .bind(&storage_prefix)
    .bind(body.max_upload_bytes)
    .fetch_one(&state.db_pool)
    .await;

    Ok(match row {
        Ok(row) => {
            state.context_cache.insert(row_to_config(&row)).await;
            (
                StatusCode::CREATED,
                Json(ContextCreatedResponse {
                    id: row.id,
                    slug: row.slug,
                    display_name: row.display_name,
                    storage_prefix: row.storage_prefix,
                    max_upload_bytes: row.max_upload_bytes,
                    api_key,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                }),
            )
                .into_response()
        }
        Err(e) if e.to_string().contains("UNIQUE") => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "slug already in use"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("create_context: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    })
}

// ── GET /api/contexts/:id ─────────────────────────────────────────────────────

pub async fn get_context(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let row: Result<Option<ContextRow>, _> = sqlx::query_as(
        "SELECT id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, \
         created_at, updated_at FROM contexts WHERE id = ?",
    )
    .bind(&id)
    .fetch_optional(&state.db_pool)
    .await;

    match row {
        Ok(Some(r)) => Json(to_response(&r)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("get_context: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

// ── PUT /api/contexts/:id ─────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateContextBody {
    pub display_name: Option<String>,
    /// Absent → preserve existing quota.
    /// Explicit `null` → clear quota (unlimited).
    /// Number → set quota.
    #[serde(default, deserialize_with = "de_opt_nullable")]
    pub max_upload_bytes: Option<Option<i64>>,
}

pub async fn update_context(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateContextBody>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;

    // Use two SQL variants: if max_upload_bytes was present in the JSON (even as null)
    // we overwrite it directly; if absent we preserve the old value via COALESCE.
    let row: Result<Option<ContextRow>, _> = match body.max_upload_bytes {
        None => sqlx::query_as(
            "UPDATE contexts SET \
               display_name     = COALESCE(?, display_name), \
               updated_at       = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ? RETURNING \
             id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, created_at, updated_at",
        )
        .bind(body.display_name.as_deref())
        .bind(&id)
        .fetch_optional(&state.db_pool)
        .await,
        Some(quota) => sqlx::query_as(
            "UPDATE contexts SET \
               display_name     = COALESCE(?, display_name), \
               max_upload_bytes = ?, \
               updated_at       = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ? RETURNING \
             id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, created_at, updated_at",
        )
        .bind(body.display_name.as_deref())
        .bind(quota)
        .bind(&id)
        .fetch_optional(&state.db_pool)
        .await,
    };

    Ok(match row {
        Ok(Some(r)) => {
            state.context_cache.insert(row_to_config(&r)).await;
            Json(to_response(&r)).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("update_context: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    })
}

// ── DELETE /api/contexts/:id ──────────────────────────────────────────────────

pub async fn delete_context(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;

    // Fetch slug first so we can remove from cache.
    let slug: Option<String> =
        sqlx::query_scalar("SELECT slug FROM contexts WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db_pool)
            .await
            .unwrap_or(None);

    let result = sqlx::query("DELETE FROM contexts WHERE id = ?")
        .bind(&id)
        .execute(&state.db_pool)
        .await;

    Ok(match result {
        Ok(r) if r.rows_affected() > 0 => {
            if let Some(s) = slug {
                state.context_cache.remove(&s).await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("delete_context: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    })
}

// ── POST /api/contexts/:id/rotate-key ─────────────────────────────────────────

#[derive(Serialize)]
pub struct RotateKeyResponse {
    pub api_key: String,
}

pub async fn rotate_context_key(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;

    let api_key = Uuid::new_v4().to_string() + "-" + &Uuid::new_v4().to_string();
    let api_key_hash = hash_api_key(&api_key);

    let row: Result<Option<ContextRow>, _> = sqlx::query_as(
        "UPDATE contexts SET api_key_hash = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
         WHERE id = ? RETURNING \
         id, slug, display_name, api_key_hash, storage_prefix, max_upload_bytes, created_at, updated_at",
    )
    .bind(&api_key_hash)
    .bind(&id)
    .fetch_optional(&state.db_pool)
    .await;

    Ok(match row {
        Ok(Some(r)) => {
            state.context_cache.insert(row_to_config(&r)).await;
            Json(RotateKeyResponse { api_key }).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            tracing::error!("rotate_context_key: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    })
}
