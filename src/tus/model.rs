use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UploadStatus {
    Created,
    Uploading,
    Completed,
    Processing,
    Finalized,
    FailedUpload,
    FailedProcessing,
    FailedFinalization,
    Abandoned,
    /// Partial upload whose bytes were merged into a concat-final upload.
    /// Distinct from Abandoned so the dashboard can show the correct reason.
    ConsumedByConcat,
}

impl UploadStatus {
    pub fn can_receive_chunk(&self) -> bool {
        matches!(self, Self::Created | Self::Uploading)
    }

    pub fn can_process(&self) -> bool {
        matches!(self, Self::Completed)
    }

    pub fn can_retry(&self) -> bool {
        matches!(self, Self::FailedProcessing | Self::FailedFinalization)
    }
}

impl std::fmt::Display for UploadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Created => "Created",
            Self::Uploading => "Uploading",
            Self::Completed => "Completed",
            Self::Processing => "Processing",
            Self::Finalized => "Finalized",
            Self::FailedUpload => "FailedUpload",
            Self::FailedProcessing => "FailedProcessing",
            Self::FailedFinalization => "FailedFinalization",
            Self::Abandoned => "Abandoned",
            Self::ConsumedByConcat => "ConsumedByConcat",
        };
        write!(f, "{s}")
    }
}

impl TryFrom<String> for UploadStatus {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "Created" => Ok(Self::Created),
            "Uploading" => Ok(Self::Uploading),
            "Completed" => Ok(Self::Completed),
            "Processing" => Ok(Self::Processing),
            "Finalized" => Ok(Self::Finalized),
            "FailedUpload" => Ok(Self::FailedUpload),
            "FailedProcessing" => Ok(Self::FailedProcessing),
            "FailedFinalization" => Ok(Self::FailedFinalization),
            "Abandoned" => Ok(Self::Abandoned),
            "ConsumedByConcat" => Ok(Self::ConsumedByConcat),
            _ => Err(format!("unknown status: {s}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Upload {
    pub id: String,
    pub filename: Option<String>,
    pub upload_length: i64,
    pub upload_offset: i64,
    pub metadata_json: Option<String>,
    pub status: UploadStatus,
    pub storage_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub length_is_deferred: bool,
    pub concat_type: Option<String>,
    pub concat_uploads: Option<String>,
    pub context_id: Option<String>,
}

impl Upload {
    pub fn is_expired(&self, expiry_hours: i64) -> bool {
        use chrono::{DateTime, Duration, Utc};
        let Ok(dt) = DateTime::parse_from_rfc3339(&self.created_at) else {
            return false;
        };
        Utc::now() > Into::<DateTime<Utc>>::into(dt) + Duration::hours(expiry_hours)
    }
}

#[derive(Debug)]
pub struct NewUpload {
    pub id: String,
    pub filename: Option<String>,
    pub upload_length: i64,
    pub metadata_json: Option<String>,
    pub storage_path: String,
    pub length_is_deferred: bool,
    pub concat_type: Option<String>,
    pub concat_uploads: Option<String>,
    pub context_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upload_with_created_at(created_at: &str) -> Upload {
        Upload {
            id: "test".into(),
            filename: None,
            upload_length: 100,
            upload_offset: 0,
            metadata_json: None,
            status: UploadStatus::Created,
            storage_path: "test.part".into(),
            created_at: created_at.to_string(),
            updated_at: created_at.to_string(),
            completed_at: None,
            error_message: None,
            length_is_deferred: false,
            concat_type: None,
            concat_uploads: None,
            context_id: None,
        }
    }

    #[test]
    fn is_expired_true_for_past_date() {
        let u = upload_with_created_at("2000-01-01T00:00:00+00:00");
        assert!(u.is_expired(24));
        assert!(u.is_expired(0));
    }

    #[test]
    fn is_expired_false_for_far_future() {
        let u = upload_with_created_at("9999-12-31T23:59:59+00:00");
        assert!(!u.is_expired(24));
    }

    #[test]
    fn is_expired_false_for_unparseable_date() {
        let u = upload_with_created_at("not-a-date");
        assert!(!u.is_expired(24));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UploadEvent {
    pub id: String,
    pub upload_id: String,
    pub event_type: String,
    pub message: Option<String>,
    pub created_at: String,
    pub context_id: Option<String>,
}
