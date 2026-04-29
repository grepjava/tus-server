use async_trait::async_trait;
use uuid::Uuid;

use crate::tus::TusError;
use super::model::{NewWebhookConfig, NewWebhookDelivery, UpdateWebhookConfig, WebhookConfig, WebhookDelivery, WebhookRow};

#[async_trait]
pub trait WebhookRepository: Send + Sync {
    async fn create(&self, config: NewWebhookConfig) -> Result<WebhookConfig, TusError>;
    async fn list(&self) -> Result<Vec<WebhookConfig>, TusError>;
    async fn update(&self, id: &str, update: UpdateWebhookConfig) -> Result<WebhookConfig, TusError>;
    async fn delete(&self, id: &str) -> Result<(), TusError>;
    async fn list_for_event(&self, event_type: &str) -> Result<Vec<WebhookConfig>, TusError>;
    async fn insert_delivery(&self, delivery: NewWebhookDelivery) -> Result<(), TusError>;
    async fn list_deliveries(&self, webhook_id: &str) -> Result<Vec<WebhookDelivery>, TusError>;
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
            "UPDATE webhooks SET name = ?, url = ?, secret = ?, events = ?, enabled = ?, \
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

    async fn list_for_event(&self, event_type: &str) -> Result<Vec<WebhookConfig>, TusError> {
        sqlx::query_as::<_, WebhookRow>(
            "SELECT DISTINCT w.id, w.name, w.url, w.secret, w.events, w.enabled, w.created_at, w.updated_at \
             FROM webhooks w, json_each(w.events) e \
             WHERE w.enabled = 1 AND e.value = ?",
        )
        .bind(event_type)
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
}
