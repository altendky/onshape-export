# Catalog

## Initial Approach

Start with a curated catalog in the repository, documented and reviewed like code. Do not build an editable catalog system in the first pass.

The catalog should describe only approved Onshape document versions and elements.

## Model Entry

Each model entry should include:

```json
{
  "slug": "example-model",
  "name": "Example Model",
  "description": "Short public description.",
  "onshape": {
    "documentId": "...",
    "versionId": "...",
    "elementId": "...",
    "elementKind": "part_studio"
  },
  "exports": {
    "downloads": ["step", "stl", "3mf"],
    "preview": "glb"
  },
  "parameterPolicy": {
    "source": "onshape",
    "allowUnknown": false
  }
}
```

For Assemblies, use:

```json
"elementKind": "assembly"
```

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
