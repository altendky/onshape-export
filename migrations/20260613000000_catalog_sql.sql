CREATE TABLE catalog_models (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_order INTEGER NOT NULL DEFAULT 0,
    catalog_schema_version INTEGER NOT NULL DEFAULT 1,
    entry_version INTEGER NOT NULL DEFAULT 1,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    published INTEGER NOT NULL DEFAULT 0 CHECK (published IN (0, 1)),
    tags_json TEXT NOT NULL DEFAULT '[]',
    thumbnail TEXT,
    document_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    element_id TEXT NOT NULL,
    element_kind TEXT NOT NULL CHECK (element_kind IN ('part_studio', 'assembly')),
    link_document_id TEXT,
    downloads_json TEXT NOT NULL,
    preview_format TEXT NOT NULL DEFAULT 'glb' CHECK (preview_format IN ('glb')),
    preview_options_json TEXT NOT NULL DEFAULT '{}',
    download_options_json TEXT NOT NULL DEFAULT '{}',
    parameter_source TEXT NOT NULL DEFAULT 'onshape' CHECK (parameter_source IN ('onshape')),
    parameter_allow_unknown INTEGER NOT NULL DEFAULT 0 CHECK (parameter_allow_unknown IN (0, 1)),
    parameter_auto_refresh INTEGER NOT NULL DEFAULT 1 CHECK (parameter_auto_refresh IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE UNIQUE INDEX catalog_models_source_identity_idx
    ON catalog_models (document_id, version_id, element_id, element_kind, ifnull(link_document_id, ''));
CREATE INDEX catalog_models_published_display_idx
    ON catalog_models (published, display_order, slug);

CREATE TABLE catalog_parameter_overrides (
    model_id INTEGER NOT NULL REFERENCES catalog_models(id) ON DELETE CASCADE,
    parameter_id TEXT NOT NULL,
    label TEXT,
    description TEXT,
    hidden INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    precision INTEGER,
    widget TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (model_id, parameter_id)
);

CREATE TABLE catalog_parameter_presets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id INTEGER NOT NULL REFERENCES catalog_models(id) ON DELETE CASCADE,
    display_order INTEGER NOT NULL DEFAULT 0,
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (model_id, slug)
);

CREATE INDEX catalog_parameter_presets_model_display_idx
    ON catalog_parameter_presets (model_id, display_order, slug);

CREATE TABLE catalog_parameter_preset_values (
    preset_id INTEGER NOT NULL REFERENCES catalog_parameter_presets(id) ON DELETE CASCADE,
    parameter_id TEXT NOT NULL,
    value TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (preset_id, parameter_id)
);
