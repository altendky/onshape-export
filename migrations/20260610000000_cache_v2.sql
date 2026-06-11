DROP TABLE IF EXISTS parameter_metadata;

CREATE TABLE source_resolutions (
    source_hash TEXT PRIMARY KEY,
    model_slug TEXT NOT NULL,
    document_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    microversion_id TEXT NOT NULL,
    element_id TEXT NOT NULL,
    element_kind TEXT NOT NULL,
    link_document_id TEXT,
    diagnostics_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX source_resolutions_catalog_identity_idx
    ON source_resolutions (document_id, version_id, element_id, element_kind, ifnull(link_document_id, ''));
CREATE INDEX source_resolutions_model_slug_updated_at_idx
    ON source_resolutions (model_slug, updated_at);

CREATE TABLE parameter_metadata (
    source_hash TEXT PRIMARY KEY,
    raw_object_key TEXT NOT NULL,
    normalized_object_key TEXT NOT NULL,
    schema_hash TEXT NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE configuration_selections (
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    values_json TEXT NOT NULL,
    validation_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_hash, config_hash)
);

CREATE INDEX configuration_selections_source_hash_updated_at_idx
    ON configuration_selections (source_hash, updated_at);

CREATE TABLE configuration_encodings (
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    encoded_id TEXT NOT NULL,
    query_param TEXT NOT NULL,
    request_json TEXT NOT NULL,
    response_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_hash, config_hash)
);

CREATE INDEX configuration_encodings_encoded_id_idx
    ON configuration_encodings (encoded_id);

CREATE TABLE export_requests (
    request_hash TEXT PRIMARY KEY,
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    options_hash TEXT NOT NULL,
    output_kind TEXT NOT NULL,
    format TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    request_json TEXT NOT NULL,
    defaults_policy_version TEXT NOT NULL,
    request_builder_version TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX export_requests_source_config_output_idx
    ON export_requests (source_hash, config_hash, output_kind, format, status, created_at);
CREATE INDEX export_requests_status_updated_at_idx
    ON export_requests (status, updated_at);

CREATE TABLE translations (
    translation_id TEXT PRIMARY KEY,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    start_response_json TEXT,
    final_response_json TEXT,
    poll_state_json TEXT,
    result_external_data_ids_json TEXT,
    result_element_ids_json TEXT,
    response_hash TEXT,
    failure_reason TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX translations_request_hash_created_at_idx
    ON translations (request_hash, created_at);
CREATE INDEX translations_request_hash_state_updated_at_idx
    ON translations (request_hash, state, updated_at);

CREATE TABLE raw_payloads (
    raw_payload_hash TEXT PRIMARY KEY,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT,
    byte_len INTEGER NOT NULL,
    headers_json TEXT NOT NULL,
    original_filename TEXT,
    filename_source TEXT,
    detected_kind TEXT NOT NULL,
    zip_manifest_json TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX raw_payloads_detected_kind_created_at_idx
    ON raw_payloads (detected_kind, created_at);

CREATE TABLE raw_payload_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_hash TEXT NOT NULL,
    translation_id TEXT,
    external_data_id TEXT,
    result_index INTEGER,
    response_headers_json TEXT NOT NULL,
    etag TEXT,
    raw_payload_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX raw_payload_sources_identity_idx
    ON raw_payload_sources (
        request_hash,
        ifnull(translation_id, ''),
        ifnull(external_data_id, ''),
        ifnull(result_index, -1)
    );
CREATE INDEX raw_payload_sources_raw_payload_hash_idx
    ON raw_payload_sources (raw_payload_hash);

CREATE TABLE postprocess_runs (
    postprocess_hash TEXT PRIMARY KEY,
    raw_payload_hash TEXT NOT NULL,
    processor_name TEXT NOT NULL,
    processor_version TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    status TEXT NOT NULL,
    log_json TEXT NOT NULL,
    derived_files_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX postprocess_runs_raw_payload_status_created_at_idx
    ON postprocess_runs (raw_payload_hash, status, created_at);

CREATE TABLE artifact_sets (
    artifact_set_hash TEXT PRIMARY KEY,
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    options_hash TEXT NOT NULL,
    request_hash TEXT,
    raw_payload_hash TEXT,
    postprocess_hash TEXT,
    output_kind TEXT NOT NULL,
    format TEXT NOT NULL,
    status TEXT NOT NULL,
    primary_object_key TEXT,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    superseded_at TEXT,
    superseded_by TEXT,
    supersession_reason TEXT
);

CREATE INDEX artifact_sets_source_config_output_idx
    ON artifact_sets (source_hash, config_hash, output_kind, format, status, created_at);
CREATE INDEX artifact_sets_request_status_created_at_idx
    ON artifact_sets (request_hash, status, created_at);
CREATE INDEX artifact_sets_status_updated_at_idx
    ON artifact_sets (status, updated_at);

CREATE TABLE artifact_files (
    artifact_set_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    logical_path TEXT NOT NULL,
    original_path TEXT,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    byte_len INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (artifact_set_hash, role, logical_path)
);

CREATE INDEX artifact_files_artifact_set_hash_idx
    ON artifact_files (artifact_set_hash);
