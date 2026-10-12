-- Service-owned request identities bind the explicit ordered selection and
-- reviewed deployment policy to exactly one immutable processing recipe.
ALTER TABLE catalog_models ADD COLUMN bambu_project_json TEXT;

CREATE TABLE bambu_project_requests (
    request_identity TEXT PRIMARY KEY,
    request_json TEXT NOT NULL,
    processing_hash TEXT REFERENCES generator_processing_recipes(processing_hash),
    output_snapshot_identity TEXT,
    failure_category TEXT,
    failure_code TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
