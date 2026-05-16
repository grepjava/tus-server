CREATE TABLE IF NOT EXISTS contexts (
    id           TEXT PRIMARY KEY,
    slug         TEXT NOT NULL UNIQUE COLLATE NOCASE,
    display_name TEXT NOT NULL,
    api_key_hash TEXT NOT NULL,
    storage_prefix TEXT NOT NULL DEFAULT '',
    max_upload_bytes INTEGER,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_contexts_slug ON contexts (slug COLLATE NOCASE);

ALTER TABLE uploads ADD COLUMN context_id TEXT REFERENCES contexts(id);
CREATE INDEX IF NOT EXISTS idx_uploads_context ON uploads (context_id);

ALTER TABLE webhooks ADD COLUMN context_id TEXT REFERENCES contexts(id);
CREATE INDEX IF NOT EXISTS idx_webhooks_context ON webhooks (context_id);

ALTER TABLE upload_events ADD COLUMN context_id TEXT;
