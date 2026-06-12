# Catalog

## Initial Approach

Start with a curated catalog in the repository, documented and reviewed like code. Do not build an editable catalog system in the first pass.

The catalog should describe only approved Onshape document versions and elements. Catalog entries must be safe for public export because completed artifacts are public and normally immutable. Do not add private, customer-specific, or access-controlled models to the MVP catalog.

## Current Branch Layout

The default implementation loads the versioned catalog index and per-model files:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json
```

Deployments should point `CATALOG_PATH` at `catalog/v1/models.json`.

Current index shape:

```json
{
  "catalogSchemaVersion": 1,
  "models": [
    {
      "slug": "example-model",
      "name": "Example Model",
      "description": "Short public description.",
      "published": true
    }
  ]
}
```

Each model file contains the full model entry. Current validation covers schema version, entry version, non-empty fields, safe slugs/tags, duplicate slugs, duplicate Onshape source identities, duplicate download formats, preview resolution values, non-empty STEP version strings, preset slug rules, override precision limits, supported override widgets, and unknown override parameter IDs once normalized Onshape metadata is available.

## v1 Layout

The catalog uses versioned files:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json
```

`catalog/v1/models.json` is an index of published slugs and lightweight display data. Each `catalog/v1/models/{slug}.json` file is the source of truth for one model entry.

## Model Entry Shape

Implemented shape:

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
      "resolution": "MEDIUM"
    },
    "downloadOptions": {
      "stepVersionString": "AP242"
    }
  },
  "parameterPolicy": {
    "source": "onshape",
    "allowUnknown": false,
    "autoRefresh": true
  },
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

For Assemblies, use:

```json
"elementKind": "assembly"
```

Slug rules:

- Lowercase ASCII letters, numbers, and hyphens only.
- Start and end with a letter or number.
- Do not use slugs as immutable cache identity.
- Treat slug renames as URL/display changes; Onshape source identity and hashes remain the durable cache identity.

`entryVersion` should increment when catalog settings affect UI validation, parameter defaults, export options, or public presentation. Export-affecting changes should produce new cache identities through option or config hashes rather than overwriting existing public artifacts.

## Parameter Metadata

Parameter metadata can be fetched from Onshape and cached in Tigris. SQLite coordinates refresh jobs so duplicate Onshape parameter fetches are not started. The repo catalog should not need to duplicate every parameter by hand unless a model needs UI-specific overrides.

Current branch behavior:

- `previewOptions` and `downloadOptions` are optional.
- The default preview resolution is `MEDIUM`.
- The default STEP version string is `AP242`.
- Preview and download options participate in the current cache identity.
- `parameterPresets` is optional and supports default, one preset, or all preset pre-generation.
- Preset values are validated against normalized Onshape parameter metadata before previews or downloads are generated.

Current UI overrides are keyed by Onshape parameter id:

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

Current branch gap: unsupported parameter types do not carry explicit `unsupportedReason` metadata.

## Later Configurability

Move the catalog out of repo data only when there is a concrete need.

Likely triggers:

- Non-developers need to add or update models.
- Admins need to publish or unpublish entries without deployment.
- Catalog validation needs draft and published states.
- Model ownership or access control becomes more complex.

Possible future storage:

- Tigris JSON catalog for simple no-deploy edits.
- Postgres if catalog data becomes relational or tied to users, quotas, analytics, or billing.
