use axum::{
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use tower_http::services::{ServeDir, ServeFile};

use crate::app_state::AppState;
use super::{
    context_handlers::{
        create_context, delete_context, get_context, list_contexts, rotate_context_key,
        update_context,
    },
    handlers::{
        create_webhook, delete_setting, delete_upload, delete_webhook, get_events, get_upload,
        health, list_audit, list_settings, list_uploads, list_webhook_deliveries, list_webhooks,
        mark_abandoned, purge_uploads, retry_processing, update_setting, update_webhook,
    },
    oidc::{auth_config_handler, oidc_callback_handler, oidc_login_handler},
    session::{login_handler, logout_handler, me_handler, session_auth_middleware},
    sse::stream_events,
    user_handlers::{change_password, create_user, delete_user, list_users},
};

pub fn dashboard_router(state: AppState) -> Router {
    // Public routes — never touch session_auth_middleware.
    // Use full /api/* paths and merge (not nest) so there is zero ambiguity.
    let public = Router::new()
        .route("/api/health",                   get(health))
        .route("/api/auth/login",               post(login_handler))
        .route("/api/auth/config",              get(auth_config_handler))
        .route("/api/auth/oidc/login",          get(oidc_login_handler))
        .route("/api/auth/oidc/callback",       get(oidc_callback_handler))
        .with_state(state.clone());

    // Protected routes — session_auth_middleware applied as a layer here only.
    let protected = Router::new()
        .route("/api/auth/me",                         get(me_handler))
        .route("/api/auth/logout",                     post(logout_handler))
        .route("/api/uploads",                         get(list_uploads))
        .route("/api/uploads/purge",                   post(purge_uploads))
        .route("/api/uploads/{id}",                    get(get_upload).delete(delete_upload))
        .route("/api/uploads/{id}/events",             get(get_events))
        .route("/api/uploads/{id}/stream",             get(stream_events))
        .route("/api/uploads/{id}/retry-processing",   post(retry_processing))
        .route("/api/uploads/{id}/mark-abandoned",     post(mark_abandoned))
        .route("/api/webhooks",                        get(list_webhooks).post(create_webhook))
        .route("/api/webhooks/{id}",                   put(update_webhook).delete(delete_webhook))
        .route("/api/webhooks/{id}/deliveries",        get(list_webhook_deliveries))
        .route("/api/audit",                           get(list_audit))
        .route("/api/settings",                        get(list_settings))
        .route("/api/settings/{key}",                  put(update_setting).delete(delete_setting))
        .route("/api/users",                           get(list_users).post(create_user))
        .route("/api/users/{id}",                      delete(delete_user))
        .route("/api/users/{id}/password",             put(change_password))
        .route("/api/contexts",                        get(list_contexts).post(create_context))
        .route("/api/contexts/{id}",                   get(get_context).put(update_context).delete(delete_context))
        .route("/api/contexts/{id}/rotate-key",        post(rotate_context_key))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            session_auth_middleware,
        ))
        .with_state(state);

    Router::new()
        .merge(public)
        .merge(protected)
        .fallback_service(
            ServeDir::new("dashboard-ui/build")
                .fallback(ServeFile::new("dashboard-ui/build/index.html")),
        )
}
