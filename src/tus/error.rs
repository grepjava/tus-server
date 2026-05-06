use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TusError {
    #[error("upload not found")]
    NotFound,

    #[error("offset mismatch: expected {expected}, got {actual}")]
    OffsetMismatch { expected: i64, actual: i64 },

    #[error("unsupported TUS version: {0}")]
    UnsupportedVersion(String),

    #[error("missing required header: {0}")]
    MissingHeader(&'static str),

    #[error("invalid header: {0}")]
    InvalidHeader(String),

    #[error("upload is not in a valid state for this operation")]
    InvalidState,

    #[error("chunk would exceed upload length")]
    ExceedsUploadLength,

    #[error("Content-Type must be application/offset+octet-stream")]
    InvalidContentType,

    #[error("upload has expired")]
    Expired,

    #[error("checksum mismatch")]
    ChecksumMismatch,

    #[error("unsupported checksum algorithm: {0}")]
    UnsupportedChecksumAlgorithm(String),

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for TusError {
    fn into_response(self) -> Response {
        let status = match &self {
            TusError::NotFound => StatusCode::NOT_FOUND,
            TusError::OffsetMismatch { .. } => StatusCode::CONFLICT,
            TusError::UnsupportedVersion(_) => StatusCode::PRECONDITION_FAILED,
            TusError::MissingHeader(_) => StatusCode::BAD_REQUEST,
            TusError::InvalidHeader(_) => StatusCode::BAD_REQUEST,
            TusError::InvalidState => StatusCode::FORBIDDEN,
            TusError::ExceedsUploadLength => StatusCode::PAYLOAD_TOO_LARGE,
            TusError::InvalidContentType => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            TusError::Expired => StatusCode::GONE,
            TusError::ChecksumMismatch => StatusCode::from_u16(460).expect("460 is valid"),
            TusError::UnsupportedChecksumAlgorithm(_) => StatusCode::BAD_REQUEST,
            TusError::Storage(_) => StatusCode::INTERNAL_SERVER_ERROR,
            TusError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            TusError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (
            status,
            [("Tus-Resumable", "1.0.0"), ("Tus-Version", "1.0.0")],
            self.to_string(),
        )
            .into_response()
    }
}
