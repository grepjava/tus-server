use axum::{
    http::{header, HeaderName},
    routing::{head, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::app_state::AppState;
use super::handlers::{create_upload, delete_upload, get_upload_offset, tus_options, upload_chunk};

pub fn tus_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
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
        ]);

    Router::new()
        .route("/", post(create_upload).options(tus_options))
        .route(
            "/{id}",
            head(get_upload_offset)
                .patch(upload_chunk)
                .delete(delete_upload)
                .options(tus_options),
        )
        .layer(cors)
        .with_state(state)
}
