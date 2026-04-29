CREATE TABLE webhooks (
    id         TEXT    PRIMARY KEY,
    name       TEXT    NOT NULL,
    url        TEXT    NOT NULL,
    secret     TEXT,
    events     TEXT    NOT NULL DEFAULT '[]',
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE webhook_deliveries (
    id            TEXT    PRIMARY KEY,
    webhook_id    TEXT    NOT NULL REFERENCES webhooks(id) ON DELETE CASCADE,
    upload_id     TEXT,
    event_type    TEXT    NOT NULL,
    payload       TEXT    NOT NULL,
    status_code   INTEGER,
    response_body TEXT,
    error         TEXT,
    attempts      INTEGER NOT NULL DEFAULT 1,
    delivered_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_webhook_deliveries_webhook_id   ON webhook_deliveries(webhook_id);
CREATE INDEX idx_webhook_deliveries_delivered_at ON webhook_deliveries(delivered_at);
