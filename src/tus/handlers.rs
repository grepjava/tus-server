use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use crate::app_state::AppState;
use super::error::TusError;
use super::model::UploadStatus;

const TUS_VERSION: &str = "1.0.0";
const TUS_SUPPORTED_VERSIONS: &str = "1.0.0";
const TUS_EXTENSIONS: &str = "creation,termination";
const TUS_MAX_SIZE: &str = "107374182400";

fn require_tus_resumable(headers: &HeaderMap) -> Result<(), TusError> {
    let version = headers
        .get("Tus-Resumable")
        .and_then(|v| v.to_str().ok())
        .ok_or(TusError::MissingHeader("Tus-Resumable"))?;

    if version != TUS_VERSION {
        return Err(TusError::UnsupportedVersion(version.to_string()));
    }

    Ok(())
}

pub async fn tus_options() -> impl IntoResponse {
    (
        StatusCode::NO_CONTENT,
        [
            ("Tus-Resumable", TUS_VERSION),
            ("Tus-Version", TUS_SUPPORTED_VERSIONS),
            ("Tus-Extension", TUS_EXTENSIONS),
            ("Tus-Max-Size", TUS_MAX_SIZE),
            ("Access-Control-Allow-Methods", "POST, HEAD, PATCH, DELETE, OPTIONS"),
            ("Access-Control-Allow-Headers",
             "Upload-Offset, Upload-Length, Upload-Metadata, Tus-Resumable, Content-Type"),
            ("Access-Control-Expose-Headers",
             "Upload-Offset, Upload-Length, Location, Tus-Resumable"),
        ],
    )
}

pub async fn create_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, TusError> {
    require_tus_resumable(&headers)?;

    let upload_length: i64 = headers
        .get("Upload-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or(TusError::MissingHeader("Upload-Length"))?;

    if upload_length < 0 {
        return Err(TusError::InvalidHeader(
            "Upload-Length must be non-negative".into(),
        ));
    }

    if upload_length > state.config.max_upload_bytes {
        return Err(TusError::ExceedsUploadLength);
    }

    let metadata = headers
        .get("Upload-Metadata")
        .and_then(|v| v.to_str().ok());

    let upload = state
        .upload_service
        .create_upload(upload_length, metadata)
        .await?;

    let location = format!("{}/files/{}", state.config.base_url, upload.id);

    Ok((
        StatusCode::CREATED,
        [
            ("Tus-Resumable", TUS_VERSION.to_string()),
            ("Location", location),
            ("Upload-Offset", "0".to_string()),
        ],
    )
        .into_response())
}

pub async fn get_upload_offset(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, TusError> {
    require_tus_resumable(&headers)?;

    let upload = state.upload_service.get_upload(&id).await?;

    if upload.status == UploadStatus::Abandoned {
        return Err(TusError::NotFound);
    }

    Ok((
        StatusCode::OK,
        [
            ("Tus-Resumable", TUS_VERSION.to_string()),
            ("Upload-Offset", upload.upload_offset.to_string()),
            ("Upload-Length", upload.upload_length.to_string()),
            ("Cache-Control", "no-store".to_string()),
        ],
    )
        .into_response())
}

pub async fn upload_chunk(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, TusError> {
    require_tus_resumable(&headers)?;

    let content_type = headers
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .ok_or(TusError::MissingHeader("Content-Type"))?;

    if !content_type.starts_with("application/offset+octet-stream") {
        return Err(TusError::InvalidContentType);
    }

    let client_offset: i64 = headers
        .get("Upload-Offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or(TusError::MissingHeader("Upload-Offset"))?;

    if client_offset < 0 {
        return Err(TusError::InvalidHeader(
            "Upload-Offset must be non-negative".into(),
        ));
    }

    let content_length: Option<i64> = headers
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    let new_offset = state
        .upload_service
        .append_chunk(&id, client_offset, content_length, body)
        .await?;

    Ok((
        StatusCode::NO_CONTENT,
        [
            ("Tus-Resumable", TUS_VERSION.to_string()),
            ("Upload-Offset", new_offset.to_string()),
        ],
    )
        .into_response())
}

pub async fn delete_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, TusError> {
    require_tus_resumable(&headers)?;

    state.upload_service.delete_upload(&id).await?;

    Ok((
        StatusCode::NO_CONTENT,
        [("Tus-Resumable", TUS_VERSION)],
    )
        .into_response())
}
