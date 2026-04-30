use std::{collections::HashMap, sync::Arc};

use axum::body::Body;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use super::{
    error::TusError,
    metadata,
    model::{NewUpload, Upload, UploadEvent, UploadStatus},
    repository::UploadRepository,
    storage::StorageBackend,
};

pub struct UploadService {
    repo: Arc<dyn UploadRepository>,
    storage: Arc<dyn StorageBackend>,
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    event_tx: broadcast::Sender<UploadEvent>,
}

impl UploadService {
    pub fn new(
        repo: Arc<dyn UploadRepository>,
        storage: Arc<dyn StorageBackend>,
        event_tx: broadcast::Sender<UploadEvent>,
    ) -> Self {
        Self {
            repo,
            storage,
            locks: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    async fn lock(&self, upload_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(upload_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn emit(&self, upload_id: &str, event_type: &str, message: Option<&str>) {
        let _ = self.repo.insert_event(upload_id, event_type, message).await;
        let event = UploadEvent {
            id: Uuid::new_v4().to_string(),
            upload_id: upload_id.to_string(),
            event_type: event_type.to_string(),
            message: message.map(str::to_string),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = self.event_tx.send(event);
    }

    pub async fn create_upload(
        &self,
        upload_length: i64,
        metadata_header: Option<&str>,
    ) -> Result<Upload, TusError> {
        let id = Uuid::new_v4().to_string();

        let (filename, metadata_json) = match metadata_header {
            Some(header) => {
                let parsed = metadata::parse(header).map_err(TusError::InvalidHeader)?;
                let filename = metadata::get_filename(&parsed);
                let json = serde_json::to_string(&parsed)
                    .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
                (filename, Some(json))
            }
            None => (None, None),
        };

        let storage_path = self.storage.create_empty(&id).await?;

        let upload = self
            .repo
            .create(NewUpload {
                id: id.clone(),
                filename,
                upload_length,
                metadata_json,
                storage_path,
            })
            .await?;

        self.emit(&id, "created", None).await;
        Ok(upload)
    }

    pub async fn get_upload(&self, id: &str) -> Result<Upload, TusError> {
        self.repo.find(id).await?.ok_or(TusError::NotFound)
    }

    pub async fn append_chunk(
        &self,
        id: &str,
        client_offset: i64,
        content_length: Option<i64>,
        body: Body,
    ) -> Result<i64, TusError> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;

        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;

        if upload.status == UploadStatus::Abandoned {
            return Err(TusError::NotFound);
        }

        if !upload.status.can_receive_chunk() {
            return Err(TusError::InvalidState);
        }

        if client_offset != upload.upload_offset {
            return Err(TusError::OffsetMismatch {
                expected: upload.upload_offset,
                actual: client_offset,
            });
        }

        if let Some(cl) = content_length {
            if upload.upload_offset + cl > upload.upload_length {
                return Err(TusError::ExceedsUploadLength);
            }
        }

        let bytes_written = self.storage.append_stream(&upload.storage_path, body).await?;
        let new_offset = upload.upload_offset + bytes_written as i64;

        if new_offset > upload.upload_length {
            return Err(TusError::ExceedsUploadLength);
        }

        self.repo
            .update_offset(id, upload.upload_offset, new_offset)
            .await?;

        self.emit(
            id,
            "chunk_received",
            Some(&format!("{new_offset}/{}", upload.upload_length)),
        )
        .await;

        if new_offset == upload.upload_length {
            self.repo.mark_completed(id).await?;
            let new_path = self.storage
                .finalize(&upload.storage_path, upload.filename.as_deref())
                .await?;
            if new_path != upload.storage_path {
                self.repo.update_storage_path(id, &new_path).await?;
            }
            self.emit(id, "completed", None).await;
        }

        Ok(new_offset)
    }

    pub async fn delete_upload(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        self.storage.delete(&upload.storage_path).await?;
        self.repo.mark_status(id, UploadStatus::Abandoned, None).await?;
        self.emit(id, "deleted", None).await;
        Ok(())
    }

    pub async fn list_uploads(&self) -> Result<Vec<Upload>, TusError> {
        self.repo.list().await
    }

    pub async fn list_events(&self, upload_id: &str) -> Result<Vec<UploadEvent>, TusError> {
        self.repo.find(upload_id).await?.ok_or(TusError::NotFound)?;
        self.repo.list_events(upload_id).await
    }

    pub async fn retry_processing(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        if !upload.status.can_retry() {
            return Err(TusError::InvalidState);
        }
        self.repo.mark_status(id, UploadStatus::Completed, None).await?;
        self.emit(id, "retry_queued", None).await;
        Ok(())
    }

    pub async fn mark_abandoned(&self, id: &str) -> Result<(), TusError> {
        self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        self.repo.mark_status(id, UploadStatus::Abandoned, None).await?;
        self.emit(id, "abandoned", None).await;
        Ok(())
    }

    pub async fn find_stale(&self, older_than_hours: i64) -> Result<Vec<Upload>, TusError> {
        self.repo.find_stale(older_than_hours).await
    }

    pub async fn begin_processing(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        if !upload.status.can_process() {
            return Err(TusError::InvalidState);
        }
        self.repo.mark_status(id, UploadStatus::Processing, None).await?;
        self.emit(id, "processing_started", None).await;
        Ok(())
    }

    pub async fn complete_processing(&self, id: &str) -> Result<(), TusError> {
        self.repo.mark_status(id, UploadStatus::Finalized, None).await?;
        self.emit(id, "finalized", None).await;
        Ok(())
    }

    pub async fn fail_processing(&self, id: &str, error: &str) -> Result<(), TusError> {
        self.repo
            .mark_status(id, UploadStatus::FailedProcessing, Some(error))
            .await?;
        self.emit(id, "processing_failed", Some(error)).await;
        Ok(())
    }

    pub async fn hard_delete(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        let _ = self.storage.delete(&upload.storage_path).await;
        self.repo.delete_record(id).await?;
        Ok(())
    }

    pub async fn purge(&self, ids: Vec<String>) -> Result<usize, TusError> {
        let mut deleted = 0;
        for id in &ids {
            if self.hard_delete(id).await.is_ok() {
                deleted += 1;
            }
        }
        Ok(deleted)
    }
}
