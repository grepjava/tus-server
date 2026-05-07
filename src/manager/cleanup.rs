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
                info!(upload_id = %upload.id, "abandoning stale upload and deleting storage");
                if let Err(e) = state.upload_service.abandon_and_delete(&upload.id).await {
                    warn!(upload_id = %upload.id, error = %e, "failed to abandon stale upload");
                }
            }
        }
        Err(e) => error!("cleanup scan failed: {e}"),
    }
}
