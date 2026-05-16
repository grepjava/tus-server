use axum::{
    http::{header, HeaderName},
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::app_state::AppState;
use crate::context::context_auth_middleware;
use super::handlers::{
    create_upload, ctx_create_upload, ctx_delete_upload, ctx_download_upload,
    ctx_get_upload_offset, ctx_upload_chunk, delete_upload, download_upload, get_upload_offset,
    tus_options, upload_chunk,
};

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers([
            HeaderName::from_static("tus-resumable"),
            HeaderName::from_static("tus-version"),
            HeaderName::from_static("upload-offset"),
            HeaderName::from_static("upload-length"),
            HeaderName::from_static("upload-expires"),
            HeaderName::from_static("upload-concat"),
            HeaderName::from_static("upload-defer-length"),
            HeaderName::from_static("upload-checksum"),
            header::LOCATION,
            header::ACCEPT_RANGES,
            header::CONTENT_RANGE,
            header::CONTENT_DISPOSITION,
            header::CONTENT_TYPE,
        ])
}

/// Legacy TUS router mounted at `/files`. Context-free (global API key).
pub fn tus_router(state: AppState) -> Router {
    Router::new()
        .route("/", post(create_upload).options(tus_options))
        .route(
            "/{id}",
            get(download_upload)
                .head(get_upload_offset)
                .patch(upload_chunk)
                .delete(delete_upload)
                .options(tus_options),
        )
        .layer(cors_layer())
        .with_state(state)
}

/// Context-scoped TUS router mounted at the root.
/// Routes: `/{context}/files` and `/{context}/files/{id}`.
pub fn context_tus_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/{context}/files",
            post(ctx_create_upload).options(tus_options),
        )
        .route(
            "/{context}/files/{id}",
            get(ctx_download_upload)
                .head(ctx_get_upload_offset)
                .patch(ctx_upload_chunk)
                .delete(ctx_delete_upload)
                .options(tus_options),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            context_auth_middleware,
        ))
        .layer(cors_layer())
        .with_state(state)
}
