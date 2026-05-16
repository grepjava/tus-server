use async_trait::async_trait;
use axum::body::Body;
use futures::StreamExt;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};

use super::error::TusError;

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError>;
    async fn append_stream(
        &self,
        path: &str,
        body: Body,
        checksum: Option<(String, Vec<u8>)>,
    ) -> Result<u64, TusError>;
    async fn finalize(&self, path: &str, filename: Option<&str>) -> Result<String, TusError>;
    async fn delete(&self, path: &str) -> Result<(), TusError>;
    async fn concat_files(&self, dest_path: &str, source_paths: &[String]) -> Result<u64, TusError>;
    async fn health(&self) -> Result<(), TusError>;
    /// Called before accepting a new upload. Implementations that stage data on local
    /// disk should verify at least `required_bytes` are available. Default is a no-op.
    async fn check_staging_capacity(&self, _required_bytes: u64) -> Result<(), TusError> {
        Ok(())
    }
    /// Stream `length` bytes from `path` beginning at `offset`.
    async fn open_for_read(&self, path: &str, offset: u64, length: u64) -> Result<Body, TusError>;
}

/// Streams `body` into `path`, computing and verifying `checksum` if provided.
/// On checksum failure the file is truncated back to its pre-call length.
pub(crate) async fn write_body_to_file(
    path: &std::path::Path,
    body: Body,
    checksum: Option<(String, Vec<u8>)>,
) -> Result<u64, TusError> {
    let original_len = fs::metadata(path).await.map(|m| m.len()).unwrap_or(0);

    let mut hasher = checksum
        .as_ref()
        .map(|(alg, _)| ChecksumHasher::from_algorithm(alg))
        .transpose()?;

    let file = fs::OpenOptions::new().append(true).open(path).await?;
    let mut writer = tokio::io::BufWriter::new(file);
    let mut stream = body.into_data_stream();
    let mut bytes_written: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| TusError::Internal(anyhow::anyhow!(e)))?;
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        writer.write_all(&chunk).await?;
        bytes_written += chunk.len() as u64;
    }
    writer.flush().await?;

    if let Some((_, expected)) = checksum {
        let computed = hasher.unwrap().finalize();
        if computed != expected {
            if let Ok(f) = fs::OpenOptions::new().write(true).open(path).await {
                let _ = f.set_len(original_len).await;
            }
            return Err(TusError::ChecksumMismatch);
        }
    }

    Ok(bytes_written)
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

pub(crate) fn sanitize_filename(name: &str) -> String {
    // Allowlist: letters, digits, spaces, dots, underscores, hyphens
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-'))
        .collect();

    let trimmed = sanitized.trim();

    // Reject path traversal components
    if trimmed.split('.').all(|s| s.is_empty()) {
        return String::new(); // ".", "..", "...", etc.
    }

    trimmed.to_string()
}

enum ChecksumHasher {
    Sha1(Sha1),
    Sha256(Sha256),
}

impl ChecksumHasher {
    fn from_algorithm(alg: &str) -> Result<Self, TusError> {
        match alg {
            "sha1" => Ok(Self::Sha1(Sha1::new())),
            "sha256" => Ok(Self::Sha256(Sha256::new())),
            _ => Err(TusError::UnsupportedChecksumAlgorithm(alg.to_string())),
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(h) => Digest::update(h, data),
            Self::Sha256(h) => Digest::update(h, data),
        }
    }

    fn finalize(self) -> Vec<u8> {
        match self {
            Self::Sha1(h) => Digest::finalize(h).to_vec(),
            Self::Sha256(h) => Digest::finalize(h).to_vec(),
        }
    }
}

#[async_trait]
impl StorageBackend for FilesystemStorage {
    async fn create_empty(&self, upload_id: &str) -> Result<String, TusError> {
        fs::create_dir_all(&self.base_dir).await?;
        let part_name = format!("{upload_id}.part");
        fs::File::create(self.full_path(&part_name)).await?;
        Ok(part_name)
    }

    async fn append_stream(
        &self,
        path: &str,
        body: Body,
        checksum: Option<(String, Vec<u8>)>,
    ) -> Result<u64, TusError> {
        write_body_to_file(&self.full_path(path), body, checksum).await
    }

    async fn finalize(&self, path: &str, filename: Option<&str>) -> Result<String, TusError> {
        let src = self.full_path(path);
        let upload_id = path.trim_end_matches(".part");

        let (new_relative, dst) = match filename.map(sanitize_filename).filter(|s| !s.is_empty()) {
            Some(name) => {
                let dir = self.base_dir.join(upload_id);
                fs::create_dir_all(&dir).await?;
                let rel = format!("{upload_id}/{name}");
                (rel.clone(), self.base_dir.join(rel))
            }
            None => {
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

    async fn concat_files(&self, dest_path: &str, source_paths: &[String]) -> Result<u64, TusError> {
        let dest = self.full_path(dest_path);
        let mut dest_file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&dest)
            .await?;

        let mut total = 0u64;
        for src_path in source_paths {
            let src = self.full_path(src_path);
            let mut src_file = fs::File::open(&src).await?;
            total += tokio::io::copy(&mut src_file, &mut dest_file).await?;
        }

        dest_file.flush().await?;
        Ok(total)
    }

    async fn health(&self) -> Result<(), TusError> {
        fs::create_dir_all(&self.base_dir).await?;
        let probe = self.base_dir.join(".health_probe");
        fs::write(&probe, b"").await?;
        fs::remove_file(&probe).await?;
        Ok(())
    }

    async fn open_for_read(&self, path: &str, offset: u64, length: u64) -> Result<Body, TusError> {
        let full = self.full_path(path);
        let mut file = fs::File::open(&full).await?;
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset)).await?;
        }
        let stream = tokio_util::io::ReaderStream::new(file.take(length));
        Ok(Body::from_stream(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::{FilesystemStorage, StorageBackend};
    use axum::body::Body;
    use sha2::{Digest, Sha256};
    use crate::tus::TusError;

    #[test]
    fn sanitize_filename_allows_safe_names() {
        assert_eq!(super::sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(super::sanitize_filename("my file 2024.tar.gz"), "my file 2024.tar.gz");
        assert_eq!(super::sanitize_filename("file_v1-final.txt"), "file_v1-final.txt");
    }

    #[test]
    fn sanitize_filename_strips_dangerous_chars() {
        assert_eq!(super::sanitize_filename("../../etc/passwd"), "....etcpasswd");
        assert_eq!(super::sanitize_filename("file\0name"), "filename");
        assert_eq!(super::sanitize_filename("a/b/c"), "abc");
        assert_eq!(super::sanitize_filename("a\\b"), "ab");
    }

    #[test]
    fn sanitize_filename_rejects_dot_only_names() {
        assert_eq!(super::sanitize_filename("."), "");
        assert_eq!(super::sanitize_filename(".."), "");
        assert_eq!(super::sanitize_filename("..."), "");
    }

    #[test]
    fn sanitize_filename_trims_whitespace() {
        assert_eq!(super::sanitize_filename("  file.txt  "), "file.txt");
        assert_eq!(super::sanitize_filename("   "), "");
    }

    #[tokio::test]
    async fn checksum_mismatch_rolls_back_written_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path().to_path_buf());

        let path = storage.create_empty("upload-1").await.unwrap();

        // Write initial 5 bytes without checksum
        storage
            .append_stream(&path, Body::from(b"hello".to_vec()), None)
            .await
            .unwrap();

        let full = dir.path().join(&path);
        assert_eq!(tokio::fs::metadata(&full).await.unwrap().len(), 5);

        // Write 5 more bytes with a wrong checksum
        let wrong = Some(("sha256".to_string(), vec![0xffu8; 32]));
        let err = storage
            .append_stream(&path, Body::from(b"world".to_vec()), wrong)
            .await
            .unwrap_err();

        assert!(matches!(err, TusError::ChecksumMismatch));
        // File must be back to exactly 5 bytes
        assert_eq!(tokio::fs::metadata(&full).await.unwrap().len(), 5);
    }

    #[tokio::test]
    async fn correct_sha256_checksum_allows_write() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path().to_path_buf());

        let path = storage.create_empty("upload-2").await.unwrap();
        let data = b"hello world";
        let hash = Sha256::digest(data).to_vec();

        let n = storage
            .append_stream(&path, Body::from(data.to_vec()), Some(("sha256".to_string(), hash)))
            .await
            .unwrap();

        assert_eq!(n, 11);
        let full = dir.path().join(&path);
        assert_eq!(tokio::fs::metadata(&full).await.unwrap().len(), 11);
    }

    #[tokio::test]
    async fn unsupported_algorithm_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let storage = FilesystemStorage::new(dir.path().to_path_buf());

        let path = storage.create_empty("upload-3").await.unwrap();
        let err = storage
            .append_stream(
                &path,
                Body::from(b"data".to_vec()),
                Some(("md5".to_string(), vec![0u8; 16])),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, TusError::UnsupportedChecksumAlgorithm(_)));
    }
}
