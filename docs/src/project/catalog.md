# Catalog

## Initial Approach

Start with a curated catalog in the repository, documented and reviewed like code. Do not build an editable catalog system in the first pass.

The catalog should describe only approved Onshape document versions and elements.

Catalog entries must be safe for public export. Because completed artifacts are public and normally immutable, do not add private, customer-specific, or access-controlled models to the MVP catalog.

Initial files:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json
```

`catalog/v1/models.json` is an index of published slugs and lightweight display data. Each `catalog/v1/models/{slug}.json` file is the source of truth for one model entry.

## Model Entry

Each model entry should include:

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
    "preview": {
      "format": "glb",
      "defaults": {
        "meshResolution": "MEDIUM"
      }
    },
    "downloads": {
      "step": {
        "enabled": true,
        "defaults": {
          "stepVersion": "AP242"
        }
      },
      "stl": {
        "enabled": true,
        "defaults": {}
      },
      "3mf": {
        "enabled": true,
        "defaults": {}
      }
    }
  },
  "parameterPolicy": {
    "source": "onshape",
    "allowUnknown": false,
    "autoRefresh": false
  },
  "parameterOverrides": {
    "parameter-id": {
      "label": "Public Label",
      "description": "Short help text.",
      "visible": true,
      "precision": 3,
      "widget": "number",
      "previewAutoGenerate": false
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
- Start with a letter or number.
- Do not use slugs as immutable cache identity.
- Treat slug renames as URL/display changes; Onshape source identity and hashes remain the durable cache identity.

`entryVersion` should increment when catalog settings affect UI validation, parameter defaults, export options, or public presentation. Export-affecting changes should produce new cache identities through option or config hashes rather than overwriting existing public artifacts.

## Parameter Metadata

Parameter metadata can be fetched from Onshape and cached in Tigris. SQLite coordinates refresh jobs so duplicate Onshape parameter fetches are not started. The repo catalog should not need to duplicate every parameter by hand unless a model needs UI-specific overrides.

Possible overrides:

- Public label.
- Description/help text.
- Visibility.
- Numeric precision.
- Preferred input widget.
- Preview auto-generation policy.
- Export option defaults.

Override merge rules:

- Onshape raw metadata remains the source of truth for parameter IDs and allowed values.
- Catalog overrides may narrow visibility or presentation, but should not expand accepted values beyond Onshape metadata.
- Unknown override parameter IDs fail catalog validation.
- Unsupported Onshape parameter types may be hidden only if they are not required to generate a valid configuration.

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
