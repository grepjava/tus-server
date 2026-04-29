use axum::{
    routing::{head, post},
    Router,
};

use crate::app_state::AppState;
use super::handlers::{create_upload, delete_upload, get_upload_offset, tus_options, upload_chunk};

pub fn tus_router(state: AppState) -> Router {
    Router::new()
        .route("/", post(create_upload).options(tus_options))
        .route(
            "/{id}",
            head(get_upload_offset)
                .patch(upload_chunk)
                .delete(delete_upload),
        )
        .with_state(state)
}
