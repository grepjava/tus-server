use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewWebhookConfig {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateWebhookConfig {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WebhookDelivery {
    pub id: String,
    pub webhook_id: String,
    pub upload_id: Option<String>,
    pub event_type: String,
    pub payload: String,
    pub status_code: Option<i64>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub attempts: i64,
    pub delivered_at: String,
}

#[derive(Debug)]
pub struct NewWebhookDelivery {
    pub id: String,
    pub webhook_id: String,
    pub upload_id: Option<String>,
    pub event_type: String,
    pub payload: String,
    pub status_code: Option<i64>,
    pub response_body: Option<String>,
    pub error: Option<String>,
    pub attempts: i64,
}

// Intermediate row type for SQLx since `events` is stored as JSON text.
#[derive(sqlx::FromRow)]
pub struct WebhookRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl TryFrom<WebhookRow> for WebhookConfig {
    type Error = serde_json::Error;
    fn try_from(row: WebhookRow) -> Result<Self, Self::Error> {
        Ok(WebhookConfig {
            id: row.id,
            name: row.name,
            url: row.url,
            secret: row.secret,
            events: serde_json::from_str(&row.events)?,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}
