use std::sync::Arc;
use tokio::sync::broadcast;

use crate::{
    config::Config,
    tus::{FilesystemStorage, SqliteUploadRepository, UploadEvent, UploadService},
    webhook::{SqliteWebhookRepository, WebhookRepository},
};

#[derive(Clone)]
pub struct AppState {
    pub upload_service: Arc<UploadService>,
    pub webhook_repo: Arc<dyn WebhookRepository>,
    pub config: Arc<Config>,
    pub event_tx: broadcast::Sender<UploadEvent>,
}

impl AppState {
    pub fn new(
        pool: sqlx::SqlitePool,
        config: Config,
        event_tx: broadcast::Sender<UploadEvent>,
    ) -> Self {
        let repo = Arc::new(SqliteUploadRepository::new(pool.clone()));
        let storage = Arc::new(FilesystemStorage::new(config.storage_dir.clone()));
        let upload_service = Arc::new(UploadService::new(
            repo,
            storage,
            event_tx.clone(),
            config.upload_expiry_hours,
        ));
        let webhook_repo: Arc<dyn WebhookRepository> =
            Arc::new(SqliteWebhookRepository::new(pool));

        Self {
            upload_service,
            webhook_repo,
            config: Arc::new(config),
            event_tx,
        }
    }
}
