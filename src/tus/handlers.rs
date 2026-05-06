use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use crate::app_state::AppState;
use super::error::TusError;
use super::model::UploadStatus;

const TUS_VERSION: &str = "1.0.0";
const TUS_SUPPORTED_VERSIONS: &str = "1.0.0";
const TUS_EXTENSIONS: &str = "creation,creation-defer-length,termination,concatenation,checksum,expiration";
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

fn parse_checksum(headers: &HeaderMap) -> Result<Option<(String, Vec<u8>)>, TusError> {
    let Some(header) = headers.get("Upload-Checksum").and_then(|v| v.to_str().ok()) else {
        return Ok(None);
    };
    let mut parts = header.trim().splitn(2, ' ');
    let alg = parts
        .next()
        .ok_or_else(|| TusError::InvalidHeader("Upload-Checksum: missing algorithm".into()))?;
    let b64 = parts
        .next()
        .ok_or_else(|| TusError::InvalidHeader("Upload-Checksum: missing checksum value".into()))?;
    let bytes = B64
        .decode(b64.trim())
        .map_err(|e| TusError::InvalidHeader(format!("Upload-Checksum: invalid base64: {e}")))?;
    Ok(Some((alg.to_string(), bytes)))
}

fn upload_expires_at(created_at: &str, expiry_hours: i64) -> String {
    use chrono::{DateTime, Duration, Utc};
    let dt: DateTime<Utc> = DateTime::parse_from_rfc3339(created_at)
        .map(Into::into)
        .unwrap_or_else(|_| Utc::now());
    (dt + Duration::hours(expiry_hours))
        .format("%a, %d %b %Y %H:%M:%S GMT")
        .to_string()
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
             "Upload-Offset, Upload-Length, Upload-Metadata, Upload-Checksum, \
              Upload-Defer-Length, Upload-Concat, Tus-Resumable, Content-Type"),
            ("Access-Control-Expose-Headers",
             "Upload-Offset, Upload-Length, Location, Tus-Resumable, Tus-Version, \
              Upload-Expires, Upload-Concat, Upload-Defer-Length, Upload-Checksum"),
        ],
    )
}

pub async fn create_upload(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    let has_length = headers.get("Upload-Length").is_some();
    let defer = headers
        .get("Upload-Defer-Length")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim() == "1")
        .unwrap_or(false);

    if defer && has_length {
        return Err(TusError::InvalidHeader(
            "Upload-Length and Upload-Defer-Length are mutually exclusive".into(),
        ));
    }
    if !defer && !has_length {
        // May be omitted for concat-final, which provides its own length
        let is_final_concat = headers
            .get("Upload-Concat")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().starts_with("final"))
            .unwrap_or(false);
        if !is_final_concat {
            return Err(TusError::MissingHeader("Upload-Length"));
        }
    }

    let (upload_length, length_is_deferred) = if defer {
        (0i64, true)
    } else if has_length {
        let length: i64 = headers
            .get("Upload-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .ok_or(TusError::MissingHeader("Upload-Length"))?;
        if length < 0 {
            return Err(TusError::InvalidHeader(
                "Upload-Length must be non-negative".into(),
            ));
        }
        if length > state.config.max_upload_bytes {
            return Err(TusError::ExceedsUploadLength);
        }
        (length, false)
    } else {
        (0i64, false)
    };

    let metadata = headers.get("Upload-Metadata").and_then(|v| v.to_str().ok());

    // Handle Upload-Concat
    let concat_raw = headers
        .get("Upload-Concat")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string());

    if let Some(ref concat) = concat_raw {
        // Accept "final ;urls", "final;urls", "final  ;  url1 url2", etc.
        if let Some(rest) = concat.strip_prefix("final") {
            let urls_str = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ';');
            let partial_ids: Vec<String> = urls_str
                .split_whitespace()
                .filter_map(|url| url.split('/').next_back().map(str::to_string))
                .filter(|s| !s.is_empty())
                .collect();

            if partial_ids.is_empty() {
                return Err(TusError::InvalidHeader(
                    "Upload-Concat final requires at least one partial upload URL".into(),
                ));
            }

            let upload = state
                .upload_service
                .create_concat_final(partial_ids, metadata)
                .await?;

            let location = format!("{}/files/{}", state.config.base_url, upload.id);
            let expires = upload_expires_at(&upload.created_at, state.config.upload_expiry_hours);

            return Ok(Response::builder()
                .status(StatusCode::CREATED)
                .header("Tus-Resumable", TUS_VERSION)
                .header("Location", location)
                .header("Upload-Offset", upload.upload_offset.to_string())
                .header("Upload-Length", upload.upload_length.to_string())
                .header("Upload-Concat", "final")
                .header("Upload-Expires", expires)
                .body(Body::empty())
                .unwrap());
        }

        if concat.as_str() != "partial" {
            return Err(TusError::InvalidHeader(format!(
                "Upload-Concat: unsupported value: {concat}"
            )));
        }
    }

    let concat_type = concat_raw.and_then(|v| {
        if v == "partial" {
            Some("partial".to_string())
        } else {
            None
        }
    });

    let upload = state
        .upload_service
        .create_upload(upload_length, metadata, length_is_deferred, concat_type)
        .await?;

    let location = format!("{}/files/{}", state.config.base_url, upload.id);
    let expires = upload_expires_at(&upload.created_at, state.config.upload_expiry_hours);

    let mut resp = Response::builder()
        .status(StatusCode::CREATED)
        .header("Tus-Resumable", TUS_VERSION)
        .header("Location", location)
        .header("Upload-Offset", "0")
        .header("Upload-Expires", expires);

    if upload.concat_type.as_deref() == Some("partial") {
        resp = resp.header("Upload-Concat", "partial");
    }

    Ok(resp.body(Body::empty()).unwrap())
}

pub async fn get_upload_offset(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    let upload = state.upload_service.get_upload(&id).await?;

    if upload.status == UploadStatus::Abandoned {
        return Err(TusError::NotFound);
    }

    if upload.is_expired(state.config.upload_expiry_hours) {
        return Err(TusError::Expired);
    }

    let expires = upload_expires_at(&upload.created_at, state.config.upload_expiry_hours);

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Tus-Resumable", TUS_VERSION)
        .header("Upload-Offset", upload.upload_offset.to_string())
        .header("Cache-Control", "no-store")
        .header("Upload-Expires", expires);

    if !upload.length_is_deferred {
        builder = builder.header("Upload-Length", upload.upload_length.to_string());
    }

    if let Some(ct) = &upload.concat_type {
        builder = builder.header("Upload-Concat", ct.as_str());
    }

    Ok(builder.body(Body::empty()).unwrap())
}

pub async fn upload_chunk(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, TusError> {
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

    let checksum = parse_checksum(&headers)?;

    // Upload-Length may appear on a PATCH to finalize a deferred-length upload
    let new_upload_length: Option<i64> = headers
        .get("Upload-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    if let Some(len) = new_upload_length {
        if len < 0 {
            return Err(TusError::InvalidHeader(
                "Upload-Length must be non-negative".into(),
            ));
        }
    }

    let new_offset = state
        .upload_service
        .append_chunk(&id, client_offset, content_length, checksum, new_upload_length, body)
        .await?;

    Ok((
        StatusCode::NO_CONTENT,
        [
            ("Tus-Resumable", TUS_VERSION.to_string()),
            ("Upload-Offset", new_offset.to_string()),
        ],
    ))
}

pub async fn delete_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    state.upload_service.delete_upload(&id).await?;

    Ok((
        StatusCode::NO_CONTENT,
        [("Tus-Resumable", TUS_VERSION)],
    ))
}
