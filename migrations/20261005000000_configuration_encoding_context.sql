ALTER TABLE configuration_encodings RENAME TO legacy_configuration_encodings;
DROP INDEX configuration_encodings_encoded_id_idx;

CREATE TABLE configuration_encodings (
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    encoding_context_hash TEXT NOT NULL,
    encoding_context_json TEXT NOT NULL,
    encoded_id TEXT NOT NULL,
    query_param TEXT NOT NULL,
    request_json TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_hash, config_hash, encoding_context_hash)
);

CREATE INDEX configuration_encodings_encoded_id_idx
    ON configuration_encodings (encoded_id);
