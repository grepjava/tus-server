# tus-server

A [TUS 1.0.0](https://tus.io/protocols/resumable-upload) resumable upload server written in Rust, with a built-in management dashboard and webhook support.

## Features

- **Full TUS 1.0.0 protocol** — OPTIONS, POST, HEAD, PATCH, DELETE with `creation` and `termination` extensions
- **SQLite state storage** — zero-dependency database, migrates automatically on startup
- **Filesystem storage** — streams chunks directly to disk without buffering in memory
- **Upload lifecycle** — Created → Uploading → Completed → Processing → Finalized (or Failed/Abandoned)
- **Background workers** — auto-processes completed uploads; cleans up stale ones on a configurable interval
- **Management dashboard** — Svelte SPA served from the same process; stats, search, filtering, bulk operations, live event log via SSE
- **Webhooks** — HTTP callbacks on any lifecycle event, configurable per-endpoint, delivery log with automatic retries
- **Test upload panel** — drag-and-drop TUS client built into the dashboard with configurable chunk size
- **Trait-based design** — `UploadRepository` and `StorageBackend` are traits; swap in PostgreSQL or S3 without touching business logic

## Quick start

### Prerequisites

- Rust 1.75+ (`rustup` recommended)
- Node.js 18+ and npm (for the dashboard UI)

### Build

```bash
# Build the dashboard UI first
cd dashboard-ui
npm install
npm run build
cd ..

# Build the server
cargo build --release
```

### Run

```bash
./start.sh
```

The server starts on `http://localhost:3000` by default. Open that URL to reach the dashboard.

To stop:

```bash
./stop.sh
```

### Without the scripts

```bash
export DATABASE_URL=tus.db
export STORAGE_DIR=uploads
export BASE_URL=http://localhost:3000
export BIND_ADDR=0.0.0.0:3000
export RUST_LOG=info

mkdir -p uploads
./target/release/tus-server
```

## Configuration

All configuration is via environment variables. Copy `.env.example` to `.env` and edit as needed — the server loads it automatically on startup.

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | `tus.db` | SQLite database file path |
| `STORAGE_DIR` | `uploads` | Directory where uploaded files are stored |
| `BASE_URL` | `http://localhost:3000` | Public base URL — used in `Location` headers returned to TUS clients |
| `BIND_ADDR` | `0.0.0.0:3000` | Address and port to listen on |
| `MAX_UPLOAD_BYTES` | `107374182400` (100 GB) | Maximum allowed `Upload-Length` per upload |
| `ABANDONED_AFTER_HOURS` | `24` | Mark uploads with no activity after this many hours as abandoned |
| `CLEANUP_INTERVAL_SECS` | `3600` | How often the cleanup worker runs |
| `RUST_LOG` | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

## TUS protocol

### Base URL

All TUS endpoints are mounted at `/files`.

### Endpoints

| Method | Path | Description |
|---|---|---|
| `OPTIONS` | `/files` | Returns server capabilities |
| `POST` | `/files` | Create a new upload, returns `Location` header |
| `HEAD` | `/files/:id` | Get current offset and length |
| `PATCH` | `/files/:id` | Upload a chunk |
| `DELETE` | `/files/:id` | Terminate an upload |

### Required headers

**POST (create):**
```
Tus-Resumable: 1.0.0
Upload-Length: <total bytes>
Upload-Metadata: filename <base64-encoded-name>   # optional
```

**PATCH (chunk):**
```
Tus-Resumable: 1.0.0
Content-Type: application/offset+octet-stream
Upload-Offset: <current offset>
Content-Length: <chunk size>
```

### Example: upload a file with curl

```bash
FILE=myfile.bin
SIZE=$(wc -c < "$FILE")
NAME=$(echo -n "$FILE" | base64)

# 1. Create
LOCATION=$(curl -si -X POST http://localhost:3000/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: $SIZE" \
  -H "Upload-Metadata: filename $NAME" \
  | grep -i location | tr -d '\r' | awk '{print $2}')

# 2. Upload (single chunk for small files)
curl -X PATCH "$LOCATION" \
  -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" \
  -H "Content-Length: $SIZE" \
  --data-binary @"$FILE"
```

## Dashboard

The management dashboard is a SvelteKit SPA served as static files from `dashboard-ui/build/`. It is served automatically by the same Axum process — no separate web server needed.

### Pages

| Path | Description |
|---|---|
| `/` | Upload list — stats, search, filtering, bulk actions |
| `/uploads/:id` | Upload detail — metadata, progress, live event log |
| `/webhooks` | Webhook management — add, edit, disable, delivery log |

### Test upload

The uploads page has a collapsible **Test Upload** panel. Select a file (or drag and drop), choose a chunk size, and click **Start upload**. The panel uses the TUS protocol directly from the browser so you can verify end-to-end behaviour including chunked transfers.

## Webhooks

The server sends an HTTP POST to any configured URL when an upload lifecycle event occurs.

### Configuring a webhook

**Via the dashboard:** go to `/webhooks` → **+ Add webhook**.

**Via the API:**
```bash
curl -X POST http://localhost:3000/api/webhooks \
  -H 'Content-Type: application/json' \
  -d '{
    "name": "My service",
    "url": "https://your-service.example.com/hooks/tus",
    "secret": "optional-shared-secret",
    "events": ["completed", "finalized", "processing_failed"]
  }'
```

### Payload

```json
{
  "event_type": "finalized",
  "upload_id": "a3f2c1d0-...",
  "event_id": "b9e1...",
  "message": null,
  "timestamp": "2026-04-29T21:14:03Z"
}
```

### Event types

| Event | When |
|---|---|
| `created` | Upload record created (POST /files received) |
| `chunk_received` | A PATCH chunk was written |
| `completed` | All bytes received (offset == length) |
| `processing_started` | Background processor picked up the upload |
| `finalized` | Processing completed successfully |
| `processing_failed` | Processing returned an error |
| `abandoned` | Upload was marked abandoned (stale cleanup or manual) |
| `deleted` | Upload was deleted via the TUS DELETE endpoint |
| `retry_queued` | A failed upload was manually queued for retry |

### Secret verification

If a secret is set, it is sent as `X-Webhook-Secret: <secret>`. Verify it on the receiver:

```js
app.post('/hooks/tus', (req, res) => {
  if (req.headers['x-webhook-secret'] !== process.env.WEBHOOK_SECRET) {
    return res.sendStatus(401);
  }
  // handle event ...
  res.sendStatus(200);
});
```

### Retries

Failed deliveries (non-2xx or network error) are retried up to 3 times with backoff (1 s, then 4 s). The final outcome — HTTP status, response body, error message, attempt count — is stored in `webhook_deliveries` and visible in the dashboard.

## Management API

All endpoints are under `/api`.

### Uploads

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/uploads` | List all uploads |
| `GET` | `/api/uploads/:id` | Get a single upload |
| `DELETE` | `/api/uploads/:id` | Hard-delete upload and file |
| `POST` | `/api/uploads/purge` | Bulk hard-delete `{ "ids": ["..."] }` |
| `GET` | `/api/uploads/:id/events` | List lifecycle events |
| `GET` | `/api/uploads/:id/stream` | SSE stream of live events |
| `POST` | `/api/uploads/:id/retry-processing` | Re-queue a failed upload |
| `POST` | `/api/uploads/:id/mark-abandoned` | Manually abandon an upload |

### Webhooks

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/webhooks` | List configured webhooks |
| `POST` | `/api/webhooks` | Create a webhook |
| `PUT` | `/api/webhooks/:id` | Update a webhook |
| `DELETE` | `/api/webhooks/:id` | Delete a webhook |
| `GET` | `/api/webhooks/:id/deliveries` | List recent deliveries (last 100) |

### Health

```
GET /api/health  →  { "status": "ok" }
```

## Custom processing

When all chunks are received, the upload transitions to `Completed` and the background worker picks it up for processing. The processing logic lives in [`src/manager/processor.rs`](src/manager/processor.rs):

```rust
pub async fn process(state: AppState, upload_id: &str) -> anyhow::Result<()> {
    state.upload_service.begin_processing(upload_id).await?;

    let upload = state.upload_service.get_upload(upload_id).await?;
    let file_path = state.config.storage_dir.join(&upload.storage_path);

    // TODO: add your logic here
    // e.g. validate, transcode, forward to S3, call a downstream API

    // On failure:
    // state.upload_service.fail_processing(upload_id, &err.to_string()).await?;

    state.upload_service.complete_processing(upload_id).await?;
    Ok(())
}
```

Alternatively, subscribe to the `completed` webhook event and process the file in a separate service — see [Webhooks](#webhooks).

## Architecture

```
tus-server/
├── src/
│   ├── main.rs               # Startup: pool, migrations, workers, router
│   ├── app_state.rs          # Shared state passed to all handlers
│   ├── config.rs             # Environment-based configuration
│   ├── tus/                  # TUS protocol implementation
│   │   ├── handlers.rs       # HTTP handlers (OPTIONS/POST/HEAD/PATCH/DELETE)
│   │   ├── service.rs        # Business logic, per-upload locking
│   │   ├── repository.rs     # UploadRepository trait + SQLite impl
│   │   ├── storage.rs        # StorageBackend trait + filesystem impl
│   │   ├── model.rs          # Upload, UploadEvent, UploadStatus types
│   │   ├── metadata.rs       # Upload-Metadata header parsing
│   │   └── error.rs          # TusError with IntoResponse
│   ├── dashboard/            # Management API + SPA serving
│   │   ├── handlers.rs       # REST handlers for uploads and webhooks
│   │   ├── routes.rs         # Router: /api/* + static fallback
│   │   └── sse.rs            # Server-Sent Events for live event streaming
│   ├── manager/              # Background tasks
│   │   ├── worker.rs         # Subscribes to events, drives processing
│   │   ├── cleanup.rs        # Periodic stale-upload abandonment
│   │   └── processor.rs      # Processing entry point — extend this
│   └── webhook/              # Outbound webhook system
│       ├── dispatcher.rs     # Broadcasts events → HTTP POST with retries
│       ├── repository.rs     # WebhookRepository trait + SQLite impl
│       └── model.rs          # WebhookConfig, WebhookDelivery types
├── dashboard-ui/             # SvelteKit frontend (adapter-static)
│   └── src/routes/
│       ├── +page.svelte      # Upload list + test uploader
│       ├── uploads/[id]/     # Upload detail + live event log
│       └── webhooks/         # Webhook management + delivery log
├── migrations/               # SQLx migrations (run automatically)
├── start.sh                  # Start server in background (PID file)
├── stop.sh                   # Graceful stop (SIGTERM → SIGKILL)
└── .env.example              # Configuration template
```

## Development

### Run backend with hot-ish reload

```bash
# Watch mode requires cargo-watch: cargo install cargo-watch
cargo watch -x run
```

Or just re-run manually:

```bash
RUST_LOG=debug cargo run
```

### Run frontend dev server

```bash
cd dashboard-ui
npm run dev
```

The Vite dev server runs on port 5173 and proxies `/api` and `/files` to `localhost:3000`, so start the backend first.

### Linting

```bash
cargo clippy -- -D warnings
cargo fmt --check
cd dashboard-ui && npm run check
```

### Database schema

Migrations live in [`migrations/`](migrations/) and are embedded into the binary via `sqlx::migrate!()`. They run automatically on every startup. To add a migration, create `migrations/003_your_change.sql`.

## Deploying

1. Build a release binary and the dashboard:
   ```bash
   cd dashboard-ui && npm run build && cd ..
   cargo build --release
   ```

2. Copy to the server:
   ```bash
   scp target/release/tus-server user@host:/opt/tus/
   scp -r dashboard-ui/build user@host:/opt/tus/dashboard-ui/
   scp migrations/ user@host:/opt/tus/   # if not embedded
   ```

3. Set environment variables (via `.env` or systemd `EnvironmentFile`) and run the binary. The binary must be started from the directory containing `dashboard-ui/build/` and `migrations/`, or set paths accordingly.

> **Behind a reverse proxy:** set `BASE_URL` to your public URL so `Location` headers returned to TUS clients are correct. Pass `X-Forwarded-For` / `X-Real-IP` headers if you need them upstream.

## License

MIT
