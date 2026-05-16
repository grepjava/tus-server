use std::path::Path;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;

use super::processor::{Processor, ProcessorContext};

fn parse_csv_lower(var: &str) -> Vec<String> {
    std::env::var(var)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub struct MimeFilterProcessor {
    allowed_mime: Vec<String>,
    denied_mime: Vec<String>,
    allowed_ext: Vec<String>,
    denied_ext: Vec<String>,
}

impl MimeFilterProcessor {
    pub fn from_env() -> Self {
        Self {
            allowed_mime: parse_csv_lower("MIME_ALLOW"),
            denied_mime: parse_csv_lower("MIME_DENY"),
            allowed_ext: parse_csv_lower("EXT_ALLOW"),
            denied_ext: parse_csv_lower("EXT_DENY"),
        }
    }
}

#[async_trait]
impl Processor for MimeFilterProcessor {
    fn name(&self) -> &str {
        "mime"
    }

    async fn process(&self, ctx: &ProcessorContext) -> anyhow::Result<()> {
        // Extension check first — no I/O required.
        if !self.allowed_ext.is_empty() || !self.denied_ext.is_empty() {
            match ctx.upload.filename.as_deref() {
                Some(filename) => {
                    let ext = Path::new(filename)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    if self.denied_ext.contains(&ext) {
                        anyhow::bail!("file extension '.{}' is not allowed", ext);
                    }
                    if !self.allowed_ext.is_empty() && !self.allowed_ext.contains(&ext) {
                        anyhow::bail!("file extension '.{}' is not permitted", ext);
                    }
                }
                None if !self.allowed_ext.is_empty() => {
                    // Allow-list configured but upload has no filename — reject to be safe.
                    anyhow::bail!(
                        "upload has no filename; extension allow-list requires a filename"
                    );
                }
                None => {}
            }
        }

        // MIME check via magic bytes.
        if !self.allowed_mime.is_empty() || !self.denied_mime.is_empty() {
            let mut file = tokio::fs::File::open(&ctx.file_path).await?;
            let mut header = vec![0u8; 8192];
            let n = file.read(&mut header).await?;
            header.truncate(n);

            match infer::get(&header).map(|t| t.mime_type().to_string()) {
                Some(mime) => {
                    if self.denied_mime.contains(&mime) {
                        anyhow::bail!("MIME type '{}' is not allowed", mime);
                    }
                    if !self.allowed_mime.is_empty() && !self.allowed_mime.contains(&mime) {
                        anyhow::bail!("MIME type '{}' is not permitted", mime);
                    }
                }
                None if !self.allowed_mime.is_empty() => {
                    // Unknown type with an allow-list configured — reject.
                    anyhow::bail!(
                        "file type could not be determined; only specific MIME types are permitted"
                    );
                }
                None => {
                    // Unknown type with only a deny-list — allow through (nothing matched).
                }
            }
        }

        Ok(())
    }
}
