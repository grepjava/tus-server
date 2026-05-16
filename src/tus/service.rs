use std::{collections::HashMap, sync::Arc};

use axum::body::Body;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::{context::ContextCache, metrics::{AppMetrics, ContextLabels}};
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
    upload_expiry_hours: i64,
    metrics: Arc<AppMetrics>,
    quota_max_storage_bytes: i64,
    quota_max_active_uploads: i64,
    context_cache: ContextCache,
}

impl UploadService {
    pub fn new(
        repo: Arc<dyn UploadRepository>,
        storage: Arc<dyn StorageBackend>,
        event_tx: broadcast::Sender<UploadEvent>,
        upload_expiry_hours: i64,
        metrics: Arc<AppMetrics>,
        quota_max_storage_bytes: i64,
        quota_max_active_uploads: i64,
        context_cache: ContextCache,
    ) -> Self {
        Self {
            repo,
            storage,
            locks: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
            upload_expiry_hours,
            metrics,
            quota_max_storage_bytes,
            quota_max_active_uploads,
            context_cache,
        }
    }

    /// Resolve a context UUID to its slug for metric labels. Returns "global" for None.
    async fn context_label(&self, context_id: Option<&str>) -> ContextLabels {
        let context = match context_id {
            None => "global".to_string(),
            Some(id) => self.context_cache.slug_for_id(id).await.unwrap_or_else(|| "global".to_string()),
        };
        ContextLabels { context }
    }

    async fn lock(&self, upload_id: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(upload_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn prune_lock(&self, upload_id: &str) {
        self.locks.lock().await.remove(upload_id);
    }

    async fn emit(&self, upload_id: &str, event_type: &str, message: Option<&str>, context_id: Option<&str>) {
        let _ = self.repo.insert_event(upload_id, event_type, message, context_id).await;
        let event = UploadEvent {
            id: Uuid::new_v4().to_string(),
            upload_id: upload_id.to_string(),
            event_type: event_type.to_string(),
            message: message.map(str::to_string),
            created_at: chrono::Utc::now().to_rfc3339(),
            context_id: context_id.map(str::to_string),
        };
        let _ = self.event_tx.send(event);
    }

    pub async fn create_upload(
        &self,
        upload_length: i64,
        metadata_header: Option<&str>,
        length_is_deferred: bool,
        concat_type: Option<String>,
        context_id: Option<&str>,
    ) -> Result<Upload, TusError> {
        if self.quota_max_active_uploads > 0 {
            let active = self.repo.count_active_uploads().await?;
            if active >= self.quota_max_active_uploads {
                return Err(TusError::QuotaExceeded(format!(
                    "active upload limit reached ({active}/{})",
                    self.quota_max_active_uploads
                )));
            }
        }

        if self.quota_max_storage_bytes > 0 && !length_is_deferred && upload_length > 0 {
            let used = self.repo.count_active_storage_bytes().await?;
            if used + upload_length > self.quota_max_storage_bytes {
                return Err(TusError::QuotaExceeded(format!(
                    "storage quota would be exceeded ({} + {} > {})",
                    used, upload_length, self.quota_max_storage_bytes
                )));
            }
        }

        if upload_length > 0 {
            self.storage.check_staging_capacity(upload_length as u64).await?;
        }

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
                length_is_deferred,
                concat_type,
                concat_uploads: None,
                context_id: context_id.map(str::to_string),
            })
            .await?;

        let lbl = self.context_label(context_id).await;
        self.metrics.uploads_created_total.get_or_create(&lbl).inc();
        self.emit(&id, "created", None, context_id).await;
        Ok(upload)
    }

    pub async fn create_concat_final(
        &self,
        partial_ids: Vec<String>,
        metadata_header: Option<&str>,
        context_id: Option<&str>,
    ) -> Result<Upload, TusError> {
        let id = Uuid::new_v4().to_string();

        let mut partials: Vec<Upload> = Vec::with_capacity(partial_ids.len());
        for pid in &partial_ids {
            let p = self.repo.find(pid).await?.ok_or(TusError::NotFound)?;
            if p.concat_type.as_deref() != Some("partial") {
                return Err(TusError::InvalidHeader(
                    format!("upload {pid} is not a partial upload"),
                ));
            }
            if p.status != UploadStatus::Completed {
                return Err(TusError::InvalidState);
            }
            // Partials must belong to the same context as the final upload.
            // Return NotFound (not Forbidden) to avoid leaking cross-context IDs.
            if p.context_id.as_deref() != context_id {
                return Err(TusError::NotFound);
            }
            partials.push(p);
        }

        let total_length: i64 = partials.iter().map(|p| p.upload_length).sum();

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

        let concat_uploads_json = serde_json::to_string(&partial_ids)
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;

        let storage_path = self.storage.create_empty(&id).await?;

        let source_paths: Vec<String> = partials.iter().map(|p| p.storage_path.clone()).collect();
        self.storage.concat_files(&storage_path, &source_paths).await?;

        let upload = self
            .repo
            .create(NewUpload {
                id: id.clone(),
                filename,
                upload_length: total_length,
                metadata_json,
                storage_path: storage_path.clone(),
                length_is_deferred: false,
                concat_type: Some("final".to_string()),
                concat_uploads: Some(concat_uploads_json),
                context_id: context_id.map(str::to_string),
            })
            .await?;

        if total_length > 0 {
            self.repo.update_offset(&id, 0, total_length).await?;
        }
        self.repo.mark_completed(&id).await?;

        let new_path = self
            .storage
            .finalize(&storage_path, upload.filename.as_deref())
            .await?;
        if new_path != storage_path {
            self.repo.update_storage_path(&id, &new_path).await?;
        }

        let lbl = self.context_label(context_id).await;
        self.metrics.uploads_completed_total.get_or_create(&lbl).inc();
        self.emit(&id, "completed", None, context_id).await;

        // Partials have been consumed; mark them so they don't flow into processing.
        // ConsumedByConcat (not Abandoned) so the dashboard shows the correct reason.
        for pid in &partial_ids {
            let _ = self.repo.mark_status(pid, UploadStatus::ConsumedByConcat, None).await;
            self.prune_lock(pid).await;
        }
        self.prune_lock(&id).await;

        self.repo.find(&id).await?.ok_or(TusError::NotFound)
    }

    pub async fn get_upload(&self, id: &str) -> Result<Upload, TusError> {
        self.repo.find(id).await?.ok_or(TusError::NotFound)
    }

    pub async fn append_chunk(
        &self,
        id: &str,
        client_offset: i64,
        content_length: Option<i64>,
        checksum: Option<(String, Vec<u8>)>,
        new_upload_length: Option<i64>,
        body: Body,
    ) -> Result<i64, TusError> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;

        let mut upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;

        if matches!(upload.status, UploadStatus::Abandoned | UploadStatus::ConsumedByConcat) {
            return Err(TusError::NotFound);
        }

        if upload.is_expired(self.upload_expiry_hours) {
            return Err(TusError::Expired);
        }

        // Deferred length: accept Upload-Length from the client to finalize the size
        if let Some(new_len) = new_upload_length {
            if !upload.length_is_deferred {
                return Err(TusError::InvalidHeader(
                    "Upload-Length may only be set on deferred-length uploads".into(),
                ));
            }
            if new_len < upload.upload_offset {
                return Err(TusError::InvalidHeader(
                    "Upload-Length cannot be less than current offset".into(),
                ));
            }
            self.repo.set_upload_length(id, new_len).await?;
            upload.upload_length = new_len;
            upload.length_is_deferred = false;
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

        if !upload.length_is_deferred {
            if let Some(cl) = content_length {
                if upload.upload_offset + cl > upload.upload_length {
                    return Err(TusError::ExceedsUploadLength);
                }
            }
        }

        let bytes_written = self
            .storage
            .append_stream(&upload.storage_path, body, checksum)
            .await?;
        let ctx_lbl = self.context_label(upload.context_id.as_deref()).await;
        self.metrics.bytes_uploaded_total.get_or_create(&ctx_lbl).inc_by(bytes_written);
        let new_offset = upload.upload_offset + bytes_written as i64;

        if !upload.length_is_deferred && new_offset > upload.upload_length {
            return Err(TusError::ExceedsUploadLength);
        }

        self.repo
            .update_offset(id, upload.upload_offset, new_offset)
            .await?;

        let ctx_id = upload.context_id.as_deref();
        self.emit(
            id,
            "chunk_received",
            Some(&format!("{new_offset}/{}", upload.upload_length)),
            ctx_id,
        )
        .await;

        if !upload.length_is_deferred && new_offset == upload.upload_length {
            self.repo.mark_completed(id).await?;
            let new_path = self
                .storage
                .finalize(&upload.storage_path, upload.filename.as_deref())
                .await?;
            if new_path != upload.storage_path {
                self.repo.update_storage_path(id, &new_path).await?;
            }
            self.metrics.uploads_completed_total.get_or_create(&ctx_lbl).inc();
            self.emit(id, "completed", None, ctx_id).await;
            // Drop the guard before pruning so the map entry is only removed
            // after the mutex is fully released, closing the window where a
            // concurrent request could acquire a fresh lock on the same ID.
            drop(_guard);
            self.prune_lock(id).await;
        }

        Ok(new_offset)
    }

    pub async fn delete_upload(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        let ctx_id = upload.context_id.clone();
        self.storage.delete(&upload.storage_path).await?;
        self.repo.mark_status(id, UploadStatus::Abandoned, None).await?;
        self.emit(id, "deleted", None, ctx_id.as_deref()).await;
        self.prune_lock(id).await;
        Ok(())
    }

    pub async fn list_uploads(&self, limit: i64, offset: i64) -> Result<Vec<Upload>, TusError> {
        self.repo.list(limit, offset).await
    }

    pub async fn list_completed(&self) -> Result<Vec<Upload>, TusError> {
        self.repo.list_completed().await
    }

    pub async fn list_events(&self, upload_id: &str, limit: i64, offset: i64) -> Result<Vec<UploadEvent>, TusError> {
        self.repo.find(upload_id).await?.ok_or(TusError::NotFound)?;
        self.repo.list_events(upload_id, limit, offset).await
    }

    pub async fn retry_processing(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        if !upload.status.can_retry() {
            return Err(TusError::InvalidState);
        }
        let ctx_id = upload.context_id;
        self.repo.mark_status(id, UploadStatus::Completed, None).await?;
        self.emit(id, "retry_queued", None, ctx_id.as_deref()).await;
        Ok(())
    }

    pub async fn mark_abandoned(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        let ctx_id = upload.context_id;
        self.repo.mark_status(id, UploadStatus::Abandoned, None).await?;
        self.emit(id, "abandoned", None, ctx_id.as_deref()).await;
        Ok(())
    }

    pub async fn abandon_and_delete(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        let ctx_id = upload.context_id.clone();
        // Mark DB first so a crash between these two steps leaves the record
        // in a terminal state rather than live-but-missing-file.
        self.repo.mark_status(id, UploadStatus::Abandoned, None).await?;
        let _ = self.storage.delete(&upload.storage_path).await;
        self.emit(id, "abandoned", None, ctx_id.as_deref()).await;
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
        let ctx_id = upload.context_id;
        self.repo.mark_status(id, UploadStatus::Processing, None).await?;
        self.emit(id, "processing_started", None, ctx_id.as_deref()).await;
        Ok(())
    }

    pub async fn complete_processing(&self, id: &str) -> Result<(), TusError> {
        let ctx_id = self.repo.find(id).await?.and_then(|u| u.context_id);
        self.repo.mark_status(id, UploadStatus::Finalized, None).await?;
        self.emit(id, "finalized", None, ctx_id.as_deref()).await;
        Ok(())
    }

    pub async fn fail_processing(&self, id: &str, error: &str) -> Result<(), TusError> {
        let ctx_id = self.repo.find(id).await?.and_then(|u| u.context_id);
        self.repo
            .mark_status(id, UploadStatus::FailedProcessing, Some(error))
            .await?;
        let lbl = self.context_label(ctx_id.as_deref()).await;
        self.metrics.processing_failures_total.get_or_create(&lbl).inc();
        self.emit(id, "processing_failed", Some(error), ctx_id.as_deref()).await;
        Ok(())
    }

    pub async fn hard_delete(&self, id: &str) -> Result<(), TusError> {
        let upload = self.repo.find(id).await?.ok_or(TusError::NotFound)?;
        let _ = self.storage.delete(&upload.storage_path).await;
        self.repo.delete_record(id).await?;
        self.prune_lock(id).await;
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

    pub async fn storage_health(&self) -> Result<(), TusError> {
        self.storage.health().await
    }

    pub async fn open_for_read(
        &self,
        path: &str,
        offset: u64,
        length: u64,
    ) -> Result<Body, TusError> {
        self.storage.open_for_read(path, offset, length).await
    }
}
