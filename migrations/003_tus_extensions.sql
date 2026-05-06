ALTER TABLE uploads ADD COLUMN length_is_deferred INTEGER NOT NULL DEFAULT 0;
ALTER TABLE uploads ADD COLUMN concat_type TEXT;
ALTER TABLE uploads ADD COLUMN concat_uploads TEXT;
