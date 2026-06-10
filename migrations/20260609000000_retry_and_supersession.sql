ALTER TABLE jobs ADD COLUMN max_attempts INTEGER NOT NULL DEFAULT 3;
ALTER TABLE jobs ADD COLUMN next_retry_at TEXT;

UPDATE jobs
SET status = 'queued',
    lease_until = NULL,
    next_retry_at = NULL
WHERE status = 'expired';

CREATE INDEX jobs_status_retry_created_at_idx ON jobs (status, next_retry_at, created_at);

ALTER TABLE artifacts ADD COLUMN status TEXT NOT NULL DEFAULT 'ready';
ALTER TABLE artifacts ADD COLUMN sha256 TEXT;
ALTER TABLE artifacts ADD COLUMN producing_job_key TEXT;
ALTER TABLE artifacts ADD COLUMN source_hash TEXT;
ALTER TABLE artifacts ADD COLUMN options_hash TEXT;
ALTER TABLE artifacts ADD COLUMN parameter_schema_version INTEGER;
ALTER TABLE artifacts ADD COLUMN config_values_json TEXT;
ALTER TABLE artifacts ADD COLUMN superseded_at TEXT;

CREATE INDEX artifacts_status_model_config_idx ON artifacts (status, model_slug, config_hash);
CREATE INDEX artifacts_status_created_at_idx ON artifacts (status, created_at);
