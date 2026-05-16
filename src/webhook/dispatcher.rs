use std::{path::PathBuf, sync::Arc, time::Duration};

use futures::StreamExt;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tokio::sync::{broadcast, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{
    config::Config,
    metrics::{AppMetrics, DeliveryLabels},
    tus::{UploadEvent, UploadService},
};
use super::{
    model::{NewWebhookDelivery, WebhookConfig},
    repository::WebhookRepository,
    validation::ssrf_safe_client,
};

const MAX_RESPONSE_BODY: usize = 4096;
const MAX_CONCURRENT_DISPATCHES: usize = 32;

pub struct WebhookDispatcher {
    pub repo: Arc<dyn WebhookRepository>,
    upload_service: Arc<UploadService>,
    storage_dir: PathBuf,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
    config: Arc<Config>,
    metrics: Arc<AppMetrics>,
}

impl WebhookDispatcher {
    pub fn new(
        repo: Arc<dyn WebhookRepository>,
        upload_service: Arc<UploadService>,
        storage_dir: PathBuf,
        config: Arc<Config>,
        metrics: Arc<AppMetrics>,
    ) -> Self {
        Self {
            repo,
            upload_service,
            storage_dir,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DISPATCHES)),
            client: ssrf_safe_client(),
            config,
            metrics,
        }
    }

    pub async fn run(self: Arc<Self>, mut rx: broadcast::Receiver<UploadEvent>) {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let d = Arc::clone(&self);
                    tokio::spawn(async move {
                        // Acquire a slot before doing any real work — caps concurrency
                        // at MAX_CONCURRENT_DISPATCHES regardless of event burst size.
                        let _permit = d.semaphore.acquire().await
                            .expect("semaphore closed unexpectedly");
                        d.dispatch(event).await;
                    });
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!(dropped = n, "webhook dispatcher lagged, scanning for missed deliveries");
                    let d = Arc::clone(&self);
                    tokio::spawn(async move {
                        match d.repo.list_missed_events(15).await {
                            Ok(events) if events.is_empty() => {}
                            Ok(events) => {
                                info!(count = events.len(), "re-dispatching missed webhook events");
                                for event in events {
                                    let d2 = Arc::clone(&d);
                                    tokio::spawn(async move {
                                        let _permit = d2.semaphore.acquire().await
                                            .expect("semaphore closed unexpectedly");
                                        d2.dispatch(event).await;
                                    });
                                }
                            }
                            Err(e) => error!(error = %e, "webhook lag recovery scan failed"),
                        }
                    });
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    async fn dispatch(&self, event: UploadEvent) {
        let hooks = match self.repo.list_for_event(&event.event_type, event.context_id.as_deref()).await {
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

        let max_attempts = self.config.webhook_max_attempts;

        for attempt in 1u32..=max_attempts {
            attempts = attempt as i64;

            let mut builder = self
                .client
                .post(&hook.url)
                .header("Content-Type", "application/json")
                .header("X-Webhook-Event", &event.event_type)
                .body(body_bytes.clone());

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

                    // Stream the response body and stop once the cap is reached —
                    // avoids buffering an unbounded response into memory.
                    let mut stream = resp.bytes_stream();
                    let mut body_buf: Vec<u8> = Vec::with_capacity(MAX_RESPONSE_BODY);
                    while let Some(chunk) = stream.next().await {
                        if let Ok(bytes) = chunk {
                            let bytes: axum::body::Bytes = bytes;
                            let remaining = MAX_RESPONSE_BODY.saturating_sub(body_buf.len());
                            if remaining == 0 { break; }
                            body_buf.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                        }
                    }
                    last_body = Some(String::from_utf8_lossy(&body_buf).to_string());

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

            if attempt < max_attempts {
                let delay_idx = (attempt - 1) as usize;
                let delay = self.config.webhook_retry_delays_secs
                    .get(delay_idx)
                    .copied()
                    .unwrap_or_else(|| *self.config.webhook_retry_delays_secs.last().unwrap_or(&1));
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
        }

        let outcome = if last_error.is_none() { "success" } else { "failure" };
        self.metrics
            .webhook_deliveries_total
            .get_or_create(&DeliveryLabels { outcome: outcome.to_string() })
            .inc();

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
