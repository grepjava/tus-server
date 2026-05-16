CREATE TABLE IF NOT EXISTS audit_log (
    id          TEXT    PRIMARY KEY,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    request_id  TEXT,
    actor       TEXT    NOT NULL,
    source_ip   TEXT,
    method      TEXT    NOT NULL,
    path        TEXT    NOT NULL,
    upload_id   TEXT,
    status_code INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_log_created_at ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_log_upload_id  ON audit_log(upload_id);
