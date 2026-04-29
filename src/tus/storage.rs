use async_trait::async_trait;
use axum::body::Body;
use futures::StreamExt;
use std::path::PathBuf;
use tokio::{
    fs,
    io::AsyncWriteExt,
};

use super::error::TusError;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError>;
    async fn append_stream(&self, path: &str, body: Body) -> Result<u64, TusError>;
    async fn finalize(&self, path: &str) -> Result<(), TusError>;
    async fn delete(&self, path: &str) -> Result<(), TusError>;
}

pub struct FilesystemStorage {
    base_dir: PathBuf,
}

impl FilesystemStorage {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn full_path(&self, path: &str) -> PathBuf {
        self.base_dir.join(path)
    }
}

#[async_trait]
impl StorageBackend for FilesystemStorage {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError> {
        fs::create_dir_all(&self.base_dir).await?;
        let part_name = format!("{upload_id}.part");
        let full = self.full_path(&part_name);
        fs::File::create(&full).await?;
        Ok(part_name)
    }

    async fn append_stream(&self, path: &str, body: Body) -> Result<u64, TusError> {
        let full = self.full_path(path);

        let file = fs::OpenOptions::new()
            .append(true)
            .open(&full)
            .await?;

        let mut writer = tokio::io::BufWriter::new(file);
        let mut stream = body.into_data_stream();
        let mut bytes_written: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
            writer.write_all(&chunk).await?;
            bytes_written += chunk.len() as u64;
        }

        writer.flush().await?;
        Ok(bytes_written)
    }

    async fn finalize(&self, _path: &str) -> Result<(), TusError> {
        // File is already fully written; rename from .part when processing confirms validity
        Ok(())
    }

    async fn delete(&self, path: &str) -> Result<(), TusError> {
        let full = self.full_path(path);
        if full.exists() {
            fs::remove_file(full).await?;
        }
        Ok(())
    }
}
