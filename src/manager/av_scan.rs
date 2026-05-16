use std::time::Duration;

use async_trait::async_trait;

use super::processor::{Processor, ProcessorContext};

enum ScanMode {
    ClamAv {
        bin: String,
    },
    Http {
        url: String,
        header: Option<(String, String)>,
        max_bytes: u64,
        client: reqwest::Client,
    },
}

pub struct AvScanProcessor {
    mode: ScanMode,
    timeout_secs: u64,
}

impl AvScanProcessor {
    pub fn from_env() -> anyhow::Result<Self> {
        let timeout_secs = std::env::var("AV_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120u64);

        let mode_str = std::env::var("AV_SCANNER").unwrap_or_else(|_| "clamav".to_string());
        let mode = match mode_str.to_lowercase().as_str() {
            "clamav" => ScanMode::ClamAv {
                bin: std::env::var("AV_CLAMAV_BIN")
                    .unwrap_or_else(|_| "clamscan".to_string()),
            },
            "http" => {
                let url = std::env::var("AV_HTTP_URL")
                    .map_err(|_| anyhow::anyhow!("AV_HTTP_URL required when AV_SCANNER=http"))?;
                let header = std::env::var("AV_HTTP_HEADER").ok().and_then(|h| {
                    let (name, value) = h.split_once(':')?;
                    Some((name.trim().to_string(), value.trim().to_string()))
                });
                let max_bytes = std::env::var("AV_HTTP_MAX_BYTES")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(104_857_600u64); // 100 MB
                ScanMode::Http {
                    url,
                    header,
                    max_bytes,
                    client: reqwest::Client::new(),
                }
            }
            other => anyhow::bail!("unknown AV_SCANNER '{other}'; expected 'clamav' or 'http'"),
        };

        Ok(Self { mode, timeout_secs })
    }

    async fn scan_clamav(&self, bin: &str, ctx: &ProcessorContext) -> anyhow::Result<()> {
        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            tokio::process::Command::new(bin)
                .arg("--no-summary")
                .arg(&ctx.file_path)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("AV scan timed out after {}s", self.timeout_secs))??;

        match output.status.code() {
            Some(0) => Ok(()),
            Some(1) => {
                // clamscan exits 1 and prints "<path>: <virus> FOUND" to stdout.
                let stdout = String::from_utf8_lossy(&output.stdout);
                let threat = stdout
                    .lines()
                    .find(|l| l.contains("FOUND"))
                    .map(|l| l.trim())
                    .unwrap_or("unknown threat");
                anyhow::bail!("malware detected: {}", threat)
            }
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("AV scan error (exit {}): {}", output.status, stderr.trim())
            }
        }
    }

    async fn scan_http(
        &self,
        url: &str,
        header: &Option<(String, String)>,
        max_bytes: u64,
        client: &reqwest::Client,
        ctx: &ProcessorContext,
    ) -> anyhow::Result<()> {
        let file_size = tokio::fs::metadata(&ctx.file_path).await?.len();
        if file_size > max_bytes {
            anyhow::bail!(
                "file ({} bytes) exceeds AV_HTTP_MAX_BYTES limit ({}); \
                 use AV_SCANNER=clamav for large files",
                file_size,
                max_bytes,
            );
        }

        let file = tokio::fs::File::open(&ctx.file_path).await?;
        let stream = tokio_util::io::ReaderStream::new(file);

        let mut req = client
            .post(url)
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", file_size.to_string())
            .body(reqwest::Body::wrap_stream(stream));

        if let Some((name, value)) = header {
            req = req.header(name.as_str(), value.as_str());
        }

        let resp = tokio::time::timeout(Duration::from_secs(self.timeout_secs), req.send())
            .await
            .map_err(|_| {
                anyhow::anyhow!("AV HTTP scan timed out after {}s", self.timeout_secs)
            })??;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("AV scan rejected file (HTTP {}): {}", status, body.trim())
        }
    }
}

#[async_trait]
impl Processor for AvScanProcessor {
    fn name(&self) -> &str {
        "av"
    }

    async fn process(&self, ctx: &ProcessorContext) -> anyhow::Result<()> {
        match &self.mode {
            ScanMode::ClamAv { bin } => self.scan_clamav(bin, ctx).await,
            ScanMode::Http {
                url,
                header,
                max_bytes,
                client,
            } => self.scan_http(url, header, *max_bytes, client, ctx).await,
        }
    }
}
