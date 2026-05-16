use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{
    config::Config,
    context::ContextCache,
    dashboard::oidc::OidcConfig,
    login_throttle::LoginThrottle,
    manager::ProcessorPipeline,
    metrics::AppMetrics,
    rate_limit::IpRateLimiter,
    tus::{SqliteUploadRepository, StorageBackend, UploadEvent, UploadService},
    webhook::{SqliteWebhookRepository, WebhookRepository},
};

#[derive(Clone)]
pub struct AppState {
    pub upload_service: Arc<UploadService>,
    pub webhook_repo: Arc<dyn WebhookRepository>,
    pub config: Arc<Config>,
    pub event_tx: broadcast::Sender<UploadEvent>,
    pub db_pool: sqlx::SqlitePool,
    pub metrics: Arc<AppMetrics>,
    pub metrics_registry: Arc<prometheus_client::registry::Registry>,
    pub pipeline: Arc<ProcessorPipeline>,
    pub rate_limiter: Option<Arc<IpRateLimiter>>,
    pub login_throttle: Arc<LoginThrottle>,
    /// Pre-computed bcrypt hash used for constant-time dummy verification when a
    /// login attempt names a username that does not exist.
    pub dummy_hash: Arc<str>,
    /// OIDC client configuration; `None` when OIDC is disabled.
    pub oidc_config: Option<Arc<OidcConfig>>,
    /// In-memory context cache for named upload namespaces.
    pub context_cache: ContextCache,
}

impl AppState {
    pub fn new(
        pool: sqlx::SqlitePool,
        config: Config,
        event_tx: broadcast::Sender<UploadEvent>,
        metrics: Arc<AppMetrics>,
        metrics_registry: Arc<prometheus_client::registry::Registry>,
        pipeline: Arc<ProcessorPipeline>,
        rate_limiter: Option<Arc<IpRateLimiter>>,
        storage: Arc<dyn StorageBackend>,
        login_throttle: Arc<LoginThrottle>,
        dummy_hash: Arc<str>,
        oidc_config: Option<Arc<OidcConfig>>,
        context_cache: ContextCache,
    ) -> Self {
        let repo = Arc::new(SqliteUploadRepository::new(pool.clone()));
        let upload_service = Arc::new(UploadService::new(
            repo,
            storage,
            event_tx.clone(),
            config.upload_expiry_hours,
            metrics.clone(),
            config.quota_max_storage_bytes,
            config.quota_max_active_uploads,
            context_cache.clone(),
        ));
        let webhook_repo: Arc<dyn WebhookRepository> =
            Arc::new(SqliteWebhookRepository::new(pool.clone()));

        Self {
            upload_service,
            webhook_repo,
            config: Arc::new(config),
            event_tx,
            db_pool: pool,
            metrics,
            metrics_registry,
            pipeline,
            rate_limiter,
            login_throttle,
            dummy_hash,
            oidc_config,
            context_cache,
        }
    }
}
