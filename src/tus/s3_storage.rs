use std::{collections::HashMap, path::{Path, PathBuf}, sync::Arc};

use async_trait::async_trait;
use aws_sdk_s3::{
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};
use axum::body::Body;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, sync::Mutex};

use crate::config::Config;
use super::{
    error::TusError,
    storage::{sanitize_filename, write_body_to_file, StorageBackend},
};

const S3_MIN_PART_BYTES: u64 = 5 * 1024 * 1024;

#[cfg(unix)]
fn staging_free_bytes(path: &std::path::Path) -> u64 {
    use std::ffi::CString;
    let Ok(cpath) = CString::new(path.as_os_str().as_encoded_bytes()) else {
        return u64::MAX;
    };
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(cpath.as_ptr(), &mut stat) } == 0 {
        (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64)
    } else {
        u64::MAX
    }
}

#[cfg(not(unix))]
fn staging_free_bytes(_path: &std::path::Path) -> u64 {
    u64::MAX
}

pub struct S3Storage {
    client: Client,
    bucket: String,
    prefix: String,
    staging_dir: PathBuf,
    multipart_threshold: u64,
    part_size: usize,
    /// Staging path → pre-assembled S3 key; set by server-side concat, consumed by finalize.
    prebuilt_keys: Arc<Mutex<HashMap<String, String>>>,
}

impl S3Storage {
    pub async fn from_config(config: &Config) -> anyhow::Result<Self> {
        let bucket = config
            .s3_bucket
            .clone()
            .ok_or_else(|| anyhow::anyhow!("S3_BUCKET is required when STORAGE_BACKEND=s3"))?;

        let aws_cfg = aws_config::from_env().load().await;

        let mut s3_builder = aws_sdk_s3::config::Builder::from(&aws_cfg);
        if config.s3_force_path_style {
            s3_builder = s3_builder.force_path_style(true);
        }

        let client = Client::from_conf(s3_builder.build());

        let staging_dir = config
            .s3_staging_dir
            .clone()
            .unwrap_or_else(|| config.storage_dir.join("staging"));
        tokio::fs::create_dir_all(&staging_dir).await?;

        Ok(Self {
            client,
            bucket,
            prefix: config.s3_prefix.clone(),
            staging_dir,
            multipart_threshold: config.s3_multipart_threshold,
            part_size: config.s3_part_size,
            prebuilt_keys: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn staging_path(&self, rel: &str) -> PathBuf {
        let stripped = rel.strip_prefix("staging/").unwrap_or(rel);
        self.staging_dir.join(stripped)
    }

    fn s3_key(&self, upload_id: &str, filename: Option<&str>) -> String {
        match filename.map(sanitize_filename).filter(|s| !s.is_empty()) {
            Some(name) => format!("{}{upload_id}/{name}", self.prefix),
            None => format!("{}{upload_id}", self.prefix),
        }
    }

    async fn upload_to_s3(&self, local: &Path, key: &str) -> Result<(), TusError> {
        let file_size = tokio::fs::metadata(local)
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?
            .len();

        if file_size <= self.multipart_threshold {
            let body = ByteStream::from_path(local)
                .await
                .map_err(|e| TusError::Internal(anyhow::anyhow!("ByteStream: {e}")))?;
            self.client
                .put_object()
                .bucket(&self.bucket)
                .key(key)
                .content_length(file_size as i64)
                .body(body)
                .send()
                .await
                .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 PutObject: {e}")))?;
        } else {
            self.multipart_upload(local, key).await?;
        }
        Ok(())
    }

    async fn multipart_upload(&self, local: &Path, key: &str) -> Result<(), TusError> {
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 CreateMultipartUpload: {e}")))?;

        let upload_id = resp
            .upload_id()
            .ok_or_else(|| TusError::Internal(anyhow::anyhow!("S3 returned no multipart upload ID")))?
            .to_string();

        let parts = match self.upload_parts(local, key, &upload_id).await {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                return Err(e);
            }
        };

        if let Err(e) = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
        {
            let _ = self
                .client
                .abort_multipart_upload()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(&upload_id)
                .send()
                .await;
            return Err(TusError::Internal(anyhow::anyhow!(
                "S3 CompleteMultipartUpload: {e}"
            )));
        }
        Ok(())
    }

    async fn upload_parts(
        &self,
        local: &Path,
        key: &str,
        upload_id: &str,
    ) -> Result<Vec<CompletedPart>, TusError> {
        let mut file = tokio::fs::File::open(local)
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
        let mut part_number = 1i32;
        let mut completed = Vec::new();

        loop {
            let mut buf = vec![0u8; self.part_size];
            let n = file
                .read(&mut buf)
                .await
                .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
            if n == 0 {
                break;
            }
            buf.truncate(n);

            let resp = self
                .client
                .upload_part()
                .bucket(&self.bucket)
                .key(key)
                .upload_id(upload_id)
                .part_number(part_number)
                .body(ByteStream::from(buf))
                .send()
                .await
                .map_err(|e| {
                    TusError::Internal(anyhow::anyhow!("S3 UploadPart {part_number}: {e}"))
                })?;

            completed.push(
                CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(resp.e_tag().unwrap_or_default())
                    .build(),
            );
            part_number += 1;
        }

        Ok(completed)
    }

    /// Returns the content-length of an existing S3 object via HeadObject.
    async fn head_object_size(&self, key: &str) -> Result<u64, TusError> {
        let resp = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 HeadObject '{key}': {e}")))?;
        Ok(resp.content_length().unwrap_or(0) as u64)
    }

    /// Assemble `source_keys` into `dest_key` entirely on the S3 side using
    /// CreateMultipartUpload + UploadPartCopy + CompleteMultipartUpload.
    /// Each non-final part must be >= 5 MiB; the caller must pre-verify this.
    async fn copy_parts_to_key(
        &self,
        source_keys: &[String],
        dest_key: &str,
    ) -> Result<(), TusError> {
        let resp = self
            .client
            .create_multipart_upload()
            .bucket(&self.bucket)
            .key(dest_key)
            .send()
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 CreateMultipartUpload: {e}")))?;

        let mpu_id = resp
            .upload_id()
            .ok_or_else(|| TusError::Internal(anyhow::anyhow!("S3 returned no multipart upload ID")))?
            .to_string();

        let result = self.do_copy_parts(source_keys, dest_key, &mpu_id).await;

        let parts = match result {
            Ok(p) => p,
            Err(e) => {
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.bucket)
                    .key(dest_key)
                    .upload_id(&mpu_id)
                    .send()
                    .await;
                return Err(e);
            }
        };

        if let Err(e) = self
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(dest_key)
            .upload_id(&mpu_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
        {
            let _ = self
                .client
                .abort_multipart_upload()
                .bucket(&self.bucket)
                .key(dest_key)
                .upload_id(&mpu_id)
                .send()
                .await;
            return Err(TusError::Internal(anyhow::anyhow!(
                "S3 CompleteMultipartUpload: {e}"
            )));
        }
        Ok(())
    }

    async fn do_copy_parts(
        &self,
        source_keys: &[String],
        dest_key: &str,
        mpu_id: &str,
    ) -> Result<Vec<CompletedPart>, TusError> {
        let mut parts = Vec::with_capacity(source_keys.len());
        for (i, src_key) in source_keys.iter().enumerate() {
            let part_number = (i + 1) as i32;
            let copy_source = format!("{}/{}", self.bucket, src_key);

            let resp = self
                .client
                .upload_part_copy()
                .bucket(&self.bucket)
                .key(dest_key)
                .upload_id(mpu_id)
                .copy_source(&copy_source)
                .part_number(part_number)
                .send()
                .await
                .map_err(|e| {
                    TusError::Internal(anyhow::anyhow!(
                        "S3 UploadPartCopy part {part_number} from '{src_key}': {e}"
                    ))
                })?;

            let e_tag = resp
                .copy_part_result()
                .and_then(|r| r.e_tag())
                .unwrap_or_default()
                .to_string();

            parts.push(
                CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(e_tag)
                    .build(),
            );
        }
        Ok(parts)
    }

    /// Concat using server-side UploadPartCopy. Assembles at a temp S3 key (no filename
    /// yet), records it in `prebuilt_keys` so `finalize()` can rename instead of re-upload.
    async fn concat_server_side(
        &self,
        dest_path: &str,
        source_keys: &[String],
    ) -> Result<u64, TusError> {
        // Get sizes to verify AWS's 5 MiB minimum-part constraint.
        let mut sizes = Vec::with_capacity(source_keys.len());
        let mut total = 0u64;
        for key in source_keys {
            let sz = self.head_object_size(key).await?;
            sizes.push(sz);
            total += sz;
        }

        // All non-final parts must be >= 5 MiB. Last part has no minimum.
        let non_final_ok = sizes[..sizes.len() - 1]
            .iter()
            .all(|&s| s >= S3_MIN_PART_BYTES);
        if !non_final_ok {
            return self.concat_via_download(dest_path, source_keys).await;
        }

        let upload_id = dest_path
            .strip_prefix("staging/")
            .unwrap_or(dest_path)
            .trim_end_matches(".part");
        // Assemble at a temp key (no filename); finalize() will rename if needed.
        let temp_key = self.s3_key(upload_id, None);

        self.copy_parts_to_key(source_keys, &temp_key).await?;

        // Record that finalize() should rename rather than re-upload.
        self.prebuilt_keys
            .lock()
            .await
            .insert(dest_path.to_string(), temp_key);

        Ok(total)
    }

    /// Original concat: download each S3 object to local staging, concatenate on disk.
    /// Used as a fallback when partials are too small for UploadPartCopy.
    async fn concat_via_download(
        &self,
        dest_path: &str,
        source_keys: &[String],
    ) -> Result<u64, TusError> {
        let dest = self.staging_path(dest_path);
        let mut dest_file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&dest)
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;

        let mut total = 0u64;
        for src_key in source_keys {
            let obj = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(src_key)
                .send()
                .await
                .map_err(|e| {
                    TusError::Internal(anyhow::anyhow!("S3 GetObject '{src_key}': {e}"))
                })?;

            let mut reader = obj.body.into_async_read();
            let bytes = tokio::io::copy(&mut reader, &mut dest_file)
                .await
                .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
            total += bytes;
        }
        dest_file.flush().await?;
        Ok(total)
    }
}

#[async_trait]
impl StorageBackend for S3Storage {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError> {
        tokio::fs::create_dir_all(&self.staging_dir)
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
        let rel = format!("staging/{upload_id}.part");
        tokio::fs::File::create(self.staging_dir.join(format!("{upload_id}.part")))
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
        Ok(rel)
    }

    async fn append_stream(
        &self,
        path: &str,
        body: Body,
        checksum: Option<(String, Vec<u8>)>,
    ) -> Result<u64, TusError> {
        write_body_to_file(&self.staging_path(path), body, checksum).await
    }

    async fn finalize(&self, path: &str, filename: Option<&str>) -> Result<String, TusError> {
        let upload_id = path
            .strip_prefix("staging/")
            .unwrap_or(path)
            .trim_end_matches(".part");
        let final_key = self.s3_key(upload_id, filename);

        // Server-side concat path: rename the pre-assembled temp key if needed.
        if let Some(temp_key) = self.prebuilt_keys.lock().await.remove(path) {
            if temp_key != final_key {
                let copy_source = format!("{}/{}", self.bucket, temp_key);
                self.client
                    .copy_object()
                    .bucket(&self.bucket)
                    .copy_source(&copy_source)
                    .key(&final_key)
                    .send()
                    .await
                    .map_err(|e| {
                        TusError::Internal(anyhow::anyhow!("S3 CopyObject rename: {e}"))
                    })?;
                let _ = self
                    .client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(&temp_key)
                    .send()
                    .await;
            }
            let _ = tokio::fs::remove_file(self.staging_path(path)).await;
            return Ok(final_key);
        }

        // Normal path: upload the staging file to S3.
        let staging = self.staging_path(path);
        self.upload_to_s3(&staging, &final_key).await?;
        let _ = tokio::fs::remove_file(&staging).await;
        Ok(final_key)
    }

    async fn delete(&self, path: &str) -> Result<(), TusError> {
        if path.starts_with("staging/") {
            let _ = tokio::fs::remove_file(self.staging_path(path)).await;
        } else {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(path)
                .send()
                .await
                .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 DeleteObject: {e}")))?;
        }
        Ok(())
    }

    async fn concat_files(
        &self,
        dest_path: &str,
        source_paths: &[String],
    ) -> Result<u64, TusError> {
        // If all sources are already in S3 (finalized partials), assemble server-side.
        // Staging paths are partials that haven't been finalized yet — shouldn't happen
        // in practice since service.rs requires Completed status before concat.
        if !source_paths.is_empty() && source_paths.iter().all(|p| !p.starts_with("staging/")) {
            return self.concat_server_side(dest_path, source_paths).await;
        }
        self.concat_via_download(dest_path, source_paths).await
    }

    async fn health(&self) -> Result<(), TusError> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 HeadBucket: {e}")))?;
        Ok(())
    }

    async fn open_for_read(&self, path: &str, offset: u64, length: u64) -> Result<Body, TusError> {
        let end = offset + length - 1;
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .range(format!("bytes={offset}-{end}"))
            .send()
            .await
            .map_err(|e| TusError::Internal(anyhow::anyhow!("S3 GetObject: {e}")))?;
        let stream =
            tokio_util::io::ReaderStream::new(resp.body.into_async_read());
        Ok(Body::from_stream(stream))
    }

    async fn check_staging_capacity(&self, required_bytes: u64) -> Result<(), TusError> {
        let free = staging_free_bytes(&self.staging_dir);
        if free < required_bytes {
            return Err(TusError::QuotaExceeded(format!(
                "staging disk has {} bytes free, need {}",
                free, required_bytes,
            )));
        }
        Ok(())
    }
}
