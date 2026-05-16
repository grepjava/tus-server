use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, Response, StatusCode},
    response::IntoResponse,
};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

use crate::app_state::AppState;
use crate::context::RequestContext;
use super::error::TusError;
use super::model::UploadStatus;

const TUS_VERSION: &str = "1.0.0";
const TUS_SUPPORTED_VERSIONS: &str = "1.0.0";
const TUS_EXTENSIONS: &str = "creation,creation-defer-length,termination,concatenation,checksum,expiration";

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

/// Parses an `Upload-Concat: final ...` header value and returns the upload IDs.
/// Returns `None` if the value does not start with `final`.
/// Accepts `final ;urls`, `final;urls`, or any whitespace/semicolon combination.
fn parse_concat_final_ids(value: &str) -> Option<Vec<String>> {
    let rest = value.trim().strip_prefix("final")?;
    let urls_str = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ';');
    let ids = urls_str
        .split_whitespace()
        .filter_map(|url| url.split('/').next_back().map(str::to_string))
        .filter(|s| !s.is_empty())
        .collect();
    Some(ids)
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

pub async fn tus_options(State(state): State<AppState>) -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header("Tus-Resumable", TUS_VERSION)
        .header("Tus-Version", TUS_SUPPORTED_VERSIONS)
        .header("Tus-Extension", TUS_EXTENSIONS)
        .header("Tus-Max-Size", state.config.max_upload_bytes.to_string())
        .header("Access-Control-Allow-Methods", "POST, HEAD, PATCH, DELETE, OPTIONS")
        .header("Access-Control-Allow-Headers",
            "Upload-Offset, Upload-Length, Upload-Metadata, Upload-Checksum, \
             Upload-Defer-Length, Upload-Concat, Tus-Resumable, Content-Type")
        .header("Access-Control-Expose-Headers",
            "Upload-Offset, Upload-Length, Location, Tus-Resumable, Tus-Version, \
             Upload-Expires, Upload-Concat, Upload-Defer-Length, Upload-Checksum")
        .body(Body::empty())
        .unwrap()
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
        if let Some(partial_ids) = parse_concat_final_ids(concat) {

            if partial_ids.is_empty() {
                return Err(TusError::InvalidHeader(
                    "Upload-Concat final requires at least one partial upload URL".into(),
                ));
            }

            let upload = state
                .upload_service
                .create_concat_final(partial_ids, metadata, None)
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
        .create_upload(upload_length, metadata, length_is_deferred, concat_type, None)
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

pub async fn download_upload(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response<Body>, TusError> {
    let upload = state.upload_service.get_upload(&id).await?;

    if upload.status != UploadStatus::Finalized {
        return Err(TusError::NotFound);
    }

    let total = upload.upload_length as u64;

    // Empty file — skip range logic entirely.
    if total == 0 {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Accept-Ranges", "bytes")
            .header("Content-Length", "0")
            .body(Body::empty())
            .unwrap());
    }

    let range_str = headers.get(header::RANGE).and_then(|v| v.to_str().ok());

    let (offset, length, is_partial) = match parse_range(range_str, total) {
        Ok(None) => (0u64, total, false),
        Ok(Some((off, len))) => (off, len, true),
        Err(()) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("Content-Range", format!("bytes */{total}"))
                .body(Body::empty())
                .unwrap());
        }
    };

    let body = state
        .upload_service
        .open_for_read(&upload.storage_path, offset, length)
        .await?;

    let end = offset + length - 1;
    let content_type = upload
        .metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<HashMap<String, String>>(json).ok())
        .and_then(|m| m.get("filetype").or(m.get("type")).cloned())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let mut builder = Response::builder()
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", length.to_string())
        .header("Content-Type", content_type);

    if let Some(name) = &upload.filename {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        builder = builder.header(
            "Content-Disposition",
            format!("attachment; filename=\"{escaped}\""),
        );
    }

    if is_partial {
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Range", format!("bytes {offset}-{end}/{total}"));
    } else {
        builder = builder.status(StatusCode::OK);
    }

    Ok(builder.body(body).unwrap())
}

// ── Context-scoped handlers ───────────────────────────────────────────────────
//
// Routes: POST /{context}/files       → ctx_create_upload
//         HEAD /{context}/files/{id}  → ctx_get_upload_offset
//         PATCH /{context}/files/{id} → ctx_upload_chunk
//         DELETE /{context}/files/{id}→ ctx_delete_upload
//         GET /{context}/files/{id}   → ctx_download_upload

#[derive(serde::Deserialize)]
pub struct ContextIdPath {
    pub context: String,
    pub id: String,
}

pub async fn ctx_create_upload(
    State(state): State<AppState>,
    Extension(RequestContext(ctx)): Extension<RequestContext>,
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
            return Err(TusError::InvalidHeader("Upload-Length must be non-negative".into()));
        }
        let max = ctx.max_upload_bytes.unwrap_or(state.config.max_upload_bytes);
        if length > max {
            return Err(TusError::ExceedsUploadLength);
        }
        (length, false)
    } else {
        (0i64, false)
    };

    let metadata = headers.get("Upload-Metadata").and_then(|v| v.to_str().ok());

    let concat_raw = headers
        .get("Upload-Concat")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string());

    if let Some(ref concat) = concat_raw {
        if let Some(partial_ids) = parse_concat_final_ids(concat) {
            if partial_ids.is_empty() {
                return Err(TusError::InvalidHeader(
                    "Upload-Concat final requires at least one partial upload URL".into(),
                ));
            }
            let upload = state
                .upload_service
                .create_concat_final(partial_ids, metadata, Some(&ctx.id))
                .await?;

            let location = format!(
                "{}/{}/files/{}",
                state.config.base_url, ctx.slug, upload.id
            );
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
        if v == "partial" { Some("partial".to_string()) } else { None }
    });

    let upload = state
        .upload_service
        .create_upload(upload_length, metadata, length_is_deferred, concat_type, Some(&ctx.id))
        .await?;

    let location = format!("{}/{}/files/{}", state.config.base_url, ctx.slug, upload.id);
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

pub async fn ctx_get_upload_offset(
    State(state): State<AppState>,
    Extension(RequestContext(ctx)): Extension<RequestContext>,
    Path(ContextIdPath { id, .. }): Path<ContextIdPath>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    let upload = state.upload_service.get_upload(&id).await?;

    if upload.context_id.as_deref() != Some(&ctx.id) {
        return Err(TusError::NotFound);
    }
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

pub async fn ctx_upload_chunk(
    State(state): State<AppState>,
    Extension(RequestContext(ctx)): Extension<RequestContext>,
    Path(ContextIdPath { id, .. }): Path<ContextIdPath>,
    headers: HeaderMap,
    body: Body,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    // Verify upload belongs to this context before touching it.
    let upload_check = state.upload_service.get_upload(&id).await?;
    if upload_check.context_id.as_deref() != Some(&ctx.id) {
        return Err(TusError::NotFound);
    }

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
        return Err(TusError::InvalidHeader("Upload-Offset must be non-negative".into()));
    }

    let content_length: Option<i64> = headers
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    let checksum = parse_checksum(&headers)?;

    let new_upload_length: Option<i64> = headers
        .get("Upload-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    if let Some(len) = new_upload_length {
        if len < 0 {
            return Err(TusError::InvalidHeader("Upload-Length must be non-negative".into()));
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

pub async fn ctx_delete_upload(
    State(state): State<AppState>,
    Extension(RequestContext(ctx)): Extension<RequestContext>,
    Path(ContextIdPath { id, .. }): Path<ContextIdPath>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, TusError> {
    require_tus_resumable(&headers)?;

    let upload = state.upload_service.get_upload(&id).await?;
    if upload.context_id.as_deref() != Some(&ctx.id) {
        return Err(TusError::NotFound);
    }

    state.upload_service.delete_upload(&id).await?;
    Ok((StatusCode::NO_CONTENT, [("Tus-Resumable", TUS_VERSION)]))
}

pub async fn ctx_download_upload(
    State(state): State<AppState>,
    Extension(RequestContext(ctx)): Extension<RequestContext>,
    Path(ContextIdPath { id, .. }): Path<ContextIdPath>,
    headers: HeaderMap,
) -> Result<Response<Body>, TusError> {
    let upload = state.upload_service.get_upload(&id).await?;

    if upload.context_id.as_deref() != Some(&ctx.id) {
        return Err(TusError::NotFound);
    }
    if upload.status != UploadStatus::Finalized {
        return Err(TusError::NotFound);
    }

    let total = upload.upload_length as u64;

    if total == 0 {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Accept-Ranges", "bytes")
            .header("Content-Length", "0")
            .body(Body::empty())
            .unwrap());
    }

    let range_str = headers.get(header::RANGE).and_then(|v| v.to_str().ok());

    let (offset, length, is_partial) = match parse_range(range_str, total) {
        Ok(None) => (0u64, total, false),
        Ok(Some((off, len))) => (off, len, true),
        Err(()) => {
            return Ok(Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("Content-Range", format!("bytes */{total}"))
                .body(Body::empty())
                .unwrap());
        }
    };

    let body = state
        .upload_service
        .open_for_read(&upload.storage_path, offset, length)
        .await?;

    let end = offset + length - 1;
    let content_type = upload
        .metadata_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<HashMap<String, String>>(json).ok())
        .and_then(|m| m.get("filetype").or(m.get("type")).cloned())
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let mut builder = Response::builder()
        .header("Accept-Ranges", "bytes")
        .header("Content-Length", length.to_string())
        .header("Content-Type", content_type);

    if let Some(name) = &upload.filename {
        let escaped = name.replace('\\', "\\\\").replace('"', "\\\"");
        builder = builder.header(
            "Content-Disposition",
            format!("attachment; filename=\"{escaped}\""),
        );
    }

    if is_partial {
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header("Content-Range", format!("bytes {offset}-{end}/{total}"));
    } else {
        builder = builder.status(StatusCode::OK);
    }

    Ok(builder.body(body).unwrap())
}

/// Parse a `Range: bytes=...` header.
///
/// Returns:
/// - `Ok(None)`              → no Range header or unrecognised unit; serve full file (200)
/// - `Ok(Some((off, len)))` → satisfiable range; serve partial (206)
/// - `Err(())`               → syntactically valid but unsatisfiable; return 416
fn parse_range(header: Option<&str>, total: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = header else { return Ok(None) };
    let Some(spec) = value.strip_prefix("bytes=") else { return Ok(None) };

    // Only first range spec (multi-range → single-range with first spec).
    let spec = spec.split(',').next().unwrap_or("").trim();
    let Some((s, e)) = spec.split_once('-') else { return Ok(None) };

    let (start, end) = if s.trim().is_empty() {
        // Suffix range: bytes=-N  (last N bytes)
        let n: u64 = e.trim().parse().map_err(|_| ())?;
        let start = total.saturating_sub(n);
        (start, total - 1)
    } else {
        let start: u64 = s.trim().parse().map_err(|_| ())?;
        let end: u64 = if e.trim().is_empty() {
            total - 1
        } else {
            e.trim().parse().map_err(|_| ())?
        };
        (start, end)
    };

    if start >= total || start > end {
        return Err(());
    }
    let end = end.min(total - 1);
    Ok(Some((start, end - start + 1)))
}

#[cfg(test)]
mod tests {
    use super::{parse_concat_final_ids, parse_range};

    // ── parse_range ──────────────────────────────────────────────────────────

    #[test]
    fn range_none_returns_full() {
        assert_eq!(parse_range(None, 1000), Ok(None));
    }

    #[test]
    fn range_open_end() {
        assert_eq!(parse_range(Some("bytes=500-"), 1000), Ok(Some((500, 500))));
    }

    #[test]
    fn range_explicit_end() {
        assert_eq!(parse_range(Some("bytes=0-499"), 1000), Ok(Some((0, 500))));
    }

    #[test]
    fn range_suffix() {
        assert_eq!(parse_range(Some("bytes=-200"), 1000), Ok(Some((800, 200))));
    }

    #[test]
    fn range_clamps_end_to_file_size() {
        // end (999) is within bounds; (1500) would be clamped to 999
        assert_eq!(parse_range(Some("bytes=0-1500"), 1000), Ok(Some((0, 1000))));
    }

    #[test]
    fn range_start_beyond_eof_is_416() {
        assert_eq!(parse_range(Some("bytes=1000-1999"), 1000), Err(()));
    }

    #[test]
    fn range_inverted_bounds_is_416() {
        assert_eq!(parse_range(Some("bytes=500-100"), 1000), Err(()));
    }

    #[test]
    fn range_unknown_unit_ignored() {
        assert_eq!(parse_range(Some("chunks=0-100"), 1000), Ok(None));
    }

    // ── parse_concat_final_ids ───────────────────────────────────────────────

    #[test]
    fn concat_final_ids_standard_form() {
        let ids = parse_concat_final_ids("final ;/files/abc /files/def").unwrap();
        assert_eq!(ids, vec!["abc", "def"]);
    }

    #[test]
    fn concat_final_ids_no_space_before_semicolon() {
        let ids = parse_concat_final_ids("final;/files/abc /files/def").unwrap();
        assert_eq!(ids, vec!["abc", "def"]);
    }

    #[test]
    fn concat_final_ids_extra_whitespace() {
        let ids = parse_concat_final_ids("final  ;  /files/abc  /files/def").unwrap();
        assert_eq!(ids, vec!["abc", "def"]);
    }

    #[test]
    fn concat_final_ids_full_urls() {
        let ids = parse_concat_final_ids(
            "final ;http://localhost:3000/files/id1 http://localhost:3000/files/id2",
        )
        .unwrap();
        assert_eq!(ids, vec!["id1", "id2"]);
    }

    #[test]
    fn concat_final_ids_single_partial() {
        let ids = parse_concat_final_ids("final ;/files/only").unwrap();
        assert_eq!(ids, vec!["only"]);
    }

    #[test]
    fn concat_final_ids_returns_none_for_partial() {
        assert!(parse_concat_final_ids("partial").is_none());
    }

    #[test]
    fn concat_final_ids_returns_none_for_unrelated() {
        assert!(parse_concat_final_ids("random value").is_none());
    }
}
