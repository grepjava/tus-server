use async_trait::async_trait;
use uuid::Uuid;

use crate::tus::{TusError, UploadEvent};
use super::model::{NewWebhookConfig, NewWebhookDelivery, UpdateWebhookConfig, WebhookConfig, WebhookDelivery, WebhookRow};

#[async_trait]
pub trait WebhookRepository: Send + Sync {
    async fn create(&self, config: NewWebhookConfig) -> Result<WebhookConfig, TusError>;
    async fn list(&self) -> Result<Vec<WebhookConfig>, TusError>;
    async fn update(&self, id: &str, update: UpdateWebhookConfig) -> Result<WebhookConfig, TusError>;
    async fn delete(&self, id: &str) -> Result<(), TusError>;
    async fn list_for_event(&self, event_type: &str, context_id: Option<&str>) -> Result<Vec<WebhookConfig>, TusError>;
    async fn insert_delivery(&self, delivery: NewWebhookDelivery) -> Result<(), TusError>;
    async fn list_deliveries(&self, webhook_id: &str) -> Result<Vec<WebhookDelivery>, TusError>;
    /// Returns upload events from the last `since_minutes` minutes that have no
    /// corresponding webhook_deliveries row — used to recover from broadcast lag.
    async fn list_missed_events(&self, since_minutes: i64) -> Result<Vec<UploadEvent>, TusError>;
    /// Deletes delivery records older than `older_than_days` days. Returns the number deleted.
    async fn prune_old_deliveries(&self, older_than_days: i64) -> Result<u64, TusError>;
}

pub struct SqliteWebhookRepository {
    pool: sqlx::SqlitePool,
}

impl SqliteWebhookRepository {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_to_config(row: WebhookRow) -> Result<WebhookConfig, TusError> {
    row.try_into().map_err(|e: serde_json::Error| TusError::Internal(anyhow::Error::new(e)))
}

#[async_trait]
impl WebhookRepository for SqliteWebhookRepository {
    async fn create(&self, config: NewWebhookConfig) -> Result<WebhookConfig, TusError> {
        let id = Uuid::new_v4().to_string();
        let events_json = serde_json::to_string(&config.events)
            .map_err(|e| TusError::Internal(anyhow::Error::new(e)))?;

        sqlx::query_as::<_, WebhookRow>(
            "INSERT INTO webhooks (id, name, url, secret, events) VALUES (?, ?, ?, ?, ?) RETURNING *",
        )
        .bind(&id)
        .bind(&config.name)
        .bind(&config.url)
        .bind(&config.secret)
        .bind(&events_json)
        .fetch_one(&self.pool)
        .await
        .map_err(TusError::Database)
        .and_then(row_to_config)
    }

    async fn list(&self) -> Result<Vec<WebhookConfig>, TusError> {
        sqlx::query_as::<_, WebhookRow>("SELECT * FROM webhooks ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await
            .map_err(TusError::Database)?
            .into_iter()
            .map(row_to_config)
            .collect()
    }

    async fn update(&self, id: &str, update: UpdateWebhookConfig) -> Result<WebhookConfig, TusError> {
        let events_json = serde_json::to_string(&update.events)
            .map_err(|e| TusError::Internal(anyhow::Error::new(e)))?;

        sqlx::query_as::<_, WebhookRow>(
            "UPDATE webhooks SET name = ?, url = ?, secret = COALESCE(?, secret), events = ?, enabled = ?, \
             updated_at = datetime('now') WHERE id = ? RETURNING *",
        )
        .bind(&update.name)
        .bind(&update.url)
        .bind(&update.secret)
        .bind(&events_json)
        .bind(update.enabled)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(TusError::Database)?
        .ok_or(TusError::NotFound)
        .and_then(row_to_config)
    }

    async fn delete(&self, id: &str) -> Result<(), TusError> {
        let result = sqlx::query("DELETE FROM webhooks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(TusError::Database)?;

        if result.rows_affected() == 0 {
            return Err(TusError::NotFound);
        }
        Ok(())
    }

    async fn list_for_event(&self, event_type: &str, context_id: Option<&str>) -> Result<Vec<WebhookConfig>, TusError> {
        // Match webhooks subscribed to event_type that are scoped to this context OR global
        // (context_id IS NULL means the webhook applies to all contexts / unscoped uploads).
        // When context_id is None (legacy unscoped upload), only global webhooks fire.
        sqlx::query_as::<_, WebhookRow>(
            "SELECT DISTINCT w.id, w.name, w.url, w.secret, w.events, w.enabled, w.created_at, w.updated_at \
             FROM webhooks w, json_each(w.events) e \
             WHERE w.enabled = 1 AND e.value = ? \
             AND (w.context_id = ? OR w.context_id IS NULL)",
        )
        .bind(event_type)
        .bind(context_id)
        .fetch_all(&self.pool)
        .await
        .map_err(TusError::Database)?
        .into_iter()
        .map(row_to_config)
        .collect()
    }

    async fn insert_delivery(&self, d: NewWebhookDelivery) -> Result<(), TusError> {
        sqlx::query(
            "INSERT INTO webhook_deliveries \
             (id, webhook_id, upload_id, event_type, payload, status_code, response_body, error, attempts) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&d.id)
        .bind(&d.webhook_id)
        .bind(&d.upload_id)
        .bind(&d.event_type)
        .bind(&d.payload)
        .bind(d.status_code)
        .bind(&d.response_body)
        .bind(&d.error)
        .bind(d.attempts)
        .execute(&self.pool)
        .await
        .map_err(TusError::Database)?;

        Ok(())
    }

    async fn list_deliveries(&self, webhook_id: &str) -> Result<Vec<WebhookDelivery>, TusError> {
        sqlx::query_as::<_, WebhookDelivery>(
            "SELECT * FROM webhook_deliveries WHERE webhook_id = ? ORDER BY delivered_at DESC LIMIT 100",
        )
        .bind(webhook_id)
        .fetch_all(&self.pool)
        .await
        .map_err(TusError::Database)
    }

    async fn prune_old_deliveries(&self, older_than_days: i64) -> Result<u64, TusError> {
        let threshold = format!("-{older_than_days} days");
        let affected = sqlx::query(
            "DELETE FROM webhook_deliveries WHERE datetime(delivered_at) < datetime('now', ?)",
        )
        .bind(threshold)
        .execute(&self.pool)
        .await
        .map_err(TusError::Database)?
        .rows_affected();
        Ok(affected)
    }

    async fn list_missed_events(&self, since_minutes: i64) -> Result<Vec<UploadEvent>, TusError> {
        let since = format!("-{since_minutes} minutes");
        sqlx::query_as::<_, UploadEvent>(
            "SELECT id, upload_id, event_type, message, created_at, context_id \
             FROM upload_events \
             WHERE datetime(created_at) > datetime('now', ?) \
             AND NOT EXISTS ( \
                 SELECT 1 FROM webhook_deliveries wd \
                 WHERE wd.upload_id = upload_events.upload_id \
                   AND wd.event_type = upload_events.event_type \
             ) \
             ORDER BY created_at ASC",
        )
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(TusError::Database)
    }
}
