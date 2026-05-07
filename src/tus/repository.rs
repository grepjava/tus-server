use async_trait::async_trait;
use chrono::Utc;
use sqlx::{FromRow, SqlitePool};
use uuid::Uuid;

use super::error::TusError;
use super::model::{NewUpload, Upload, UploadEvent, UploadStatus};

#[async_trait]
pub trait UploadRepository: Send + Sync {
    async fn create(&self, upload: NewUpload) -> Result<Upload, TusError>;
    async fn find(&self, id: &str) -> Result<Option<Upload>, TusError>;
    async fn update_offset(&self, id: &str, old_offset: i64, new_offset: i64) -> Result<(), TusError>;
    async fn mark_completed(&self, id: &str) -> Result<(), TusError>;
    async fn mark_status(&self, id: &str, status: UploadStatus, error: Option<&str>) -> Result<(), TusError>;
    async fn set_upload_length(&self, id: &str, length: i64) -> Result<(), TusError>;
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Upload>, TusError>;
    async fn list_completed(&self) -> Result<Vec<Upload>, TusError>;
    async fn find_stale(&self, older_than_hours: i64) -> Result<Vec<Upload>, TusError>;
    async fn insert_event(&self, upload_id: &str, event_type: &str, message: Option<&str>) -> Result<(), TusError>;
    async fn list_events(&self, upload_id: &str, limit: i64, offset: i64) -> Result<Vec<UploadEvent>, TusError>;
    async fn update_storage_path(&self, id: &str, path: &str) -> Result<(), TusError>;
    async fn delete_record(&self, id: &str) -> Result<(), TusError>;
}

#[derive(FromRow)]
struct UploadRow {
    id: String,
    filename: Option<String>,
    upload_length: i64,
    upload_offset: i64,
    metadata_json: Option<String>,
    status: String,
    storage_path: String,
    created_at: String,
    updated_at: String,
    completed_at: Option<String>,
    error_message: Option<String>,
    length_is_deferred: i64,
    concat_type: Option<String>,
    concat_uploads: Option<String>,
}

fn row_to_upload(row: UploadRow) -> Result<Upload, TusError> {
    let status = UploadStatus::try_from(row.status)
        .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
    Ok(Upload {
        id: row.id,
        filename: row.filename,
        upload_length: row.upload_length,
        upload_offset: row.upload_offset,
        metadata_json: row.metadata_json,
        status,
        storage_path: row.storage_path,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
        error_message: row.error_message,
        length_is_deferred: row.length_is_deferred != 0,
        concat_type: row.concat_type,
        concat_uploads: row.concat_uploads,
    })
}

pub struct SqliteUploadRepository {
    pool: SqlitePool,
}

impl SqliteUploadRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

const COLS: &str =
    "id, filename, upload_length, upload_offset, metadata_json, status, storage_path, \
     created_at, updated_at, completed_at, error_message, length_is_deferred, concat_type, concat_uploads";

#[async_trait]
impl UploadRepository for SqliteUploadRepository {
    async fn create(&self, upload: NewUpload) -> Result<Upload, TusError> {
        let now = Utc::now().to_rfc3339();
        let status = UploadStatus::Created.to_string();

        sqlx::query(
            "INSERT INTO uploads \
             (id, filename, upload_length, upload_offset, metadata_json, status, storage_path, \
              created_at, updated_at, length_is_deferred, concat_type, concat_uploads) \
             VALUES (?, ?, ?, 0, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&upload.id)
        .bind(&upload.filename)
        .bind(upload.upload_length)
        .bind(&upload.metadata_json)
        .bind(&status)
        .bind(&upload.storage_path)
        .bind(&now)
        .bind(&now)
        .bind(upload.length_is_deferred as i64)
        .bind(&upload.concat_type)
        .bind(&upload.concat_uploads)
        .execute(&self.pool)
        .await?;

        self.find(&upload.id).await?.ok_or(TusError::NotFound)
    }

    async fn find(&self, id: &str) -> Result<Option<Upload>, TusError> {
        let row = sqlx::query_as::<_, UploadRow>(
            &format!("SELECT {COLS} FROM uploads WHERE id = ?"),
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(row_to_upload).transpose()
    }

    async fn update_offset(&self, id: &str, old_offset: i64, new_offset: i64) -> Result<(), TusError> {
        let now = Utc::now().to_rfc3339();
        let status = UploadStatus::Uploading.to_string();

        let affected = sqlx::query(
            "UPDATE uploads SET upload_offset = ?, status = ?, updated_at = ? \
             WHERE id = ? AND upload_offset = ?",
        )
        .bind(new_offset)
        .bind(&status)
        .bind(&now)
        .bind(id)
        .bind(old_offset)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if affected == 0 {
            return Err(TusError::OffsetMismatch {
                expected: old_offset,
                actual: old_offset,
            });
        }

        Ok(())
    }

    async fn mark_completed(&self, id: &str) -> Result<(), TusError> {
        let now = Utc::now().to_rfc3339();
        let status = UploadStatus::Completed.to_string();

        sqlx::query(
            "UPDATE uploads SET status = ?, updated_at = ?, completed_at = ? WHERE id = ?",
        )
        .bind(&status)
        .bind(&now)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn mark_status(&self, id: &str, status: UploadStatus, error: Option<&str>) -> Result<(), TusError> {
        let now = Utc::now().to_rfc3339();
        let status_str = status.to_string();

        sqlx::query(
            "UPDATE uploads SET status = ?, error_message = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&status_str)
        .bind(error)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn set_upload_length(&self, id: &str, length: i64) -> Result<(), TusError> {
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "UPDATE uploads SET upload_length = ?, length_is_deferred = 0, updated_at = ? WHERE id = ?",
        )
        .bind(length)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<Upload>, TusError> {
        let rows = sqlx::query_as::<_, UploadRow>(
            &format!("SELECT {COLS} FROM uploads ORDER BY created_at DESC LIMIT ? OFFSET ?"),
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_upload).collect()
    }

    async fn list_completed(&self) -> Result<Vec<Upload>, TusError> {
        let rows = sqlx::query_as::<_, UploadRow>(
            &format!("SELECT {COLS} FROM uploads WHERE status = 'Completed'"),
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_upload).collect()
    }

    async fn find_stale(&self, older_than_hours: i64) -> Result<Vec<Upload>, TusError> {
        let threshold = format!("-{older_than_hours} hours");

        let rows = sqlx::query_as::<_, UploadRow>(
            &format!(
                "SELECT {COLS} FROM uploads \
                 WHERE status IN ('Created', 'Uploading') \
                 AND datetime(updated_at) < datetime('now', ?)"
            ),
        )
        .bind(&threshold)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(row_to_upload).collect()
    }

    async fn insert_event(&self, upload_id: &str, event_type: &str, message: Option<&str>) -> Result<(), TusError> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO upload_events (id, upload_id, event_type, message, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(upload_id)
        .bind(event_type)
        .bind(message)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn list_events(&self, upload_id: &str, limit: i64, offset: i64) -> Result<Vec<UploadEvent>, TusError> {
        let events = sqlx::query_as::<_, UploadEvent>(
            "SELECT id, upload_id, event_type, message, created_at \
             FROM upload_events WHERE upload_id = ? ORDER BY created_at ASC LIMIT ? OFFSET ?",
        )
        .bind(upload_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(events)
    }

    async fn update_storage_path(&self, id: &str, path: &str) -> Result<(), TusError> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE uploads SET storage_path = ?, updated_at = ? WHERE id = ?")
            .bind(path)
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_record(&self, id: &str) -> Result<(), TusError> {
        sqlx::query("DELETE FROM upload_events WHERE upload_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM uploads WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
