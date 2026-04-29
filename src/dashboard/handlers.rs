use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    app_state::AppState,
    tus::TusError,
    webhook::{NewWebhookConfig, UpdateWebhookConfig},
};

pub async fn list_uploads(State(state): State<AppState>) -> Result<impl IntoResponse, TusError> {
    let uploads = state.upload_service.list_uploads().await?;
    Ok(Json(uploads))
}

pub async fn get_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    let upload = state.upload_service.get_upload(&id).await?;
    Ok(Json(upload))
}

pub async fn get_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    let events = state.upload_service.list_events(&id).await?;
    Ok(Json(events))
}

pub async fn retry_processing(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    state.upload_service.retry_processing(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn mark_abandoned(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    state.upload_service.mark_abandoned(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    state.upload_service.hard_delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PurgeRequest {
    pub ids: Vec<String>,
}

pub async fn purge_uploads(
    State(state): State<AppState>,
    Json(req): Json<PurgeRequest>,
) -> Result<impl IntoResponse, TusError> {
    let deleted = state.upload_service.purge(req.ids).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

// Webhook handlers

pub async fn list_webhooks(State(state): State<AppState>) -> Result<impl IntoResponse, TusError> {
    let webhooks = state.webhook_repo.list().await?;
    Ok(Json(webhooks))
}

pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<NewWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    let webhook = state.webhook_repo.create(body).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

pub async fn update_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    let webhook = state.webhook_repo.update(&id, body).await?;
    Ok(Json(webhook))
}

pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    state.webhook_repo.delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    let deliveries = state.webhook_repo.list_deliveries(&id).await?;
    Ok(Json(deliveries))
}
