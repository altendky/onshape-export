# Catalog

## Current Approach

The runtime catalog is live SQLite application data. It is not a versioned release file and it is not stored in Tigris.

SQLite is the source of truth for:

- Model slug, name, description, tags, thumbnail, and publication state.
- Onshape document/version/element IDs, element kind, and optional link document ID.
- Export formats and export option defaults.
- Parameter policy, presets, and UI overrides.

Tigris remains the store for artifact bytes, raw Onshape payloads, normalized parameter metadata, thumbnails/media if needed, and blob-like cache data.

The `catalog/v1` JSON files remain as a local seed and test fixture:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json
```

Seed a new database with:

```sh
onshape-export catalog import catalog/v1/models.json
```

Normal `serve`, `worker`, and `catalog validate` operations read from SQLite. There is no `CATALOG_PATH` runtime setting.

## Schema

Implemented SQLite tables:

- `catalog_models`: display/publication fields, Onshape source IDs, export settings, and parameter policy.
- `catalog_parameter_overrides`: per-parameter label, description, hidden flag, precision, and widget hints.
- `catalog_parameter_presets`: named preset metadata per model.
- `catalog_parameter_preset_values`: preset parameter values.

The catalog tables have internal row IDs for relational storage, but cache identity must not depend on row IDs or mutable slugs. Source, configuration, options, request, raw payload, post-processing, and artifact-set hashes remain the durable cache identities.

## CLI Operations

Implemented catalog commands:

```text
onshape-export catalog validate
onshape-export catalog import <models.json>
onshape-export catalog list [--json]
onshape-export catalog show <slug>
```

`catalog import` validates the JSON catalog using the same `Catalog`/`Model` rules and replaces the live SQLite catalog in a short transaction. It does not call Onshape or Tigris.

No web catalog editing routes exist. Browser-based admin is intentionally deferred until authentication and CSRF requirements are designed.

## Model Entry Shape

The JSON import fixture uses the same Rust model shape that is reconstructed from SQL:

```json
{
  "catalogSchemaVersion": 1,
  "entryVersion": 1,
  "slug": "example-model",
  "name": "Example Model",
  "description": "Short public description.",
  "published": true,
  "tags": ["example"],
  "thumbnail": null,
  "onshape": {
    "documentId": "...",
    "versionId": "...",
    "elementId": "...",
    "elementKind": "part_studio",
    "linkDocumentId": null
  },
  "exports": {
    "downloads": ["step", "stl", "3mf"],
    "preview": "glb",
    "previewOptions": {
      "resolution": "FINE"
    },
    "downloadOptions": {
      "stepVersionString": "AP242",
      "stl": {
        "resolution": "fine",
        "stlMode": "BINARY"
      },
      "3mf": {
        "resolution": "fine"
      }
    }
  },
  "parameterPolicy": {
    "source": "onshape",
    "allowUnknown": false,
    "autoRefresh": true
  },
  "parameterPresets": [
    {
      "slug": "small",
      "name": "Small",
      "values": {
        "parameter-id": "10"
      }
    }
  ],
  "parameterOverrides": {
    "parameter-id": {
      "label": "Public Label",
      "description": "Short help text.",
      "hidden": false,
      "precision": 3,
      "widget": "number"
    }
  }
}
```

For assemblies, use:

```json
"elementKind": "assembly"
```

## Validation

Current validation covers schema version, entry version, non-empty fields, safe slugs/tags, duplicate slugs, duplicate Onshape source identities, duplicate download formats, preview resolution values, STL/3MF resolution values, STL mode values, non-empty STEP version strings, preset slug rules, override precision limits, supported override widgets, and unknown override parameter IDs once normalized Onshape metadata is available.

Slug rules:

- Lowercase ASCII letters, numbers, and hyphens only.
- Start and end with a letter or number.
- Do not use slugs as immutable cache identity.
- Treat slug renames as URL/display changes; Onshape source identity and hashes remain durable cache identity.

`entryVersion` should increment when catalog settings affect UI validation, parameter defaults, export options, or public presentation. Export-affecting changes should produce new cache identities through option or config hashes rather than overwriting existing public artifacts.

## Parameter Metadata

Parameter metadata is fetched from Onshape and cached in Tigris. SQLite coordinates refresh jobs so duplicate Onshape parameter fetches are not started. The catalog should not duplicate every parameter by hand unless a model needs UI-specific overrides or presets.

Current behavior:

- `previewOptions` and `downloadOptions` are optional.
- Preview quality and download quality are separate catalog settings.
- Preview GLB `resolution` accepts `COARSE`, `MEDIUM`, or `FINE`; the default is `FINE`.
- The default STEP version string is `AP242`.
- STL `downloadOptions.stl.resolution` accepts `coarse`, `medium`, or `fine`; the default is `fine`. Generic async STL resolution did not change tested outputs, so this records requested intent rather than proven tessellation behavior.
- STL `downloadOptions.stl.stlMode` accepts `BINARY` or `TEXT`; the default is `BINARY`.
- 3MF `downloadOptions.3mf.resolution` accepts `coarse`, `medium`, or `fine`; the default is `fine`.
- Export quality controls are catalog/admin-only. Public users choose parameters and download format, not mesh quality.
- Preview and download options participate in cache identity.
- `parameterPresets` supports default, one preset, or all preset pre-generation.
- Preset values are validated against normalized Onshape parameter metadata before previews or downloads are generated.

Current UI overrides are keyed by Onshape parameter ID:

- Public label.
- Description/help text.
- Hidden flag.
- Numeric precision.
- Preferred input widget.

Target override behavior:

- Onshape raw metadata remains the source of truth for parameter IDs and allowed values.
- Catalog overrides may narrow visibility or presentation, but should not expand accepted values beyond Onshape metadata.
- Unknown override parameter IDs fail catalog validation after metadata is available.
- Unsupported Onshape parameter types may be hidden only if they are not required to generate a valid configuration.
- Preview auto-generation policy may become an explicit future field if per-parameter generation policy is needed.

Unsupported or ambiguous parameter types normalize as explicit `unsupported` entries so the UI can surface them and validation can fail before those parameters enter cache identity.
