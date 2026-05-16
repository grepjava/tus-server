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

    let retention_days = state.config.webhook_delivery_retention_days;
    match state.webhook_repo.prune_old_deliveries(retention_days).await {
        Ok(0) => {}
        Ok(n) => info!(count = n, "pruned old webhook deliveries"),
        Err(e) => error!("webhook delivery pruning failed: {e}"),
    }

    match sqlx::query(
        "DELETE FROM sessions WHERE datetime(expires_at) <= datetime('now')",
    )
    .execute(&state.db_pool)
    .await
    {
        Ok(r) if r.rows_affected() > 0 => {
            info!(count = r.rows_affected(), "pruned expired sessions")
        }
        Ok(_) => {}
        Err(e) => error!("session pruning failed: {e}"),
    }

    let audit_retention = state.config.audit_log_retention_days;
    if audit_retention > 0 {
        let threshold = format!("-{audit_retention} days");
        match sqlx::query(
            "DELETE FROM audit_log WHERE datetime(created_at) < datetime('now', ?)",
        )
        .bind(&threshold)
        .execute(&state.db_pool)
        .await
        {
            Ok(r) if r.rows_affected() > 0 => {
                info!(count = r.rows_affected(), "pruned old audit log entries")
            }
            Ok(_) => {}
            Err(e) => error!("audit log pruning failed: {e}"),
        }
    }
}
