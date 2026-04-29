use std::{sync::Arc, time::Duration};

use tokio::sync::broadcast;
use uuid::Uuid;

use crate::tus::{UploadEvent, UploadService};
use super::{
    model::{NewWebhookDelivery, WebhookConfig},
    repository::WebhookRepository,
};

pub struct WebhookDispatcher {
    pub repo: Arc<dyn WebhookRepository>,
    upload_service: Arc<UploadService>,
    client: reqwest::Client,
}

impl WebhookDispatcher {
    pub fn new(repo: Arc<dyn WebhookRepository>, upload_service: Arc<UploadService>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self { repo, upload_service, client }
    }

    pub async fn run(self: Arc<Self>, mut rx: broadcast::Receiver<UploadEvent>) {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let d = Arc::clone(&self);
                    tokio::spawn(async move { d.dispatch(event).await });
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn dispatch(&self, event: UploadEvent) {
        let hooks = match self.repo.list_for_event(&event.event_type).await {
            Ok(h) => h,
            Err(_) => return,
        };
        for hook in hooks {
            self.fire(&hook, &event).await;
        }
    }

    async fn fire(&self, hook: &WebhookConfig, event: &UploadEvent) {
        // Enrich payload with upload details if available
        let upload = self.upload_service.get_upload(&event.upload_id).await.ok();

        let payload = serde_json::json!({
            "event_type":    event.event_type,
            "upload_id":     event.upload_id,
            "event_id":      event.id,
            "message":       event.message,
            "timestamp":     event.created_at,
            "file": upload.as_ref().map(|u| serde_json::json!({
                "filename":      u.filename,
                "storage_path":  u.storage_path,
                "size":          u.upload_length,
                "offset":        u.upload_offset,
                "status":        u.status,
            })),
        });

        let mut last_error: Option<String> = None;
        let mut last_status: Option<i64> = None;
        let mut last_body: Option<String> = None;
        let mut attempts = 0i64;

        for attempt in 1u32..=3 {
            attempts = attempt as i64;

            let mut builder = self
                .client
                .post(&hook.url)
                .header("X-Webhook-Event", &event.event_type);

            if let Some(secret) = &hook.secret {
                builder = builder.header("X-Webhook-Secret", secret);
            }

            match builder.json(&payload).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    last_status = Some(status.as_u16() as i64);
                    last_body = resp.text().await.ok();

                    if status.is_success() {
                        last_error = None;
                        break;
                    }

                    last_error = Some(format!("HTTP {}", status.as_u16()));
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                    last_status = None;
                    last_body = None;
                }
            }

            if attempt < 3 {
                tokio::time::sleep(Duration::from_secs(if attempt == 1 { 1 } else { 4 })).await;
            }
        }

        let _ = self
            .repo
            .insert_delivery(NewWebhookDelivery {
                id: Uuid::new_v4().to_string(),
                webhook_id: hook.id.clone(),
                upload_id: Some(event.upload_id.clone()),
                event_type: event.event_type.clone(),
                payload: payload.to_string(),
                status_code: last_status,
                response_body: last_body,
                error: last_error,
                attempts,
            })
            .await;
    }
}
