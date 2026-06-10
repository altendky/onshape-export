CREATE TABLE parameter_metadata (
    model_slug TEXT PRIMARY KEY,
    raw_object_key TEXT NOT NULL,
    normalized_object_key TEXT NOT NULL,
    refreshed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
