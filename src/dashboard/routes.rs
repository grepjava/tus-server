use axum::{routing::{get, post, put}, Router};
use tower_http::services::{ServeDir, ServeFile};

use crate::app_state::AppState;
use super::{
    handlers::{
        create_webhook, delete_upload, delete_webhook, get_events, get_upload, health,
        list_uploads, list_webhook_deliveries, list_webhooks, mark_abandoned, purge_uploads,
        retry_processing, update_webhook,
    },
    sse::stream_events,
};

pub fn dashboard_router(state: AppState) -> Router {
    let api = Router::new()
        .route("/uploads", get(list_uploads))
        .route("/uploads/purge", post(purge_uploads))
        .route("/uploads/{id}", get(get_upload).delete(delete_upload))
        .route("/uploads/{id}/events", get(get_events))
        .route("/uploads/{id}/stream", get(stream_events))
        .route("/uploads/{id}/retry-processing", post(retry_processing))
        .route("/uploads/{id}/mark-abandoned", post(mark_abandoned))
        .route("/webhooks", get(list_webhooks).post(create_webhook))
        .route("/webhooks/{id}", put(update_webhook).delete(delete_webhook))
        .route("/webhooks/{id}/deliveries", get(list_webhook_deliveries))
        .route("/health", get(health))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback_service(
            ServeDir::new("dashboard-ui/build")
                .fallback(ServeFile::new("dashboard-ui/build/index.html")),
        )
}
