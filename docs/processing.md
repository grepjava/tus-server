# Custom Processing

Once all chunks are received for an upload, the server transitions it to `Completed` and the background worker triggers processing. This document explains how to add your own logic.

## The extension point

All processing logic goes in [`src/manager/processor.rs`](../src/manager/processor.rs):

```rust
pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);

    // ← your logic here

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
```

`begin_processing` marks the upload as `Processing`. `complete_processing` marks it `Finalized`. If you return an `Err`, the worker calls `fail_processing` automatically, setting the status to `FailedProcessing` with the error message attached.

The file on disk at `file_path` is the fully assembled upload — all chunks have been written and finalized before `process` is called.

## Accessing metadata

Upload metadata (from the `Upload-Metadata` TUS header) is stored as a JSON object:

```rust
let meta: Option<serde_json::Value> = upload
    .metadata_json
    .as_deref()
    .and_then(|s| serde_json::from_str(s).ok());

if let Some(filename) = meta.and_then(|m| m["filename"].as_str().map(str::to_string)) {
    // use filename
}
```

## Example: validate a file type

```rust
use std::io::Read;

pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);

    // Read magic bytes
    let mut f = std::fs::File::open(&file_path)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;

    if &magic != b"%PDF" {
        anyhow::bail!("not a PDF");
        // worker catches this, calls fail_processing automatically
    }

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
```

## Example: forward to S3

```rust
// Cargo.toml: aws-sdk-s3 = "1"
use aws_sdk_s3::Client;
use tokio::fs::File;
use tokio_util::io::ReaderStream;

pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);
    let key = format!("uploads/{}", upload.id);

    let sdk_config = aws_config::load_from_env().await;
    let client = Client::new(&sdk_config);

    let file = File::open(&file_path).await?;
    let stream = aws_sdk_s3::primitives::ByteStream::read_from()
        .path(&file_path)
        .build()
        .await?;

    client
        .put_object()
        .bucket("my-bucket")
        .key(&key)
        .body(stream)
        .send()
        .await?;

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
```

## Example: call a downstream HTTP API

```rust
pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;

    let client = reqwest::Client::new();
    let res = client
        .post("https://api.example.com/ingest")
        .json(&serde_json::json!({
            "upload_id": upload.id,
            "filename":  upload.filename,
            "size":      upload.upload_length,
        }))
        .send()
        .await?;

    if !res.status().is_success() {
        anyhow::bail!("downstream API returned {}", res.status());
    }

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
```

## Retries

If processing fails, the upload status becomes `FailedProcessing`. You can re-queue it from the dashboard (↺ Retry on the detail page) or via the API:

```bash
curl -X POST http://localhost:3000/api/uploads/{id}/retry-processing
```

This resets the status to `Completed`, which the worker picks up again on its next tick.

## Processing via webhook instead

If you prefer to keep processing logic outside the server binary entirely, skip editing `processor.rs` and configure a webhook for the `completed` event. Your service receives the upload ID, fetches the file, and calls back to mark it done if needed. See the [Webhooks](../README.md#webhooks) section of the README.

The in-process approach (`processor.rs`) is simpler for single-service deployments. The webhook approach is better when processing is CPU-heavy, needs separate scaling, or is owned by a different team.
