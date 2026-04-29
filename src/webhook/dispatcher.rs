use std::{path::PathBuf, sync::Arc, time::Duration};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::tus::{UploadEvent, UploadService};
use super::{
    model::{NewWebhookDelivery, WebhookConfig},
    repository::WebhookRepository,
};

const MAX_RESPONSE_BODY: usize = 4096;

pub struct WebhookDispatcher {
    pub repo: Arc<dyn WebhookRepository>,
    upload_service: Arc<UploadService>,
    storage_dir: PathBuf,
    client: reqwest::Client,
}

impl WebhookDispatcher {
    pub fn new(
        repo: Arc<dyn WebhookRepository>,
        upload_service: Arc<UploadService>,
        storage_dir: PathBuf,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self { repo, upload_service, storage_dir, client }
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
            Err(e) => {
                tracing::error!(
                    event_type = %event.event_type,
                    upload_id  = %event.upload_id,
                    error      = %e,
                    "failed to load webhooks for event"
                );
                return;
            }
        };
        for hook in hooks {
            self.fire(&hook, &event).await;
        }
    }

    async fn fire(&self, hook: &WebhookConfig, event: &UploadEvent) {
        let upload = self.upload_service.get_upload(&event.upload_id).await.ok();

        let file_info = upload.as_ref().map(|u| {
            let abs_path = std::fs::canonicalize(self.storage_dir.join(&u.storage_path))
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| {
                    self.storage_dir
                        .join(&u.storage_path)
                        .to_string_lossy()
                        .to_string()
                });

            serde_json::json!({
                "filename":      u.filename,
                "storage_path":  u.storage_path,
                "absolute_path": abs_path,
                "size":          u.upload_length,
                "offset":        u.upload_offset,
                "status":        u.status,
            })
        });

        let payload = serde_json::json!({
            "event_type":  event.event_type,
            "upload_id":   event.upload_id,
            "event_id":    event.id,
            "message":     event.message,
            "timestamp":   event.created_at,
            "file":        file_info,
        });

        // Serialize once; used for both the request body and HMAC signing
        let body_bytes = match serde_json::to_vec(&payload) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(webhook_id = %hook.id, error = %e, "failed to serialize webhook payload");
                return;
            }
        };

        let mut last_error: Option<String> = None;
        let mut last_status: Option<i64> = None;
        let mut last_body: Option<String> = None;
        let mut attempts = 0i64;

        for attempt in 1u32..=3 {
            attempts = attempt as i64;

            let mut builder = self
                .client
                .post(&hook.url)
                .header("Content-Type", "application/json")
                .header("X-Webhook-Event", &event.event_type)
                .body(body_bytes.clone());

            // Sign with HMAC-SHA256 instead of sending the raw secret
            if let Some(secret) = &hook.secret {
                let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                    .expect("HMAC accepts any key length");
                mac.update(&body_bytes);
                let sig = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
                builder = builder.header("X-Hub-Signature-256", sig);
            }

            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    last_status = Some(status.as_u16() as i64);

                    // Cap response body to avoid unbounded memory use
                    let bytes = resp.bytes().await.unwrap_or_default();
                    let truncated = &bytes[..bytes.len().min(MAX_RESPONSE_BODY)];
                    last_body = Some(String::from_utf8_lossy(truncated).to_string());

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

        if let Err(e) = self
            .repo
            .insert_delivery(NewWebhookDelivery {
                id: Uuid::new_v4().to_string(),
                webhook_id: hook.id.clone(),
                upload_id: Some(event.upload_id.clone()),
                event_type: event.event_type.clone(),
                payload: String::from_utf8_lossy(&body_bytes).to_string(),
                status_code: last_status,
                response_body: last_body,
                error: last_error,
                attempts,
            })
            .await
        {
            tracing::error!(webhook_id = %hook.id, error = %e, "failed to record webhook delivery");
        }
    }
}
