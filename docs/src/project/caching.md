# Caching

## Goals

- Reduce Onshape API load.
- Make repeated exports fast.
- Avoid duplicate work for identical parameter selections.
- Keep cache state inspectable with a small local coordination database.
- Support later admin rebuild and invalidation workflows.

## Cache Tiers

| Cache | Keyed By | Purpose |
| --- | --- | --- |
| Catalog metadata | Model slug | Curated model information. Initially stored in repo docs/config. |
| Raw Onshape configuration | `did`, `vid`, `eid` | Preserve original Onshape parameter response. |
| Normalized parameter schema | `did`, `vid`, `eid`, schema version | UI-ready form model. |
| Configuration encoding | `did`, `vid`, `eid`, config hash | Cached output of Onshape configuration encoding, if used. |
| GLB preview | `did`, `vid`, `eid`, config hash, preview options hash | Browser 3D preview. |
| Download artifact | `did`, `vid`, `eid`, config hash, format, export options hash | STEP, STL, and 3MF downloads. |
| Manifest | Artifact group id | Source of truth for completed outputs. |
| Job record | Deterministic work key | Status polling and strict deduplication in SQLite. |
| Failure record | Job id or work key plus timestamp | Debugging and retry cooldown. |

## Identity

Onshape IDs identify the source, but not the generated output. The output identity must include parameter values and export settings.

The artifact identity should be derived from:

```text
model slug
element kind: part_studio or assembly
document id
version id
element id
canonical parameter values
requested format or preview format
format-specific options
exporter version
```

Use canonical JSON for parameter values and options before hashing:

- Apply defaults.
- Reject unknown parameters.
- Sort object keys.
- Normalize numeric values according to the catalog's precision rules.
- Include units where they affect the Onshape configuration string.

## Object Storage Layout

Proposed layout:

```text
catalog/models.json
catalog/models/{slug}.json

onshape/{did}/v/{vid}/e/{eid}/configuration.raw.json
onshape/{did}/v/{vid}/e/{eid}/parameters.normalized.json

encodings/{did}/v/{vid}/e/{eid}/{config_hash}.json

previews/{slug}/{vid}/{eid}/{config_hash}/{preview_options_hash}/preview.glb

artifacts/{slug}/{vid}/{eid}/{config_hash}/step/{artifact_id}.step
artifacts/{slug}/{vid}/{eid}/{config_hash}/stl/{artifact_id}.stl
artifacts/{slug}/{vid}/{eid}/{config_hash}/3mf/{artifact_id}.3mf

manifests/{group_id}.json
```

The `group_id` can represent one selected configuration. It links a GLB preview and any generated STEP, STL, or 3MF outputs for that configuration.

Completed public artifacts under `previews/` and `artifacts/` are served directly through stable Tigris URLs. Internal operational objects should stay out of public prefixes, and object listing should not be exposed. Job and failure state lives primarily in SQLite; detailed failure payloads may also be stored in private Tigris prefixes if they are too large for SQLite summaries.

## Manifest

A completed configuration should have a manifest that points to every generated output.

```json
{
  "groupId": "...",
  "modelSlug": "...",
  "elementKind": "part_studio",
  "onshape": {
    "documentId": "...",
    "versionId": "...",
    "elementId": "..."
  },
  "configuration": {
    "hash": "...",
    "values": {}
  },
  "outputs": {
    "previewGlb": "previews/.../preview.glb",
    "step": "artifacts/.../model.step",
    "stl": "artifacts/.../model.stl",
    "3mf": "artifacts/.../model.3mf"
  },
  "createdAt": "...",
  "exporterVersion": "..."
}
```

The manifest is application state. Tigris object metadata is useful but should not be the only source of truth.

## Job Records

SQLite job records are the coordination source of truth.

```json
{
  "groupId": "...",
  "status": "running",
  "requestedOutputs": ["preview_glb", "step"],
  "createdAt": "...",
  "updatedAt": "...",
  "leaseUntil": "...",
  "attempt": 1,
  "error": null
}
```

The `groupId` shown here may be a deterministic work key or may link to a separate group identity for a selected configuration.

States:

- `queued`
- `running`
- `ready`
- `failed`
- `expired`

Use a unique constraint on the deterministic work key so only one job exists for each parameter refresh or export. Workers must claim jobs in short SQLite transactions and must not hold write transactions while calling Onshape.

Workers should still re-check artifact existence before starting expensive Onshape work and before writing final results. This protects recovery paths and manual rebuild workflows, but it is not the primary deduplication mechanism.

Recommended SQLite durability settings for the MVP:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

If SQLite-on-volume constraints become painful, replace it with Postgres before adding multi-machine workers.

## What Not To Cache

- Onshape credentials.
- Temporary Onshape external data URLs.
- Raw bearer tokens or API secrets.
- Workspace current microversion state, since the product only supports versions.
- Transient failures forever. Use retry cooldowns and expiration.
