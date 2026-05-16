use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};

use crate::{app_state::AppState, tus::TusError};
use super::session::SessionUser;

#[derive(Serialize, sqlx::FromRow)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: String,
}

fn require_admin(user: &SessionUser) -> Result<(), TusError> {
    if user.role == "admin" { Ok(()) } else { Err(TusError::Forbidden) }
}

pub async fn list_users(
    Extension(current): Extension<SessionUser>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&current)?;
    let users = sqlx::query_as::<_, UserRow>(
        "SELECT id, username, role, created_at FROM users ORDER BY created_at ASC",
    )
    .fetch_all(&state.db_pool)
    .await?;
    Ok(Json(users))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub role: Option<String>,
}

pub async fn create_user(
    Extension(current): Extension<SessionUser>,
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&current)?;

    let username = body.username.trim().to_string();
    if username.is_empty() {
        return Err(TusError::InvalidHeader("username is required".into()));
    }
    if body.password.len() < 6 {
        return Err(TusError::InvalidHeader("password must be at least 6 characters".into()));
    }
    let role = body.role.as_deref().unwrap_or("viewer").to_string();
    if role != "admin" && role != "viewer" {
        return Err(TusError::InvalidHeader("role must be admin or viewer".into()));
    }

    let password = body.password.clone();
    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(password, bcrypt::DEFAULT_COST))
        .await
        .map_err(|e| TusError::Internal(e.into()))?
        .map_err(|e| TusError::Internal(anyhow::anyhow!("bcrypt: {e}")))?;

    let user = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (username, password_hash, role)
         VALUES (?, ?, ?)
         RETURNING id, username, role, created_at",
    )
    .bind(&username)
    .bind(&hash)
    .bind(&role)
    .fetch_one(&state.db_pool)
    .await?;

    Ok((axum::http::StatusCode::CREATED, Json(user)))
}

pub async fn delete_user(
    Extension(current): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&current)?;
    if current.id == id {
        return Err(TusError::InvalidHeader("cannot delete your own account".into()));
    }
    let result = sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&id)
        .execute(&state.db_pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(TusError::NotFound);
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    pub current_password: Option<String>,
    pub new_password: String,
}

pub async fn change_password(
    Extension(current): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ChangePasswordRequest>,
) -> Result<impl IntoResponse, TusError> {
    let is_self = current.id == id;
    if !is_self && current.role != "admin" {
        return Err(TusError::Forbidden);
    }
    if body.new_password.len() < 6 {
        return Err(TusError::InvalidHeader("password must be at least 6 characters".into()));
    }

    if is_self {
        // Require current password when changing own password
        let current_pw = body.current_password.clone().unwrap_or_default();
        if current_pw.is_empty() {
            return Err(TusError::InvalidHeader("current_password is required".into()));
        }
        let row = sqlx::query_as::<_, (String,)>("SELECT password_hash FROM users WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db_pool)
            .await?
            .ok_or(TusError::NotFound)?;
        let valid = tokio::task::spawn_blocking(move || bcrypt::verify(&current_pw, &row.0))
            .await
            .map_err(|e| TusError::Internal(e.into()))?
            .map_err(|e| TusError::Internal(anyhow::anyhow!("bcrypt: {e}")))?;
        if !valid {
            return Err(TusError::Unauthorized);
        }
    }

    let new_pw = body.new_password.clone();
    let hash = tokio::task::spawn_blocking(move || bcrypt::hash(new_pw, bcrypt::DEFAULT_COST))
        .await
        .map_err(|e| TusError::Internal(e.into()))?
        .map_err(|e| TusError::Internal(anyhow::anyhow!("bcrypt: {e}")))?;

    sqlx::query("UPDATE users SET password_hash = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(&hash)
        .bind(&id)
        .execute(&state.db_pool)
        .await?;

    Ok(axum::http::StatusCode::NO_CONTENT)
}
