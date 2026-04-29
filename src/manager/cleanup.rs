use std::time::Duration;
use tracing::{error, info, warn};

use crate::app_state::AppState;

pub async fn run(state: AppState) {
    let mut ticker = tokio::time::interval(Duration::from_secs(state.config.cleanup_interval_secs));

    loop {
        ticker.tick().await;
        run_once(state.clone()).await;
    }
}

async fn run_once(state: AppState) {
    let hours = state.config.abandoned_after_hours;

    match state.upload_service.find_stale(hours).await {
        Ok(stale) => {
            for upload in stale {
                info!(upload_id = %upload.id, "marking stale upload as abandoned");
                if let Err(e) = state.upload_service.mark_abandoned(&upload.id).await {
                    warn!(upload_id = %upload.id, error = %e, "failed to mark abandoned");
                }
            }
        }
        Err(e) => error!("cleanup scan failed: {e}"),
    }
}
