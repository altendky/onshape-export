CREATE TABLE source_resolution_aliases (
    document_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    element_id TEXT NOT NULL,
    element_kind TEXT NOT NULL,
    link_document_id TEXT,
    source_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX source_resolution_aliases_catalog_identity_idx
    ON source_resolution_aliases (document_id, version_id, element_id, element_kind, ifnull(link_document_id, ''));
CREATE INDEX source_resolution_aliases_source_hash_idx
    ON source_resolution_aliases (source_hash, updated_at);

INSERT INTO source_resolution_aliases (
    document_id, version_id, element_id, element_kind, link_document_id, source_hash
)
SELECT document_id, version_id, element_id, element_kind, link_document_id, source_hash
FROM source_resolutions;
