# Tuskar

A [TUS 1.0.0](https://tus.io/protocols/resumable-upload) resumable upload server written in Rust, with a built-in management dashboard, monitoring, and webhook support.

## Features

- **Full TUS 1.0.0 protocol** — OPTIONS, POST, HEAD, PATCH, DELETE with six extensions: `creation`, `creation-defer-length`, `termination`, `concatenation`, `checksum`, `expiration`
- **Resumable downloads** — `GET /{context}/files/:id` with `Range` header support; serves partial content (206) or full file (200)
- **SQLite state storage** — zero-dependency database, migrates automatically on startup
- **Filesystem and S3 storage** — filesystem streams chunks directly to disk; S3 backend stages locally then uploads on completion with multipart support
- **Upload lifecycle** — Created → Uploading → Completed → Processing → Finalized (or Failed / Abandoned)
- **Background workers** — auto-processes completed uploads; cleans up stale ones on a configurable interval
- **Management dashboard** — SvelteKit SPA served from the same process; stats, search, filtering, bulk operations, live event log via SSE
- **Session-based auth** — login screen, bcrypt passwords, 24-hour HttpOnly sessions; admin and viewer roles
- **OIDC / SSO** — OpenID Connect authorization code flow with PKCE; auto-provisions users from any compliant IdP (Google, Okta, Entra ID, Keycloak, etc.)
- **User management** — create/delete users, change passwords, role assignment — all from the dashboard
- **Multi-context** — run multiple isolated upload namespaces on one server; each context gets its own URL prefix (`/{slug}/files`), API key, per-context quota, and scoped webhooks
- **Webhooks** — HMAC-SHA256-signed HTTP callbacks on any lifecycle event, configurable per-endpoint, delivery log with automatic retries; can be scoped to a specific context or fire globally
- **Processing pipeline** — `Processor` trait with built-in `nop`, `exec`, `mime`, and `av` processors; configure a pipeline of processors to run after each upload completes
- **Antivirus scanning** — ClamAV (`clamscan`) or HTTP AV API backend; ClamAV signatures kept up to date by a sidecar container
- **MIME / extension filtering** — magic-byte MIME detection via `infer`; allow-lists and deny-lists for both MIME types and file extensions
- **Rate limiting** — per-IP token-bucket rate limiter; configurable steady rate and burst
- **Login throttling** — in-memory per-IP brute-force protection with configurable lockout
- **Storage quotas** — cap total stored bytes and/or number of concurrent active uploads; per-context quotas also supported
- **Audit log** — every request recorded: timestamp, actor, source IP, method, path, upload ID, HTTP status; visible in the dashboard
- **Prometheus metrics** — counters and gauges at `/metrics`; Grafana dashboard included out-of-the-box via Docker Compose
- **Live settings management** — most configuration can be changed from the Settings page without touching environment variables or restarting

## Quick start (Docker)

### First-time setup

```bash
./setup.sh
```

This checks prerequisites (Docker, Compose), creates `.env` from the template, pulls third-party images, and builds the Tuskar image.

### Start

```bash
./start.sh
```

Services started:

| Service | Default URL | Description |
|---|---|---|
| Tuskar | http://localhost:3000 | Upload server + dashboard |
| Grafana | http://localhost:3001 | Metrics dashboard |
| Prometheus | http://localhost:9090 | Metrics scraper |

Log in to the dashboard with the default credentials:

| Username | Password |
|---|---|
| `admin` | `admin123` |

**Change the password immediately** after first login via the Users page.

### Stop

```bash
./stop.sh          # stop containers, keep data volumes
./stop.sh --clean  # stop + delete all volumes (irreversible)
```

### Rebuild after code changes

```bash
./build.sh              # rebuild image + restart tus container
./build.sh --no-cache   # force clean rebuild (busts all Docker layers)
./build.sh --no-restart # build only, do not restart the running container
```

### Without the scripts

```bash
BASE_URL=https://uploads.example.com docker compose up
```

Data and uploads are stored in named Docker volumes (`tus-data`, `tus-uploads`) that survive container restarts.

## Quick start (native)

### Prerequisites

- Rust 1.75+ (`rustup` recommended)
- Node.js 18+ and npm (for the dashboard UI)

### Build

```bash
cd dashboard-ui && npm install && npm run build && cd ..
cargo build --release
```

### Run

```bash
export DATABASE_URL=tus.db
export STORAGE_DIR=uploads
export BASE_URL=http://localhost:3000
export BIND_ADDR=0.0.0.0:3000
export RUST_LOG=info

mkdir -p uploads
./target/release/tus-server
```

## Authentication

The dashboard and all `/api/*` endpoints require a valid session, except the following public routes:

| Public route | Purpose |
|---|---|
| `GET /api/health` | Health check — always public |
| `POST /api/auth/login` | Obtain a session via username / password |
| `GET /api/auth/config` | Tells the login page whether OIDC is enabled |
| `GET /api/auth/oidc/login` | Initiates OIDC authorization code flow |
| `GET /api/auth/oidc/callback` | OIDC callback — provisions user and sets session |

The `API_KEY` environment variable applies **only** to the legacy TUS endpoint (`/files`, `/files/:id`). It does not affect dashboard login, session APIs, or the UI.

On first startup, a default `admin` user is created automatically with the password `admin123`. Log in at `http://localhost:3000/login`.

### Roles

| Role | Capabilities |
|---|---|
| `admin` | Full access — can manage users, change settings, delete uploads |
| `viewer` | Read-only access to uploads, webhooks, audit log, and metrics |

### Changing the default password

Open the dashboard → **Users** → click the key icon next to `admin` → enter the new password.

### OIDC / SSO

Set the four required environment variables and Tuskar will show a **Sign in with SSO** button on the login page:

```bash
OIDC_ISSUER_URL=https://accounts.google.com   # discovery endpoint
OIDC_CLIENT_ID=your-client-id
OIDC_CLIENT_SECRET=your-client-secret
OIDC_REDIRECT_URI=https://tuskar.example.com/api/auth/oidc/callback
OIDC_DEFAULT_ROLE=viewer                       # role assigned to auto-provisioned users (default: viewer)
```

The callback URL to register with your IdP is `{BASE_URL}/api/auth/oidc/callback`.

**Auto-provisioning:** on first SSO login, Tuskar looks up the user by `oidc_sub`. If not found it tries to link to an existing local account with the same verified email. Otherwise it creates a new account with a random username derived from the IdP profile and the role set by `OIDC_DEFAULT_ROLE`. Auto-provisioned accounts have no local password (`*`).

Works with any compliant OIDC provider: Google, Microsoft Entra ID, Okta, Keycloak, Auth0, GitLab, and others.

### Auth API

```bash
# Login
curl -si -X POST http://localhost:3000/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin123"}'
# → sets tuskar_session cookie

# Whoami
curl -s http://localhost:3000/api/auth/me \
  -H 'Cookie: tuskar_session=<token>'

# Logout
curl -X POST http://localhost:3000/api/auth/logout \
  -H 'Cookie: tuskar_session=<token>'
```

Sessions are stored as HttpOnly cookies (`tuskar_session`) and expire after 24 hours.

## Dashboard

The management dashboard is a SvelteKit SPA served as static files from `dashboard-ui/build/` by the same Axum process — no separate web server needed.

### Pages

| Path | Role required | Description |
|---|---|---|
| `/` | Any | Upload list — stats cards, search, status filtering, bulk delete, context column, live auto-refresh |
| `/uploads/:id` | Any | Upload detail — metadata, progress bar, live SSE event log, download button |
| `/dashboard` | Any | Grafana metrics embedded (configure `GRAFANA_URL` in Settings) |
| `/webhooks` | Any | Webhook management — add, edit, toggle, delivery log |
| `/audit` | Any | Audit log — searchable request history |
| `/settings` | Admin | Live configuration editor — grouped by category, with restart-required indicators |
| `/users` | Admin | User management — create users, change passwords, assign roles |
| `/contexts` | Admin | Context management — create namespaces, set quotas, rotate API keys |
| `/health` | Any | Server health status |
| `/metrics` | Any | Raw Prometheus metrics |

### Test upload panel

The uploads page has a collapsible **Test Upload** panel. Select a file (or drag and drop), pick a context and chunk size, then click **Start upload**. The panel speaks the TUS protocol directly from the browser so you can verify end-to-end behaviour including chunked transfers.

**Context selector** — the dropdown defaults to *Global (/files)*. Selecting a named context switches the endpoint to `/{slug}/files` and reveals an **API key** field; the Bearer token is sent with every create and patch request. Uploads are tagged with their context in the table's **Context** column (cyan slug badge for context uploads, grey "global" for the default endpoint).

## Configuration

All configuration is via environment variables. Copy `.env.example` to `.env` and edit as needed — the server loads it automatically on startup. Most settings can also be changed live from the **Settings** page in the dashboard (changes take effect immediately for non-restart-required settings).

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | `tus.db` | SQLite database file path |
| `STORAGE_DIR` | `uploads` | Directory where uploaded files are stored |
| `BASE_URL` | `http://localhost:3000` | Public base URL — used in `Location` headers returned to TUS clients |
| `BIND_ADDR` | `0.0.0.0:3000` | Address and port to listen on |
| `MAX_UPLOAD_BYTES` | `107374182400` (100 GB) | Maximum allowed `Upload-Length` per upload |
| `UPLOAD_EXPIRY_HOURS` | `24` | Hours until an upload expires (returned as `Upload-Expires`) |
| `ABANDONED_AFTER_HOURS` | `24` | Mark uploads with no activity after this many hours as abandoned |
| `CLEANUP_INTERVAL_SECS` | `3600` | How often the cleanup worker runs |
| `API_KEY` | _(none)_ | If set, all TUS `/files` (legacy) requests must include `Authorization: Bearer <key>`. `/api/health` is always public. Context-scoped routes (`/{slug}/files`) use per-context keys instead. |
| `COOKIE_SECURE` | `false` | Set to `true` when serving over HTTPS — adds the `Secure` flag to session cookies |
| `TRUSTED_PROXIES` | _(none)_ | Comma-separated CIDRs of trusted reverse proxies whose `X-Forwarded-For` header is honoured. When unset, forwarded headers are ignored and the TCP peer address is used directly. Set to your proxy/Docker network CIDRs (e.g. `10.0.0.0/8,172.16.0.0/12,192.168.0.0/16`) when running behind nginx/Caddy/Traefik. |
| `LOGIN_MAX_ATTEMPTS` | `10` | Maximum failed login attempts from one IP before lockout |
| `LOGIN_LOCKOUT_SECS` | `900` | Lockout duration in seconds (15 minutes by default). In-memory — resets on restart. |
| `WEBHOOK_MAX_ATTEMPTS` | `3` | Maximum delivery attempts per webhook event before giving up |
| `WEBHOOK_RETRY_DELAYS` | `1,4` | Comma-separated seconds to wait before each retry attempt |
| `WEBHOOK_DELIVERY_RETENTION_DAYS` | `30` | Webhook delivery records older than this are deleted by the cleanup worker |
| `PROCESSORS` | `nop` | Comma-separated processor pipeline: `nop`, `exec`, `mime`, `av` |
| `PROCESSOR_EXEC_COMMAND` | _(none)_ | Shell command run by the `exec` processor. Required when `exec` is in `PROCESSORS`. |
| `PROCESSOR_EXEC_TIMEOUT_SECS` | `300` | Seconds before the `exec` processor is forcibly killed |
| `MIME_ALLOW` | _(none)_ | Comma-separated MIME type allow-list (e.g. `image/jpeg,application/pdf`) |
| `MIME_DENY` | _(none)_ | Comma-separated MIME type deny-list |
| `EXT_ALLOW` | _(none)_ | Comma-separated filename extension allow-list (without dot, e.g. `jpg,png,pdf`) |
| `EXT_DENY` | _(none)_ | Comma-separated filename extension deny-list |
| `AV_SCANNER` | `clamav` | AV backend: `clamav` (runs `clamscan`) or `http` (POSTs to an HTTP AV API) |
| `AV_TIMEOUT_SECS` | `120` | Seconds before the AV scanner is forcibly killed |
| `AV_CLAMAV_BIN` | `clamscan` | Path to the `clamscan` binary |
| `AV_HTTP_URL` | _(none)_ | URL to POST the file to for scanning (required when `AV_SCANNER=http`). HTTP 2xx = clean. |
| `AV_HTTP_HEADER` | _(none)_ | Single `Name: value` header added to the HTTP AV request (e.g. `X-Api-Key: secret`) |
| `AV_HTTP_MAX_BYTES` | `104857600` (100 MB) | Files larger than this are refused by the HTTP scanner |
| `STORAGE_BACKEND` | `filesystem` | Storage backend: `filesystem` or `s3` |
| `S3_BUCKET` | _(none)_ | S3 bucket name. Required when `STORAGE_BACKEND=s3`. |
| `S3_PREFIX` | `uploads/` | Key prefix for all objects written to S3 |
| `S3_STAGING_DIR` | `{STORAGE_DIR}/staging` | Local staging directory for in-progress uploads |
| `S3_FORCE_PATH_STYLE` | `false` | Use path-style S3 URLs (required for MinIO and LocalStack) |
| `S3_MULTIPART_THRESHOLD` | `8388608` (8 MiB) | Files smaller than this use single `PutObject`; larger files use multipart |
| `S3_PART_SIZE` | `8388608` (8 MiB) | Part size for multipart uploads (min 5 MiB per AWS) |
| `RATE_LIMIT_RPS` | `0` (disabled) | Requests per second per client IP. `0` = disabled. |
| `RATE_LIMIT_BURST` | same as RPS | Burst allowance above the steady rate. Excess requests get `429 Too Many Requests`. |
| `QUOTA_MAX_STORAGE_BYTES` | `0` (no limit) | Maximum total bytes across all active uploads. Rejected with `507 Insufficient Storage`. |
| `QUOTA_MAX_ACTIVE_UPLOADS` | `0` (no limit) | Maximum number of concurrent active uploads. Rejected with `507 Insufficient Storage`. |
| `AUDIT_LOG_RETENTION_DAYS` | `90` | Audit log entries older than this are deleted by the cleanup worker. `0` = keep forever. |
| `GRAFANA_URL` | _(none)_ | URL of your Grafana instance — embedded in the Metrics tab of the dashboard |
| `OIDC_ISSUER_URL` | _(none)_ | OIDC provider discovery URL (e.g. `https://accounts.google.com`). Set to enable SSO. |
| `OIDC_CLIENT_ID` | _(none)_ | OAuth 2.0 client ID. Required when `OIDC_ISSUER_URL` is set. |
| `OIDC_CLIENT_SECRET` | _(none)_ | OAuth 2.0 client secret. |
| `OIDC_REDIRECT_URI` | _(none)_ | Callback URL registered with your IdP (e.g. `https://tuskar.example.com/api/auth/oidc/callback`). |
| `OIDC_DEFAULT_ROLE` | `viewer` | Role assigned to auto-provisioned SSO users (`admin` or `viewer`). |
| `RUST_LOG` | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

## TUS protocol

### Base URLs

Tuskar exposes two TUS mounting points:

| Prefix | Auth | Use case |
|---|---|---|
| `/files` | Global `API_KEY` (or none) | Legacy / single-tenant deployments |
| `/{context}/files` | Per-context API key | Multi-context deployments (see [Contexts](#contexts)) |

Both support the same TUS extensions and protocol semantics. Use the context-scoped route when different applications should have isolated upload namespaces.

### Endpoints

| Method | Path | Description |
|---|---|---|
| `OPTIONS` | `/{prefix}` | Returns server capabilities |
| `POST` | `/{prefix}` | Create a new upload, returns `Location` header |
| `HEAD` | `/{prefix}/:id` | Get current offset and length |
| `PATCH` | `/{prefix}/:id` | Upload a chunk |
| `DELETE` | `/{prefix}/:id` | Terminate an upload |
| `GET` | `/{prefix}/:id` | Download the completed file (supports `Range`) |

### Required headers

**POST (create):**
```
Tus-Resumable: 1.0.0
Upload-Length: <total bytes>          # omit when using Upload-Defer-Length
Upload-Defer-Length: 1                # optional — defer size declaration
Upload-Metadata: filename <base64>    # optional
Upload-Concat: partial                # optional — mark as a concat segment
Upload-Concat: final ;/files/id1 ...  # optional — create a concat final upload
```

The response always includes `Upload-Expires`. When `Upload-Concat: partial` is sent, the response echoes `Upload-Concat: partial` to confirm the type.

**PATCH (chunk):**
```
Tus-Resumable: 1.0.0
Content-Type: application/offset+octet-stream
Upload-Offset: <current offset>
Content-Length: <chunk size>
Upload-Checksum: sha256 <base64>      # optional — verified before write is committed
Upload-Length: <total bytes>          # optional — only for deferred-length uploads
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

### Extensions

#### Deferred length (`creation-defer-length`)

When the total size is not known upfront, omit `Upload-Length` and send `Upload-Defer-Length: 1` in the POST. Include `Upload-Length` in any subsequent PATCH once the size is known — the server fixes the length at that point and enforces it for remaining chunks.

```bash
LOCATION=$(curl -si -X POST http://localhost:3000/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Defer-Length: 1" \
  | grep -i location | tr -d '\r' | awk '{print $2}')

curl -X PATCH "$LOCATION" \
  -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" \
  -H "Upload-Length: $SIZE" \
  -H "Content-Length: $SIZE" \
  --data-binary @"$FILE"
```

HEAD responses omit `Upload-Length` until the size is finalized.

#### Checksum (`checksum`)

Include `Upload-Checksum: <algorithm> <base64>` in a PATCH to ask the server to verify the chunk. Supported algorithms: `sha1`, `sha256`. The hash is computed while streaming — no extra buffering. On mismatch the server rolls back the written bytes and returns **460 Checksum Mismatch**.

```bash
SUM=$(sha256sum "$FILE" | awk '{print $1}' | xxd -r -p | base64)

curl -X PATCH "$LOCATION" \
  -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" \
  -H "Upload-Length: $SIZE" \
  -H "Upload-Checksum: sha256 $SUM" \
  --data-binary @"$FILE"
```

#### Expiration (`expiration`)

POST and HEAD responses include an `Upload-Expires` header (RFC 2616 date format). The expiry is computed as `created_at + UPLOAD_EXPIRY_HOURS`.

Expiry is **enforced**, not just advertised:

- **HEAD** returns **410 Gone** if the upload is past its expiry time.
- **PATCH** returns **410 Gone** if the upload is past its expiry time, rolling back any partially written bytes before responding.

The background cleanup worker additionally abandons uploads that have been *inactive* beyond `ABANDONED_AFTER_HOURS`.

#### Concatenation (`concatenation`)

Upload large files in parallel segments, then merge them in one request.

```bash
# 1. Create two partial uploads
P1=$(curl -si -X POST http://localhost:3000/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: $PART1_SIZE" \
  -H "Upload-Concat: partial" \
  | grep -i location | tr -d '\r' | awk '{print $2}')

P2=$(curl -si -X POST http://localhost:3000/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: $PART2_SIZE" \
  -H "Upload-Concat: partial" \
  | grep -i location | tr -d '\r' | awk '{print $2}')

# 2. Upload each partial (can be done in parallel)
curl -X PATCH "$P1" -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" --data-binary @part1.bin

curl -X PATCH "$P2" -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" --data-binary @part2.bin

# 3. Create the final concatenated upload (returns immediately)
curl -si -X POST http://localhost:3000/files \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Concat: final ;$P1 $P2"
```

The server concatenates the partial files on disk, marks the final upload as `Completed`, and it flows through the normal processing pipeline. The consumed partial uploads are automatically marked `Abandoned` and pruned by the cleanup worker.

## Client libraries

| Platform | Library | Notes |
|---|---|---|
| **Web / Node.js** | [tus-js-client](https://github.com/tus/tus-js-client) | Official TUS client; works in browsers, Node.js, React Native, and Cordova |
| **Flutter (Android + iOS)** | [tusc](https://pub.dev/packages/tusc) | Pure-Dart client; supports pause/resume, persistent caching, stream-based uploads |

## Webhooks

The server sends an HTTP POST to any configured URL when an upload lifecycle event occurs.

### Event types

| Event | When | Notes |
|---|---|---|
| `created` | Upload record created (POST /files received) | No bytes yet |
| `chunk_received` | A PATCH chunk was written | High volume — fires on every chunk |
| `completed` | All bytes received | Processors may still be running |
| `processing_started` | Background processor began | |
| `finalized` | All processors passed | **Recommended** — file is safe and ready to use |
| `processing_failed` | A processor rejected or errored | File may be quarantined |
| `abandoned` | Upload expired or manually abandoned | |
| `deleted` | Upload deleted via DELETE /files/:id | |
| `retry_queued` | Processing manually re-queued | |

**Use `finalized`** for downstream integrations (move the file, notify another service, etc.). It is the only event that guarantees all configured processors (AV scan, MIME filter, etc.) have passed.

### Configuring a webhook

**Via the dashboard:** go to **Webhooks** → **+ Add webhook**.

**Via the API:**
```bash
curl -X POST http://localhost:3000/api/webhooks \
  -H 'Content-Type: application/json' \
  -H 'Cookie: tuskar_session=<token>' \
  -d '{
    "name": "My service",
    "url": "https://your-service.example.com/hooks/tus",
    "secret": "optional-shared-secret",
    "events": ["finalized", "processing_failed"]
  }'
```

### Payload

```json
{
  "event_type": "finalized",
  "upload_id": "a3f2c1d0-...",
  "event_id": "b9e1...",
  "message": null,
  "timestamp": "2026-05-07T12:34:56Z",
  "file": {
    "filename": "report.pdf",
    "storage_path": "a3f2c1d0-.../report.pdf",
    "absolute_path": "/var/uploads/a3f2c1d0-.../report.pdf",
    "size": 2097152,
    "offset": 2097152,
    "status": "Finalized"
  }
}
```

### Signature verification

If a `secret` is set, every delivery includes an `X-Hub-Signature-256` header (`sha256=<hex>`). Verify it on the receiver:

```js
const crypto = require('crypto');

app.post('/hooks/tus', (req, res) => {
  const sig = req.headers['x-hub-signature-256'];
  const expected = 'sha256=' + crypto
    .createHmac('sha256', process.env.WEBHOOK_SECRET)
    .update(req.rawBody)
    .digest('hex');

  if (!crypto.timingSafeEqual(Buffer.from(sig), Buffer.from(expected))) {
    return res.sendStatus(401);
  }
  res.sendStatus(200);
});
```

### Retries

Failed deliveries (non-2xx or network error) are retried up to `WEBHOOK_MAX_ATTEMPTS` times with delays defined by `WEBHOOK_RETRY_DELAYS` (default: 1 s, then 4 s). The full outcome — HTTP status, response body (capped at 4 KB), error, attempt count — is stored and visible in the delivery log in the dashboard. At most 32 webhook dispatches run concurrently.

## Contexts

Contexts let multiple applications share one Tuskar instance while keeping their uploads, webhooks, and quotas completely isolated. Each context gets:

- A dedicated URL prefix: `/{slug}/files`
- Its own API key (SHA-256 hashed at rest, returned in plaintext only once)
- An optional per-context upload quota (overrides global `MAX_UPLOAD_BYTES`)
- Scoped webhooks — webhooks can be tied to a specific context so they only fire for that context's uploads

Webhooks with no `context_id` set fire for **all** uploads regardless of context.

### Creating a context

**Via the dashboard:** go to **Contexts** → **+ New context**. Fill in the slug (URL-safe, e.g. `hr-system`), display name, and optional quota. Copy the API key from the green banner — it is shown only once.

**Via the API:**
```bash
curl -X POST http://localhost:3000/api/contexts \
  -H 'Content-Type: application/json' \
  -H 'Cookie: tuskar_session=<token>' \
  -d '{
    "slug": "hr-system",
    "display_name": "HR System",
    "max_upload_bytes": 10737418240
  }'
```

Response includes `api_key` — store it securely. It cannot be retrieved again; use `/rotate-key` to replace it.

### Using a context

Include the API key as a Bearer token on all TUS requests to that context's prefix:

```bash
FILE=contract.pdf
SIZE=$(wc -c < "$FILE")
NAME=$(echo -n "$FILE" | base64)

# Create
LOCATION=$(curl -si -X POST http://localhost:3000/hr-system/files \
  -H "Authorization: Bearer <context-api-key>" \
  -H "Tus-Resumable: 1.0.0" \
  -H "Upload-Length: $SIZE" \
  -H "Upload-Metadata: filename $NAME" \
  | grep -i location | tr -d '\r' | awk '{print $2}')

# Upload
curl -X PATCH "$LOCATION" \
  -H "Authorization: Bearer <context-api-key>" \
  -H "Tus-Resumable: 1.0.0" \
  -H "Content-Type: application/offset+octet-stream" \
  -H "Upload-Offset: 0" \
  --data-binary @"$FILE"
```

The `Location` header in the POST response will be `{BASE_URL}/hr-system/files/{id}`, so clients automatically resume against the correct context prefix.

### Rotating an API key

**Via the dashboard:** Contexts page → **Rotate key** next to the context. The old key stops working immediately.

**Via the API:**
```bash
curl -X POST http://localhost:3000/api/contexts/<id>/rotate-key \
  -H 'Cookie: tuskar_session=<token>'
```

### Contexts API reference

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/contexts` | List all contexts |
| `POST` | `/api/contexts` | Create a context — returns `api_key` in response |
| `GET` | `/api/contexts/:id` | Get a single context |
| `PUT` | `/api/contexts/:id` | Update display name or quota |
| `DELETE` | `/api/contexts/:id` | Delete a context (uploads are orphaned, not deleted) |
| `POST` | `/api/contexts/:id/rotate-key` | Issue a new API key — returns `api_key` |

## Management API

All endpoints require a valid session cookie (`tuskar_session`). Obtain one via `/api/auth/login`.

### Auth

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/auth/login` | `{"username":"…","password":"…"}` → sets session cookie |
| `POST` | `/api/auth/logout` | Clears session |
| `GET` | `/api/auth/me` | Returns current user (`id`, `username`, `role`) |
| `GET` | `/api/auth/config` | Returns `{"oidc": true/false}` — used by login page to show SSO button |
| `GET` | `/api/auth/oidc/login` | Initiates OIDC authorization code flow (redirect) |
| `GET` | `/api/auth/oidc/callback` | OIDC callback — exchanges code, provisions user, sets session |

### Uploads

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/uploads` | List all uploads |
| `GET` | `/api/uploads/:id` | Get a single upload |
| `DELETE` | `/api/uploads/:id` | Hard-delete upload and file |
| `POST` | `/api/uploads/purge` | Bulk hard-delete `{ "ids": ["…"] }` |
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

### Settings (admin only)

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/settings` | List all settings with current values, defaults, and metadata |
| `PUT` | `/api/settings/:key` | Set a value `{ "value": "…" }` |
| `DELETE` | `/api/settings/:key` | Reset to default |

### Users (admin only)

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/users` | List all users |
| `POST` | `/api/users` | Create a user `{ "username", "password", "role" }` |
| `DELETE` | `/api/users/:id` | Delete a user (cannot delete yourself) |
| `PUT` | `/api/users/:id/password` | Change password `{ "new_password", "current_password?" }` |

### Contexts (admin only)

See [Contexts API reference](#contexts-api-reference) above.

### Audit log

| Method | Path | Description |
|---|---|---|
| `GET` | `/api/audit` | List recent audit log entries (last 500) |

### Health

```
GET /api/health  →  { "status": "ok" }
```

This endpoint is always public — no session required.

### Metrics

```
GET /metrics
```

Returns a Prometheus/OpenMetrics text payload. Subject to `API_KEY` auth if set.

| Metric | Type | Description |
|---|---|---|
| `tus_uploads_created_total` | Counter | Uploads created (POST /files) |
| `tus_uploads_completed_total` | Counter | Uploads where all bytes were received |
| `tus_processing_failures_total` | Counter | Uploads that failed the processing step |
| `tus_bytes_uploaded_total` | Counter | Total bytes received across all uploads |
| `tus_webhook_deliveries_total{outcome}` | Counter | Webhook delivery attempts by `outcome` (`success` or `failure`) |
| `tus_active_uploads` | Gauge | Current uploads in `Created` or `Uploading` state |
| `tus_processing_uploads` | Gauge | Current uploads in `Processing` state |

## Monitoring

Docker Compose starts Prometheus and Grafana alongside Tuskar:

- **Prometheus** scrapes `/metrics` every 15 seconds and retains 30 days of data.
- **Grafana** is pre-provisioned with a Tuskar dashboard at `http://localhost:3001`. Anonymous viewer access is enabled by default.
- The **Metrics tab** in the dashboard embeds Grafana in an iframe. Set `GRAFANA_URL` in Settings to point to your Grafana instance.

To configure Prometheus with `API_KEY` auth, add to `monitoring/prometheus.yml`:

```yaml
scrape_configs:
  - job_name: tuskar
    authorization:
      credentials: "Bearer <your-api-key>"
    static_configs:
      - targets: ["tus:3000"]
```

## Custom processing

When the final chunk arrives the upload transitions to `Completed` and the background worker runs the configured processor pipeline:

```bash
PROCESSORS=mime,av,exec
```

Processors run in order. Any failure marks the upload `FailedProcessing` and stops the pipeline.

### Built-in processors

| Name | Description | Key env vars |
|---|---|---|
| `nop` | No-op (default — does nothing) | — |
| `exec` | Runs a shell command; receives upload context via env vars | `PROCESSOR_EXEC_COMMAND`, `PROCESSOR_EXEC_TIMEOUT_SECS` |
| `mime` | Rejects files by MIME type or extension (magic-byte detection via `infer`) | `MIME_ALLOW`, `MIME_DENY`, `EXT_ALLOW`, `EXT_DENY` |
| `av` | Virus scan via ClamAV (`clamscan`) or an HTTP AV API | `AV_SCANNER`, `AV_CLAMAV_BIN`, `AV_HTTP_URL`, `AV_TIMEOUT_SECS` |

### `exec` processor

The shell command (`PROCESSOR_EXEC_COMMAND`) receives these environment variables:

| Variable | Value |
|---|---|
| `UPLOAD_ID` | UUID of the upload |
| `FILE_PATH` | Absolute path to the completed file |
| `FILENAME` | Original filename (may be empty) |
| `UPLOAD_SIZE` | File size in bytes |
| `METADATA_JSON` | Raw TUS metadata JSON string |

A non-zero exit code is treated as a processing failure.

**Examples:**

```bash
# Copy to an archive directory
PROCESSOR_EXEC_COMMAND='cp "$FILE_PATH" /mnt/archive/"$FILENAME"'

# Trigger a downstream service
PROCESSOR_EXEC_COMMAND='curl -sf -X POST https://myapp/ingest -d "$UPLOAD_ID"'

# Chain with AV scan first
PROCESSORS=av,exec
```

### Adding a custom Rust processor

Implement the `Processor` trait in `src/manager/` and register it in `ProcessorPipeline::from_env()` in [`src/manager/processor.rs`](src/manager/processor.rs):

```rust
#[async_trait]
impl Processor for MyProcessor {
    fn name(&self) -> &str { "my_processor" }

    async fn process(&self, ctx: &ProcessorContext) -> anyhow::Result<()> {
        // ctx.upload_id, ctx.file_path, ctx.upload
        Ok(())
    }
}
```

Alternatively, subscribe to the `finalized` webhook event and process the file in a separate service — no code changes needed.

### Antivirus (ClamAV)

When `PROCESSORS=av` is set, ClamAV signatures must be present at startup. In Docker Compose this is handled automatically: a `freshclam-update` sidecar container downloads signatures on first run and refreshes them daily.

To use an external HTTP AV API instead:

```bash
AV_SCANNER=http
AV_HTTP_URL=https://av-api.example.com/scan
AV_HTTP_HEADER=X-Api-Key: secret
```

## Audit log

Every inbound HTTP request is written to the `audit_log` SQLite table. The write is non-blocking — a failed write does not affect the response.

| Column | Description |
|---|---|
| `id` | UUID |
| `created_at` | UTC timestamp |
| `request_id` | Value of the `X-Request-Id` header |
| `actor` | `api_key` if an `Authorization` header was present, otherwise `anonymous` |
| `source_ip` | Client IP from `X-Forwarded-For` → `X-Real-IP` → TCP peer |
| `method` | HTTP method |
| `path` | Request path |
| `upload_id` | Extracted from `/files/:id` or `/api/uploads/:id` paths |
| `status_code` | HTTP response status |

Old entries are pruned automatically by the cleanup worker per `AUDIT_LOG_RETENTION_DAYS` (default 90 days). Set to `0` to keep all entries.

The audit log is also browsable from the **Audit** page in the dashboard.

## S3 storage backend

Set `STORAGE_BACKEND=s3` to store uploaded files in S3 (or any S3-compatible store). AWS credentials are read from the standard environment variable and credential chain.

```bash
# Static credentials
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
export AWS_REGION=us-east-1
export STORAGE_BACKEND=s3
export S3_BUCKET=my-uploads-bucket
```

In-progress chunks are written to a local staging directory (`S3_STAGING_DIR`). When the final chunk arrives the complete file is uploaded to S3 and the staging copy is deleted.

### MinIO / LocalStack

```bash
export STORAGE_BACKEND=s3
export S3_BUCKET=my-bucket
export S3_FORCE_PATH_STYLE=true
export AWS_ENDPOINT_URL=http://localhost:9000
export AWS_REGION=us-east-1
export AWS_ACCESS_KEY_ID=minioadmin
export AWS_SECRET_ACCESS_KEY=minioadmin
```

## Reverse proxy and TLS

The server speaks plain HTTP. TLS termination should be handled by a reverse proxy.

1. **Set `BASE_URL` to your public HTTPS URL.** The server uses this to construct `Location` headers. If this is wrong, resumable uploads will break because clients resume against the wrong URL.

2. **Forward the real client IP** via `X-Forwarded-For` or `X-Real-IP` for correct audit log entries.

### nginx

```nginx
server {
    listen 443 ssl http2;
    server_name uploads.example.com;

    ssl_certificate     /etc/ssl/certs/uploads.example.com.pem;
    ssl_certificate_key /etc/ssl/private/uploads.example.com.key;

    proxy_request_buffering off;
    client_max_body_size 0;
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;

    location / {
        proxy_pass         http://127.0.0.1:3000;
        proxy_set_header   Host               $host;
        proxy_set_header   X-Forwarded-For    $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto  $scheme;
        proxy_set_header   X-Real-IP          $remote_addr;
        proxy_set_header   X-Request-Id       $request_id;
    }
}
```

### Caddy

```caddy
uploads.example.com {
    reverse_proxy 127.0.0.1:3000 {
        header_up X-Real-IP {remote_host}
    }
}
```

### AWS ALB / Cloudflare

Both set `X-Forwarded-For` and `X-Forwarded-Proto` automatically. Increase the ALB idle timeout (default 60 s) to at least 3600 s to avoid drops during large uploads.

## Architecture

```
tuskar/
├── src/
│   ├── main.rs               # Startup: pool, migrations, workers, router
│   ├── app_state.rs          # Shared state passed to all handlers
│   ├── config.rs             # Environment-based configuration
│   ├── audit.rs              # Audit log middleware
│   ├── auth.rs               # Global API key middleware (legacy /files only)
│   ├── context.rs            # ContextConfig, ContextCache, context_auth_middleware
│   ├── login_throttle.rs     # Per-IP brute-force lockout
│   ├── metrics.rs            # Prometheus counters and gauges
│   ├── rate_limit.rs         # Per-IP token-bucket rate limiter
│   ├── trusted_proxy.rs      # X-Forwarded-For trust logic
│   ├── tus/                  # TUS protocol implementation
│   │   ├── handlers.rs       # HTTP handlers (legacy + context-scoped variants)
│   │   ├── service.rs        # Business logic, per-upload locking, event emission
│   │   ├── repository.rs     # UploadRepository trait + SQLite impl
│   │   ├── routes.rs         # tus_router (legacy) + context_tus_router
│   │   ├── storage.rs        # StorageBackend trait + filesystem impl
│   │   ├── s3_storage.rs     # S3StorageBackend (staging + multipart upload)
│   │   ├── model.rs          # Upload, NewUpload, UploadEvent, UploadStatus types
│   │   ├── metadata.rs       # Upload-Metadata header parsing
│   │   └── error.rs          # TusError with IntoResponse
│   ├── dashboard/            # Management API + SPA serving
│   │   ├── handlers.rs       # REST handlers (uploads, webhooks, settings, audit)
│   │   ├── routes.rs         # Router: public + session-protected /api/* + static fallback
│   │   ├── session.rs        # Auth: login/logout/me, session middleware, admin seeding
│   │   ├── user_handlers.rs  # User management (list, create, delete, change password)
│   │   ├── context_handlers.rs # Context CRUD + key rotation
│   │   ├── oidc.rs           # OIDC authorization code + PKCE flow, auto-provisioning
│   │   └── sse.rs            # Server-Sent Events for live event streaming
│   ├── manager/              # Background tasks
│   │   ├── worker.rs         # Subscribes to events, drives processing pipeline
│   │   ├── cleanup.rs        # Periodic stale-upload abandonment + log pruning
│   │   ├── processor.rs      # ProcessorPipeline, Processor trait, nop/exec built-ins
│   │   ├── mime_filter.rs    # mime processor: magic-byte MIME + extension filtering
│   │   └── av_scan.rs        # av processor: ClamAV or HTTP antivirus scanning
│   └── webhook/              # Outbound webhook system
│       ├── dispatcher.rs     # Broadcasts events → HMAC-signed HTTP POST with retries
│       ├── repository.rs     # WebhookRepository trait + SQLite impl (context-scoped)
│       └── model.rs          # WebhookConfig, WebhookDelivery types
├── dashboard-ui/             # SvelteKit frontend (adapter-static)
│   └── src/routes/
│       ├── +layout.svelte    # App shell: auth guard, sidebar, session state
│       ├── +page.svelte      # Upload list + test uploader
│       ├── login/            # Login page (password + SSO button)
│       ├── uploads/[id]/     # Upload detail + live event log + download button
│       ├── webhooks/         # Webhook management + delivery log
│       ├── contexts/         # Context management — create, edit, quota, rotate key
│       ├── dashboard/        # Grafana metrics iframe
│       ├── settings/         # Live settings editor
│       ├── users/            # User management
│       ├── audit/            # Audit log viewer
│       └── health/           # Health status page
├── migrations/               # SQLx migrations (run automatically on startup)
│   ├── 001_initial.sql       # uploads and upload_events tables
│   ├── 002_webhooks.sql      # webhooks and webhook_deliveries tables
│   ├── 003_tus_extensions.sql# deferred-length and concatenation columns
│   ├── 004_audit_log.sql     # audit_log table
│   ├── 005_settings.sql      # settings table
│   ├── 006_users.sql         # users and sessions tables
│   ├── 007_oidc.sql          # oidc_sub and email columns on users
│   └── 008_contexts.sql      # contexts table; context_id FK on uploads, webhooks, upload_events
├── monitoring/
│   ├── prometheus.yml        # Prometheus scrape config
│   └── grafana/              # Grafana provisioning (datasource + dashboard)
├── docker-compose.yml        # tus + prometheus + grafana + freshclam-update
├── Dockerfile
├── docker-entrypoint.sh      # Runs freshclam (if av enabled) then starts server
├── setup.sh                  # First-time setup: prereqs, .env, pull images, build
├── start.sh                  # Start all services; poll health; print URLs
├── stop.sh                   # Stop services (--clean to also delete volumes)
├── build.sh                  # Rebuild tus image + restart container
└── .env.example              # Configuration template
```

## Development

### Run backend

```bash
RUST_LOG=debug cargo run
```

### Run frontend dev server

```bash
cd dashboard-ui
npm run dev
```

The Vite dev server runs on port 5173 and proxies `/api` and `/files` to `localhost:3000`. Start the backend first.

### Lint and type-check

```bash
cargo clippy -- -D warnings
cargo fmt --check
cd dashboard-ui && npm run check
```

### Adding a migration

Create `migrations/009_your_change.sql`. Migrations are embedded in the binary via `sqlx::migrate!()` and run automatically on every startup.

## Deploying (non-Docker)

1. Build a release binary and the dashboard:
   ```bash
   cd dashboard-ui && npm run build && cd ..
   cargo build --release
   ```

2. Copy to the server:
   ```bash
   scp target/release/tus-server user@host:/opt/tuskar/
   scp -r dashboard-ui/build user@host:/opt/tuskar/dashboard-ui/
   ```

3. Set environment variables (via `.env` or a systemd `EnvironmentFile`) and run the binary from the directory containing `dashboard-ui/build/`.

## License

MIT
