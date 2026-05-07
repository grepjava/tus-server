use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

use crate::app_state::AppState;
use super::processor;

pub async fn run(state: AppState) {
    let mut rx = state.event_tx.subscribe();

    loop {
        match rx.recv().await {
            Ok(event) if event.event_type == "completed" => {
                info!(upload_id = %event.upload_id, "upload complete, starting processing");
                let s = state.clone();
                let id = event.upload_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = processor::process(s.clone(), &id).await {
                        error!(upload_id = %id, error = %e, "processing failed");
                        let _ = s.upload_service.fail_processing(&id, &e.to_string()).await;
                    }
                });
            }

            Ok(_) => {}

            Err(RecvError::Lagged(n)) => {
                warn!("worker lagged by {n} messages, scanning for missed completions");
                scan_completed(state.clone()).await;
            }

            Err(RecvError::Closed) => break,
        }
    }
}

async fn scan_completed(state: AppState) {
    match state.upload_service.list_completed().await {
        Ok(uploads) => {
            for upload in uploads {
                let s = state.clone();
                let id = upload.id.clone();
                tokio::spawn(async move {
                    if let Err(e) = processor::process(s.clone(), &id).await {
                        error!(upload_id = %id, error = %e, "processing failed during scan");
                        let _ = s.upload_service.fail_processing(&id, &e.to_string()).await;
                    }
                });
            }
        }
        Err(e) => error!("scan failed: {e}"),
    }
}
