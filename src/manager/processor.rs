use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use tracing::info;

use crate::{app_state::AppState, tus::Upload};
use super::av_scan::AvScanProcessor;
use super::mime_filter::MimeFilterProcessor;

pub struct ProcessorContext {
    pub upload_id: String,
    pub file_path: PathBuf,
    pub upload: Upload,
}

#[async_trait]
pub trait Processor: Send + Sync {
    fn name(&self) -> &str;
    async fn process(&self, ctx: &ProcessorContext) -> anyhow::Result<()>;
}

pub struct NopProcessor;

#[async_trait]
impl Processor for NopProcessor {
    fn name(&self) -> &str {
        "nop"
    }

    async fn process(&self, _ctx: &ProcessorContext) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct ExecProcessor {
    command: String,
    timeout_secs: u64,
}

#[async_trait]
impl Processor for ExecProcessor {
    fn name(&self) -> &str {
        "exec"
    }

    async fn process(&self, ctx: &ProcessorContext) -> anyhow::Result<()> {
        let output = tokio::time::timeout(
            Duration::from_secs(self.timeout_secs),
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&self.command)
                .env("UPLOAD_ID", &ctx.upload_id)
                .env("FILE_PATH", ctx.file_path.to_string_lossy().as_ref())
                .env("FILENAME", ctx.upload.filename.as_deref().unwrap_or(""))
                .env("UPLOAD_SIZE", ctx.upload.upload_length.to_string())
                .env(
                    "METADATA_JSON",
                    ctx.upload.metadata_json.as_deref().unwrap_or("{}"),
                )
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("exec processor timed out after {}s", self.timeout_secs))??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "exec processor exited with {}: {}",
                output.status,
                stderr.trim()
            );
        }
        Ok(())
    }
}

pub struct ProcessorPipeline {
    processors: Vec<Box<dyn Processor>>,
}

impl ProcessorPipeline {
    pub fn from_env() -> anyhow::Result<Self> {
        let names = std::env::var("PROCESSORS").unwrap_or_else(|_| "nop".to_string());
        let mut processors: Vec<Box<dyn Processor>> = Vec::new();

        for name in names.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let p: Box<dyn Processor> = match name {
                "nop" => Box::new(NopProcessor),
                "exec" => {
                    let command = std::env::var("PROCESSOR_EXEC_COMMAND").map_err(|_| {
                        anyhow::anyhow!(
                            "PROCESSOR_EXEC_COMMAND required when 'exec' processor is configured"
                        )
                    })?;
                    let timeout_secs = std::env::var("PROCESSOR_EXEC_TIMEOUT_SECS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(300u64);
                    Box::new(ExecProcessor {
                        command,
                        timeout_secs,
                    })
                }
                "mime" => Box::new(MimeFilterProcessor::from_env()),
                "av" => Box::new(AvScanProcessor::from_env()?),
                other => anyhow::bail!("unknown processor: {other}"),
            };
            processors.push(p);
        }

        Ok(Self { processors })
    }

    pub async fn run(&self, ctx: &ProcessorContext) -> anyhow::Result<()> {
        for p in &self.processors {
            info!(processor = p.name(), upload_id = %ctx.upload_id, "running processor");
            p.process(ctx).await?;
        }
        Ok(())
    }
}

pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;
    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);

    let ctx = ProcessorContext {
        upload_id: upload_id.to_string(),
        file_path,
        upload,
    };

    state.pipeline.run(&ctx).await?;
    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
