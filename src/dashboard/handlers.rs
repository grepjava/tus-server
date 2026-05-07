use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    app_state::AppState,
    tus::TusError,
    webhook::{validation::validate_webhook_url, NewWebhookConfig, UpdateWebhookConfig, WebhookConfig},
};
use crate::tus::Upload;

#[derive(Deserialize)]
pub struct Pagination {
    #[serde(default = "default_upload_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_upload_limit() -> i64 {
    50
}

#[derive(Deserialize)]
pub struct EventPagination {
    #[serde(default = "default_event_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_event_limit() -> i64 {
    200
}

#[derive(Serialize)]
pub struct UploadResponse {
    pub id: String,
    pub filename: Option<String>,
    pub upload_length: i64,
    pub upload_offset: i64,
    pub metadata_json: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub length_is_deferred: bool,
    pub concat_type: Option<String>,
    pub concat_uploads: Option<String>,
}

impl From<Upload> for UploadResponse {
    fn from(u: Upload) -> Self {
        Self {
            id: u.id,
            filename: u.filename,
            upload_length: u.upload_length,
            upload_offset: u.upload_offset,
            metadata_json: u.metadata_json,
            status: u.status.to_string(),
            created_at: u.created_at,
            updated_at: u.updated_at,
            completed_at: u.completed_at,
            error_message: u.error_message,
            length_is_deferred: u.length_is_deferred,
            concat_type: u.concat_type,
            concat_uploads: u.concat_uploads,
        }
    }
}

#[derive(Serialize)]
pub struct WebhookResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    pub has_secret: bool,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<WebhookConfig> for WebhookResponse {
    fn from(w: WebhookConfig) -> Self {
        Self {
            id: w.id,
            name: w.name,
            url: w.url,
            has_secret: w.secret.is_some(),
            events: w.events,
            enabled: w.enabled,
            created_at: w.created_at,
            updated_at: w.updated_at,
        }
    }
}

pub async fn list_uploads(
    State(state): State<AppState>,
    Query(page): Query<Pagination>,
) -> Result<impl IntoResponse, TusError> {
    let uploads: Vec<UploadResponse> = state
        .upload_service
        .list_uploads(page.limit, page.offset)
        .await?
        .into_iter()
        .map(UploadResponse::from)
        .collect();
    Ok(Json(uploads))
}

pub async fn get_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    let upload = state.upload_service.get_upload(&id).await?;
    Ok(Json(UploadResponse::from(upload)))
}

pub async fn get_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(page): Query<EventPagination>,
) -> Result<impl IntoResponse, TusError> {
    let events = state.upload_service.list_events(&id, page.limit, page.offset).await?;
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
    let webhooks: Vec<WebhookResponse> = state
        .webhook_repo
        .list()
        .await?
        .into_iter()
        .map(WebhookResponse::from)
        .collect();
    Ok(Json(webhooks))
}

pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<NewWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    validate_webhook_url(&body.url).await?;
    let webhook = state.webhook_repo.create(body).await?;
    Ok((StatusCode::CREATED, Json(WebhookResponse::from(webhook))))
}

pub async fn update_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    validate_webhook_url(&body.url).await?;
    let webhook = state.webhook_repo.update(&id, body).await?;
    Ok(Json(WebhookResponse::from(webhook)))
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
