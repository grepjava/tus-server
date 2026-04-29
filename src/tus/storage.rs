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
    /// Renames the .part file to its final name. Returns the new relative path.
    async fn finalize(&self, path: &str, filename: Option<&str>) -> Result<String, TusError>;
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

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect::<String>()
        .trim()
        .to_string()
}

#[async_trait]
impl StorageBackend for FilesystemStorage {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError> {
        fs::create_dir_all(&self.base_dir).await?;
        let part_name = format!("{upload_id}.part");
        fs::File::create(self.full_path(&part_name)).await?;
        Ok(part_name)
    }

    async fn append_stream(&self, path: &str, body: Body) -> Result<u64, TusError> {
        let file = fs::OpenOptions::new()
            .append(true)
            .open(self.full_path(path))
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

    async fn finalize(&self, path: &str, filename: Option<&str>) -> Result<String, TusError> {
        let src = self.full_path(path);

        // Derive the upload ID from the .part filename
        let upload_id = path.trim_end_matches(".part");

        let (new_relative, dst) = match filename.map(sanitize_filename).filter(|s| !s.is_empty()) {
            Some(name) => {
                // Place in a subdirectory named after the upload ID
                let dir = self.base_dir.join(upload_id);
                fs::create_dir_all(&dir).await?;
                let rel = format!("{upload_id}/{name}");
                (rel.clone(), self.base_dir.join(rel))
            }
            None => {
                // No filename — just drop the .part extension
                let rel = upload_id.to_string();
                (rel.clone(), self.base_dir.join(rel))
            }
        };

        fs::rename(&src, &dst).await?;
        Ok(new_relative)
    }

    async fn delete(&self, path: &str) -> Result<(), TusError> {
        let full = self.full_path(path);
        if full.is_file() {
            fs::remove_file(&full).await?;
            // Remove the parent upload directory if it is now empty and is not the base dir
            if let Some(parent) = full.parent() {
                if parent != self.base_dir {
                    let _ = fs::remove_dir(parent).await;
                }
            }
        } else if full.is_dir() {
            fs::remove_dir_all(&full).await?;
        }
        Ok(())
    }
}
