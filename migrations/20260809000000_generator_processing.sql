CREATE TABLE generator_processing_recipes (
    processing_hash TEXT PRIMARY KEY,
    recipe_version INTEGER NOT NULL,
    deployed_generator_identity TEXT NOT NULL,
    manifest_identity TEXT NOT NULL,
    input_set_identity TEXT NOT NULL,
    settings_identity TEXT NOT NULL,
    settings_schema_identity TEXT NOT NULL,
    compatibility_decision_identity TEXT NOT NULL,
    compatibility_status TEXT NOT NULL,
    recipe_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX generator_processing_recipes_exact_idx
    ON generator_processing_recipes (
        processing_hash,
        deployed_generator_identity,
        manifest_identity,
        settings_identity,
        compatibility_decision_identity
    );

CREATE TABLE generator_processing_occurrences (
    occurrence_identity TEXT PRIMARY KEY,
    processing_hash TEXT NOT NULL,
    occurrence_order INTEGER NOT NULL,
    object_identity TEXT NOT NULL,
    content_identity TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    content_byte_length INTEGER NOT NULL,
    staged_path TEXT NOT NULL,
    transport_role TEXT NOT NULL,
    display_name TEXT,
    mapping_json TEXT NOT NULL,
    provenance_json TEXT NOT NULL,
    placement_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY (processing_hash) REFERENCES generator_processing_recipes(processing_hash),
    UNIQUE (processing_hash, occurrence_order),
    UNIQUE (processing_hash, object_identity),
    UNIQUE (processing_hash, staged_path)
);

CREATE INDEX generator_processing_occurrences_recipe_order_idx
    ON generator_processing_occurrences (processing_hash, occurrence_order);
CREATE INDEX generator_processing_occurrences_content_idx
    ON generator_processing_occurrences (content_sha256, content_byte_length);

ALTER TABLE artifact_sets ADD COLUMN generator_processing_hash TEXT
    REFERENCES generator_processing_recipes(processing_hash);

CREATE INDEX artifact_sets_generator_postprocess_ready_idx
    ON artifact_sets (
        generator_processing_hash,
        postprocess_hash,
        output_kind,
        format,
        status,
        created_at
    );
