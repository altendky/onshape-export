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
| Catalog metadata | Model slug | Curated model information. Initially stored in `catalog/v1/`. |
| Raw Onshape configuration | `did`, `vid`, `eid` | Preserve original Onshape parameter response. |
| Normalized parameter schema | `did`, `vid`, `eid`, schema version | UI-ready form model. |
| Configuration encoding | `did`, `vid`, `eid`, config hash | Cached output of Onshape configuration encoding, if used. |
| GLB preview | `did`, `vid`, `eid`, config hash, preview options hash | Browser 3D preview. |
| Download artifact | `did`, `vid`, `eid`, config hash, format, export options hash | STEP, STL, and 3MF downloads. |
| Manifest | Artifact group id | Application state for completed and superseded outputs. |
| Job record | Deterministic work key | Status polling and strict deduplication in SQLite. |
| Failure record | Job id or work key plus timestamp | Debugging and retry cooldown. |

## Identity Model

Onshape IDs identify the immutable source, but not a generated output. Output identity must also include validated parameter values, export settings, and implementation versions that affect bytes.

Use these identifiers consistently in routes, object keys, manifests, jobs, and admin commands:

| Identifier | Meaning | Inputs | Storage |
| --- | --- | --- | --- |
| `sourceIdentity` | Immutable Onshape source element. | `documentId`, `versionId`, `elementId`, `elementKind`, optional `linkDocumentId`. | Catalog, manifests, jobs, artifacts. |
| `sourceHash` | Compact hash of `sourceIdentity`. | Canonical `sourceIdentity`. | Object keys, manifests, jobs. |
| `configHash` | Validated selected configuration. | `sourceIdentity`, normalized parameter schema version, canonical parameter values, canonicalization version. | Routes, manifests, jobs, artifacts. |
| `optionsHash` | Preview or export options. | Format, format-specific options, exporter version. | Object keys, manifests, jobs, artifacts. |
| `groupId` | One selected source/configuration group. | `sourceHash`, `configHash`. | Manifest key, status responses. |
| `workKey` | One deduplicated unit of work. | Job kind plus source/config/options identity. | Unique SQLite index. |
| `artifactId` | One immutable produced artifact. | Source/config/options identity plus content hash or deterministic output identity. | Artifact records, object keys. |
| `manifestId` | Manifest for a selected source/configuration group. | Usually `groupId`. | Tigris manifest object and SQLite artifact index. |

Hashing rules:

- Use SHA-256 encoded as lowercase hex.
- Do not truncate hashes in the MVP.
- Prefix hashable payloads with a domain/version such as `source-v1`, `config-v1`, `options-v1`, or `work-v1`.
- Use RFC 8785 JSON Canonicalization Scheme for JSON hash preimages in the MVP. Any non-RFC-8785 canonicalization must be introduced as a separate versioned algorithm with documented rules and golden test vectors before use.
- Hash typed, validated parameter values after defaults and catalog overrides are applied.
- Include units where they affect the Onshape configuration string.
- Do not rely on `slug` for immutable identity. Slugs are useful for URLs and display, but source IDs and hashes own uniqueness.

## Object Storage Layout

Proposed layout:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json

onshape/v1/{source_hash}/configuration.raw.json
onshape/v1/{source_hash}/parameters.normalized/{parameter_schema_hash}.json

encodings/v1/{source_hash}/{config_hash}.json

previews/v1/{source_hash}/{config_hash}/{options_hash}/preview.glb

artifacts/v1/{source_hash}/{config_hash}/step/{options_hash}/{artifact_id}.step
artifacts/v1/{source_hash}/{config_hash}/stl/{options_hash}/{artifact_id}.stl
artifacts/v1/{source_hash}/{config_hash}/3mf/{options_hash}/{artifact_id}.3mf

manifests/v1/{group_id}.json
```

The `group_id` can represent one selected configuration. It links a GLB preview and any generated STEP, STL, or 3MF outputs for that configuration.

Completed public artifacts under `previews/` and `artifacts/` are served directly through stable Tigris URLs. Internal operational objects should stay out of public prefixes, and object listing should not be exposed. Job and failure state lives primarily in SQLite; detailed failure payloads may also be stored in private Tigris prefixes if they are too large for SQLite summaries.

Public artifacts are immutable in normal operation. Cache invalidation means writing a new object key and updating manifests or SQLite index state to mark older artifacts as superseded. Do not overwrite public GLB, STEP, STL, or 3MF objects in place. Delete public artifacts only for explicit operator cleanup, legal or IP concerns, or storage-cost management.

## Data Contracts

The exact Rust types can evolve during implementation, but the MVP should preserve these logical contracts.

### Normalized Parameter Metadata

```json
{
  "schemaVersion": 1,
  "sourceIdentity": {},
  "rawObjectKey": "onshape/v1/.../configuration.raw.json",
  "parameters": [
    {
      "parameterId": "...",
      "name": "Length",
      "label": "Length",
      "type": "quantity",
      "defaultValue": { "kind": "quantity", "value": "25 mm" },
      "units": "mm",
      "min": "5 mm",
      "max": "100 mm",
      "step": "1 mm",
      "precision": 3,
      "visible": true,
      "required": true,
      "unsupportedReason": null
    }
  ],
  "createdAt": "..."
}
```

Unsupported Onshape parameter types should remain visible in metadata with `unsupportedReason` so the UI can explain why a model or parameter cannot be exported yet.

### Canonical Configuration Payload

```json
{
  "canonicalizationVersion": 1,
  "sourceIdentity": {},
  "parameterSchemaVersion": 1,
  "values": {
    "parameter-id": { "kind": "quantity", "value": "25 mm" }
  }
}
```

The browser may submit parameter values, but the server must validate, apply defaults, canonicalize, and compute `configHash` before trusting any status or enqueue request.

### Manifest

A completed configuration should have a manifest that points to every generated output.

```json
{
  "groupId": "...",
  "modelSlug": "...",
  "manifestSchemaVersion": 1,
  "onshape": {
    "documentId": "...",
    "versionId": "...",
    "elementId": "...",
    "elementKind": "part_studio",
    "linkDocumentId": null
  },
  "configuration": {
    "hash": "...",
    "values": {}
  },
  "outputs": {
    "previewGlb": {
      "objectKey": "previews/.../preview.glb",
      "publicUrl": "https://...",
      "status": "ready",
      "contentType": "model/gltf-binary",
      "sizeBytes": 123,
      "sha256": "...",
      "jobId": "..."
    },
    "step": {
      "objectKey": "artifacts/.../model.step",
      "publicUrl": "https://...",
      "status": "ready",
      "contentType": "model/step",
      "sizeBytes": 456,
      "sha256": "...",
      "jobId": "..."
    },
    "stl": {
      "objectKey": "artifacts/.../model.stl",
      "status": "superseded"
    },
    "3mf": {
      "objectKey": "artifacts/.../model.3mf",
      "status": "missing"
    }
  },
  "createdAt": "...",
  "exporterVersion": "..."
}
```

The manifest is application state. Tigris object metadata is useful but should not be the only source of truth.

Expected artifact write ordering:

1. Upload the public artifact object under a new immutable key.
2. Verify the uploaded object is readable or can be inspected through object metadata.
3. Write or update the manifest and artifact index.
4. Mark the corresponding SQLite job ready.

Recovery and reconciliation commands should handle partial writes, such as an uploaded object whose SQLite job was not marked ready before a restart.

### Job Records

SQLite job records are the coordination source of truth.

```json
{
  "jobId": "...",
  "workKey": "...",
  "groupId": "...",
  "kind": "preview_export",
  "status": "running",
  "sourceIdentity": {},
  "configHash": "...",
  "format": "glb",
  "optionsHash": "...",
  "onshapeTranslationId": "...",
  "onshapeState": "ACTIVE",
  "lastPollAt": "...",
  "nextPollAt": "...",
  "createdAt": "...",
  "updatedAt": "...",
  "leaseOwner": "...",
  "leaseUntil": "...",
  "attempt": 1,
  "maxAttempts": 3,
  "nextRetryAt": null,
  "error": null
}
```

Job kinds:

- `parameter_refresh`
- `configuration_encoding`
- `preview_export`
- `download_export`

States:

- `queued`
- `running`
- `ready`
- `failed`
- `superseded`

Valid transitions:

- `queued -> running` when a worker claims the job lease.
- `running -> ready` after artifacts, manifests, and artifact index records are written.
- `running -> failed` after a terminal Onshape failure, upload failure, validation failure, or max attempts.
- `running -> queued` when the lease expires and attempts remain.
- `failed -> queued` only through explicit retry or retry cooldown policy.
- `ready -> superseded` when newer artifact identity replaces it in normal invalidation.

These are the normal MVP job transitions. Validation should happen before enqueue, so terminal validation failures that are discovered later happen after a worker claim and use `running -> failed`. Active work should not move directly from `running -> superseded`; it should finish, fail, or return to `queued` on lease expiry. Workers must re-check artifact and index state before expensive Onshape work so obsolete queued or running work can exit without producing duplicate public artifacts. The unique `workKey` constraint means retries reuse the existing job row; rebuilds that need distinct work must change the deterministic identity inputs or use explicit invalidation/retry flows.

Use a unique constraint on the deterministic work key so only one job exists for each parameter refresh or export. Workers must claim jobs in short SQLite transactions and must not hold write transactions while calling Onshape.

Workers should still re-check artifact existence before starting expensive Onshape work and before writing final results. This protects recovery paths and manual rebuild workflows, but it is not the primary deduplication mechanism.

Store Onshape translation IDs and polling state in the job row so a restarted worker can resume polling instead of starting duplicate translations.

### Failure Records

```json
{
  "failureId": "...",
  "jobId": "...",
  "workKey": "...",
  "attempt": 1,
  "errorClass": "onshape_rate_limit",
  "statusCode": 429,
  "message": "Public-safe summary.",
  "detailsObjectKey": "failures/...json",
  "retryable": true,
  "createdAt": "..."
}
```

Initial failure classes:

- `validation`
- `unsupported_parameter`
- `onshape_auth`
- `onshape_rate_limit`
- `onshape_server_error`
- `translation_failed`
- `translation_timeout`
- `external_data_download`
- `tigris_upload`
- `sqlite_write`
- `worker_crash_recovery`

### Artifact Records

```json
{
  "artifactId": "...",
  "sourceHash": "...",
  "configHash": "...",
  "kind": "download",
  "format": "step",
  "optionsHash": "...",
  "objectKey": "artifacts/v1/.../model.step",
  "publicUrl": "https://...",
  "contentType": "model/step",
  "sizeBytes": 123,
  "sha256": "...",
  "manifestId": "...",
  "producingJobId": "...",
  "createdAt": "...",
  "supersededAt": null,
  "supersededBy": null,
  "supersessionReason": null
}
```

### Retry Policy

Start conservatively:

- Validation and unsupported-parameter failures are not retryable without input or catalog changes.
- Onshape rate limits, Onshape server errors, translation timeouts, external data download failures, and Tigris upload failures are retryable.
- Use bounded exponential backoff with a small max attempt count, such as three attempts, until real Onshape behavior is measured.
- Public status responses should expose safe `errorCode`, `userMessage`, and `retryAfterSeconds` fields, not raw internal failure details.

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
