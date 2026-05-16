use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

use crate::{
    app_state::AppState,
    tus::TusError,
    webhook::{validation::validate_webhook_url, NewWebhookConfig, UpdateWebhookConfig, WebhookConfig},
};
use crate::tus::Upload;
use super::session::SessionUser;

fn require_admin(user: &SessionUser) -> Result<(), TusError> {
    if user.role == "admin" { Ok(()) } else { Err(TusError::Forbidden) }
}

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

const MAX_PAGE_LIMIT: i64 = 200;

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
    pub context_id: Option<String>,
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
            context_id: u.context_id,
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
        .list_uploads(page.limit.min(MAX_PAGE_LIMIT), page.offset)
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
    let events = state
        .upload_service
        .list_events(&id, page.limit.min(MAX_PAGE_LIMIT), page.offset)
        .await?;
    Ok(Json(events))
}

pub async fn retry_processing(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    state.upload_service.retry_processing(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn mark_abandoned(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    state.upload_service.mark_abandoned(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_upload(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    state.upload_service.hard_delete(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PurgeRequest {
    pub ids: Vec<String>,
}

pub async fn purge_uploads(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Json(req): Json<PurgeRequest>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    let deleted = state.upload_service.purge(req.ids).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db_pool)
        .await
        .is_ok();

    let storage_ok = state.upload_service.storage_health().await.is_ok();

    let status = if db_ok && storage_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status,
        Json(serde_json::json!({
            "status": if db_ok && storage_ok { "ok" } else { "degraded" },
            "db": db_ok,
            "storage": storage_ok,
        })),
    )
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
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Json(body): Json<NewWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    validate_webhook_url(&body.url).await?;
    let webhook = state.webhook_repo.create(body).await?;
    Ok((StatusCode::CREATED, Json(WebhookResponse::from(webhook))))
}

pub async fn update_webhook(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateWebhookConfig>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    validate_webhook_url(&body.url).await?;
    let webhook = state.webhook_repo.update(&id, body).await?;
    Ok(Json(WebhookResponse::from(webhook)))
}

pub async fn delete_webhook(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
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

#[derive(Serialize, FromRow)]
pub struct AuditEntry {
    pub id: String,
    pub created_at: String,
    pub request_id: Option<String>,
    pub actor: String,
    pub source_ip: Option<String>,
    pub method: String,
    pub path: String,
    pub upload_id: Option<String>,
    pub status_code: i64,
}

pub async fn list_audit(
    State(state): State<AppState>,
    Query(page): Query<Pagination>,
) -> Result<impl IntoResponse, TusError> {
    let entries = sqlx::query_as::<_, AuditEntry>(
        "SELECT id, created_at, request_id, actor, source_ip, method, path, upload_id, status_code \
         FROM audit_log ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(page.limit.min(MAX_PAGE_LIMIT))
    .bind(page.offset)
    .fetch_all(&state.db_pool)
    .await?;
    Ok(Json(entries))
}

// ── Settings ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct SettingEntry {
    pub key: String,
    pub label: String,
    pub description: String,
    pub category: String,
    pub input_type: String,
    pub value: String,
    pub source: String,
    pub restart_required: bool,
    pub options: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingRequest {
    pub value: String,
}

struct SettingDef {
    key: &'static str,
    label: &'static str,
    description: &'static str,
    category: &'static str,
    input_type: &'static str,
    options: Option<Vec<String>>,
    restart_required: bool,
    default_val: &'static str,
}

fn all_defs() -> Vec<SettingDef> {
    vec![
        // General
        SettingDef { key:"BASE_URL",           label:"Base URL",                description:"Public URL of this server, used in upload Location headers.",         category:"General",       input_type:"url",     options:None, restart_required:false, default_val:"http://localhost:3000" },
        SettingDef { key:"MAX_UPLOAD_BYTES",   label:"Max upload size",         description:"Maximum bytes allowed per upload. Default 100 GiB.",                  category:"General",       input_type:"bytes",   options:None, restart_required:false, default_val:"107374182400" },
        SettingDef { key:"UPLOAD_EXPIRY_HOURS",label:"Upload expiry",           description:"Hours after creation before an incomplete upload expires. 0 = never.", category:"General",       input_type:"number",  options:None, restart_required:false, default_val:"24" },
        SettingDef { key:"ABANDONED_AFTER_HOURS",label:"Abandon after",         description:"Hours of inactivity before the cleanup worker abandons an upload.",    category:"General",       input_type:"number",  options:None, restart_required:false, default_val:"24" },
        SettingDef { key:"CLEANUP_INTERVAL_SECS",label:"Cleanup interval",      description:"Seconds between cleanup worker runs.",                                 category:"General",       input_type:"number",  options:None, restart_required:true,  default_val:"3600" },
        SettingDef { key:"BIND_ADDR",          label:"Bind address",            description:"TCP address the server listens on.",                                   category:"General",       input_type:"text",    options:None, restart_required:true,  default_val:"0.0.0.0:3000" },
        // Quotas
        SettingDef { key:"QUOTA_MAX_STORAGE_BYTES",  label:"Storage quota",    description:"Max total bytes across all active uploads. 0 = no limit.",             category:"Quotas",        input_type:"bytes",   options:None, restart_required:false, default_val:"0" },
        SettingDef { key:"QUOTA_MAX_ACTIVE_UPLOADS", label:"Active upload quota",description:"Max number of concurrent active uploads. 0 = no limit.",             category:"Quotas",        input_type:"number",  options:None, restart_required:false, default_val:"0" },
        // Rate limiting
        SettingDef { key:"RATE_LIMIT_RPS",  label:"Rate limit (req/s)",         description:"Requests per second per client IP. 0 = disabled.",                    category:"Rate Limiting", input_type:"number",  options:None, restart_required:true,  default_val:"0" },
        SettingDef { key:"RATE_LIMIT_BURST",label:"Rate limit burst",           description:"Burst allowance above the steady rate.",                               category:"Rate Limiting", input_type:"number",  options:None, restart_required:true,  default_val:"0" },
        // Security
        SettingDef { key:"API_KEY",         label:"API key",                    description:"Bearer token required on all requests. Empty = auth disabled.",        category:"Security",      input_type:"password",options:None, restart_required:true,  default_val:"" },
        // Storage
        SettingDef { key:"STORAGE_BACKEND", label:"Storage backend",            description:"Where to store uploaded files.",                                       category:"Storage",       input_type:"select",  options:Some(vec!["filesystem".into(),"s3".into()]), restart_required:true, default_val:"filesystem" },
        SettingDef { key:"STORAGE_DIR",     label:"Storage directory",          description:"Local directory for filesystem storage and S3 staging.",               category:"Storage",       input_type:"text",    options:None, restart_required:true,  default_val:"uploads" },
        SettingDef { key:"S3_BUCKET",       label:"S3 bucket",                  description:"Bucket name. Required when STORAGE_BACKEND=s3.",                       category:"Storage",       input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"S3_PREFIX",       label:"S3 key prefix",              description:"Key prefix for all objects written to S3.",                            category:"Storage",       input_type:"text",    options:None, restart_required:true,  default_val:"uploads/" },
        SettingDef { key:"S3_FORCE_PATH_STYLE",label:"S3 path-style URLs",      description:"Required for MinIO and LocalStack.",                                   category:"Storage",       input_type:"boolean", options:None, restart_required:true,  default_val:"false" },
        SettingDef { key:"S3_MULTIPART_THRESHOLD",label:"S3 multipart threshold",description:"Files larger than this use multipart upload. Default 8 MiB.",        category:"Storage",       input_type:"bytes",   options:None, restart_required:true,  default_val:"8388608" },
        SettingDef { key:"S3_PART_SIZE",    label:"S3 part size",               description:"Part size for multipart uploads. Min 5 MiB.",                          category:"Storage",       input_type:"bytes",   options:None, restart_required:true,  default_val:"8388608" },
        // Processing
        SettingDef { key:"PROCESSORS",              label:"Processors",             description:"Comma-separated pipeline: nop, exec, mime, av.",                   category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"nop" },
        SettingDef { key:"PROCESSOR_EXEC_COMMAND",  label:"Exec command",           description:"Shell command run by the exec processor.",                         category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"PROCESSOR_EXEC_TIMEOUT_SECS",label:"Exec timeout (s)",   description:"Seconds before the exec processor is killed.",                     category:"Processing",    input_type:"number",  options:None, restart_required:true,  default_val:"300" },
        SettingDef { key:"MIME_ALLOW",  label:"MIME allow-list",                    description:"Comma-separated MIME types to allow. Others are rejected.",         category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"MIME_DENY",   label:"MIME deny-list",                     description:"Comma-separated MIME types to reject.",                             category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"EXT_ALLOW",   label:"Extension allow-list",               description:"Comma-separated extensions to allow (e.g. jpg,png,pdf).",           category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"EXT_DENY",    label:"Extension deny-list",                description:"Comma-separated extensions to reject.",                             category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"AV_SCANNER",  label:"AV scanner",                         description:"Antivirus backend: clamav or http.",                               category:"Processing",    input_type:"select",  options:Some(vec!["clamav".into(),"http".into()]), restart_required:true, default_val:"clamav" },
        SettingDef { key:"AV_TIMEOUT_SECS",label:"AV timeout (s)",                  description:"Seconds before the AV scanner is killed.",                         category:"Processing",    input_type:"number",  options:None, restart_required:true,  default_val:"120" },
        SettingDef { key:"AV_CLAMAV_BIN",  label:"clamscan binary",                 description:"Path to the clamscan executable.",                                 category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"clamscan" },
        SettingDef { key:"AV_HTTP_URL",    label:"AV HTTP URL",                     description:"URL to POST files to for scanning.",                               category:"Processing",    input_type:"url",     options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"AV_HTTP_HEADER", label:"AV HTTP header",                  description:"Extra header for the AV HTTP request (Name: value).",              category:"Processing",    input_type:"text",    options:None, restart_required:true,  default_val:"" },
        SettingDef { key:"AV_HTTP_MAX_BYTES",label:"AV HTTP max bytes",             description:"Files larger than this are rejected by the HTTP scanner.",         category:"Processing",    input_type:"bytes",   options:None, restart_required:true,  default_val:"104857600" },
        // Webhooks
        SettingDef { key:"WEBHOOK_MAX_ATTEMPTS",          label:"Max webhook attempts",   description:"Max delivery attempts before giving up.",                   category:"Webhooks",      input_type:"number",  options:None, restart_required:false, default_val:"3" },
        SettingDef { key:"WEBHOOK_RETRY_DELAYS",          label:"Webhook retry delays",   description:"Comma-separated seconds between retry attempts.",            category:"Webhooks",      input_type:"text",    options:None, restart_required:false, default_val:"1,4" },
        SettingDef { key:"WEBHOOK_DELIVERY_RETENTION_DAYS",label:"Webhook log retention", description:"Days to keep webhook delivery records. 0 = forever.",       category:"Webhooks",      input_type:"number",  options:None, restart_required:false, default_val:"30" },
        // Retention
        SettingDef { key:"AUDIT_LOG_RETENTION_DAYS",  label:"Audit log retention",  description:"Days to keep audit log entries. 0 = forever.",                    category:"Retention",     input_type:"number",  options:None, restart_required:false, default_val:"90" },
        // Observability
        SettingDef { key:"GRAFANA_URL",label:"Grafana dashboard URL",               description:"Link to your Grafana dashboard. Shown on the Metrics page.",       category:"Observability", input_type:"url",     options:None, restart_required:false, default_val:"" },
        SettingDef { key:"RUST_LOG",   label:"Log level",                           description:"Tracing filter (error, warn, info, debug, trace).",                 category:"Observability", input_type:"text",    options:None, restart_required:true,  default_val:"info" },
    ]
}

fn resolve_setting(key: &str, db: &HashMap<String, String>, default_val: &str) -> (String, String) {
    if let Some(v) = db.get(key) {
        return (v.clone(), "db".into());
    }
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => (v, "env".into()),
        _ => (default_val.to_string(), "default".into()),
    }
}

fn build_setting_entry(def: &SettingDef, db: &HashMap<String, String>) -> SettingEntry {
    let (mut value, source) = resolve_setting(def.key, db, def.default_val);
    if def.input_type == "password" && source != "default" && !value.is_empty() {
        value = "••••••••".to_string();
    }
    SettingEntry {
        key: def.key.to_string(),
        label: def.label.to_string(),
        description: def.description.to_string(),
        category: def.category.to_string(),
        input_type: def.input_type.to_string(),
        value,
        source,
        restart_required: def.restart_required,
        options: def.options.clone(),
    }
}

pub async fn list_settings(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    let db: HashMap<String, String> =
        sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
            .fetch_all(&state.db_pool)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();

    let entries: Vec<SettingEntry> = all_defs()
        .iter()
        .map(|d| build_setting_entry(d, &db))
        .collect();
    Ok(Json(entries))
}

pub async fn update_setting(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<UpdateSettingRequest>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    let defs = all_defs();
    let def = defs.iter().find(|d| d.key == key.as_str())
        .ok_or(TusError::NotFound)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?,?,?) \
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
    )
    .bind(&key)
    .bind(&body.value)
    .bind(&now)
    .execute(&state.db_pool)
    .await?;

    let db: HashMap<String, String> = std::iter::once((key.clone(), body.value)).collect();
    Ok(Json(build_setting_entry(def, &db)))
}

pub async fn delete_setting(
    Extension(user): Extension<SessionUser>,
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, TusError> {
    require_admin(&user)?;
    let defs = all_defs();
    let def = defs.iter().find(|d| d.key == key.as_str())
        .ok_or(TusError::NotFound)?;
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(&key)
        .execute(&state.db_pool)
        .await?;
    Ok(Json(build_setting_entry(def, &HashMap::new())))
}
