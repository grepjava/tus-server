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
}

#[derive(Debug)]
pub struct NewUpload {
    pub id: String,
    pub filename: Option<String>,
    pub upload_length: i64,
    pub metadata_json: Option<String>,
    pub storage_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UploadEvent {
    pub id: String,
    pub upload_id: String,
    pub event_type: String,
    pub message: Option<String>,
    pub created_at: String,
}
