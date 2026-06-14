# Caching

## Goals

- Reduce Onshape API load.
- Make repeated exports fast.
- Avoid duplicate work for identical parameter selections.
- Keep cache state inspectable with a small local coordination database.
- Support later admin rebuild and invalidation workflows.

## Current Branch Snapshot

This page is retained as historical v1 cache background. The authoritative current design for layered request, response, raw-payload, post-processing, artifact-set, and manifest identity is documented in [Forward-Looking Cache Model](cache-model.md).

The repository now uses a hard-cut v2 cache model. Existing v1 SQLite records, object keys, public URLs, and manifests are not preserved for compatibility.

Where this page describes v1 layouts or behaviors, treat them as historical context rather than the current branch state.

Implemented now:

- SQLite `jobs`, `artifacts`, and `parameter_metadata` tables.
- Unique `jobs.work_key` for deduplicated parameter refresh, preview, and download work.
- RFC 8785 JSON canonicalization for source, configuration, options, and work-key hash preimages.
- Split `sourceHash`, `configHash`, and `optionsHash` helpers for artifact keys, object keys, jobs, and status responses.
- `catalog/v1/models.json` plus `catalog/v1/models/{slug}.json`.
- Versioned object prefixes for parameter metadata, previews, and downloads.
- Tigris/S3 reads and writes for raw parameter metadata, normalized parameter metadata, preview artifacts, and download artifacts.
- Server-side value validation before enqueueing preview or download work.
- Preview storage prefers a single GLB, but direct glTF JSON is accepted as a browser viewer artifact. When Onshape returns a ZIP with exactly one glTF viewer asset instead, the branch publishes that `.gltf` plus sidecars under the same immutable preview identity and retains the original ZIP privately as a raw payload. ZIPs with multiple `.gltf` files are rejected until a real merge path exists. The target cache model preserves Onshape-provided filenames and stores roles in metadata instead of relying on a fixed public ZIP name.
- Ready artifact metadata for public URL, byte length, SHA-256, producing job key, source hash, options hash, and parameter schema version where available.
- Persisted `max_attempts`, `next_retry_at`, and bounded exponential full-jitter retry backoff for worker failures.
- `superseded` job and artifact state for invalidation and pruning without deleting public object-store artifacts.
- Operational listing, retry, invalidation, and pruning commands.

Remaining deviations from the updated plan:

- Public manifests are not part of the initial v2 flow; DB state and status routes are the current source of truth for ready and superseded outputs.
- Failure records are still stored as job summaries only. Stable public-safe error codes and user messages are not implemented.
- Onshape `Retry-After` values are not persisted yet.

## Current v1 Cache Tiers

| Cache | Keyed By | Purpose |
| --- | --- | --- |
| Catalog metadata | Model slug | Curated model information in `catalog/v1/`. |
| Raw Onshape configuration | Source identity | Preserve original Onshape parameter response. |
| Normalized parameter schema | Source identity and schema version | UI-ready form model. |
| Configuration encoding | Source identity and config hash | Cached output of Onshape configuration encoding, if used. |
| Preview artifact | Source identity, config hash, preview options hash | Browser 3D preview, usually GLB but sometimes direct glTF or a single glTF asset set. |
| Download artifact | Source identity, config hash, format, export options hash | STEP, STL, and 3MF downloads. |
| Manifest | Artifact group id | Application state for completed, missing, and superseded outputs. |
| Job record | Deterministic work key | Status polling and strict deduplication in SQLite. |
| Failure record | Job id or work key plus timestamp | Debugging and retry cooldown. Current branch stores only `jobs.error_summary`. |

## Current v1 Identity Model

Onshape IDs identify the immutable source, but not a generated output. Output identity must also include validated parameter values, export settings, and implementation versions that affect bytes.

Current v1 identifiers:

| Identifier | Meaning | Inputs | Storage |
| --- | --- | --- | --- |
| `sourceIdentity` | Immutable Onshape source element. | `documentId`, `versionId`, `elementId`, `elementKind`, optional `linkDocumentId`. | Catalog, manifests, jobs, artifacts. |
| `sourceHash` | Compact hash of `sourceIdentity`. | Canonical `sourceIdentity`. | Object keys, manifests, jobs. |
| `configHash` | Validated selected configuration. | `sourceIdentity`, normalized parameter schema version, canonical parameter values, canonicalization version. | Routes, manifests, jobs, artifacts. |
| `optionsHash` | Preview or export options. | Format and format-specific logical options. | Object keys, manifests, jobs, artifacts. |
| `groupId` | One selected source/configuration group. | `sourceHash`, `configHash`. | Manifest key, status responses. |
| `workKey` | One deduplicated v1 unit of work. | Job kind plus source/config/options identity. | Unique SQLite index. |
| `artifactId` | One immutable v1 produced artifact. | Source/config/options identity plus content hash or deterministic output identity. | Artifact records, object keys. |
| `manifestId` | Manifest for a selected source/configuration group. | Usually `groupId`. | Tigris manifest object and SQLite artifact index. |

Current and target-compatible hashing rules:

- Use SHA-256 encoded as lowercase hex.
- Do not truncate hashes in the MVP.
- Prefix hashable payloads with a domain/version such as `source-v1`, `config-v1`, `options-v1`, or `work-v1`.
- Use RFC 8785 JSON Canonicalization Scheme for JSON hash preimages in the MVP.
- Introduce any non-RFC-8785 canonicalization only as a separate versioned algorithm with documented rules and golden test vectors before use.
- Hash typed, validated parameter values after defaults and catalog overrides are applied.
- Include units where they affect the Onshape configuration string.
- Do not rely on `slug` for immutable identity. Slugs are useful for URLs and display, but source IDs and hashes own uniqueness.

Current branch status: v2 export deduplication uses `requestHash`, source identity is resolved through immutable microversions, configuration encodings are built from typed canonical values, retained raw payloads are verified before reuse, and artifact readiness is modeled through `rawPayloadHash`, `postprocessHash`, and `artifactSetHash`. See [Forward-Looking Cache Model](cache-model.md).

## Object Storage Layout

Current branch layout:

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json

onshape/v1/{source_hash}/configuration.raw.json
onshape/v1/{source_hash}/parameters.normalized/schema-v{schema_version}.json

previews/v1/{source_hash}/{config_hash}/{options_hash}/preview.glb
previews/v1/{source_hash}/{config_hash}/{options_hash}/preview.gltf
previews/v1/{source_hash}/{config_hash}/{options_hash}/{onshape_gltf_assets}
previews/v1/{source_hash}/{config_hash}/{options_hash}/source.zip

artifacts/v1/{source_hash}/{config_hash}/step/{options_hash}/{artifact_id}.step
artifacts/v1/{source_hash}/{config_hash}/stl/{options_hash}/{artifact_id}.stl
artifacts/v1/{source_hash}/{config_hash}/3mf/{options_hash}/{artifact_id}.3mf

manifests/v1/{group_id}.json
```

For the current v1 implementation track, the logical target layout remains the same. The forward-looking v2 cache model replaces fixed raw-payload names such as `source.zip` with preserved Onshape filenames and metadata roles.

```text
catalog/v1/models.json
catalog/v1/models/{slug}.json

onshape/v1/{source_hash}/configuration.raw.json
onshape/v1/{source_hash}/parameters.normalized/{parameter_schema_hash}.json

encodings/v1/{source_hash}/{config_hash}.json

previews/v1/{source_hash}/{config_hash}/{options_hash}/preview.glb
previews/v1/{source_hash}/{config_hash}/{options_hash}/preview.gltf
previews/v1/{source_hash}/{config_hash}/{options_hash}/{onshape_gltf_assets}
previews/v1/{source_hash}/{config_hash}/{options_hash}/source.zip

artifacts/v1/{source_hash}/{config_hash}/step/{options_hash}/{artifact_id}.step
artifacts/v1/{source_hash}/{config_hash}/stl/{options_hash}/{artifact_id}.stl
artifacts/v1/{source_hash}/{config_hash}/3mf/{options_hash}/{artifact_id}.3mf

manifests/v1/{group_id}.json
```

The `groupId` represents one selected source/configuration. In v1 it links a preview artifact and any generated STEP, STL, or 3MF outputs for that configuration.

Completed public artifacts under `previews/` and `artifacts/` are served directly through stable Tigris URLs. Internal operational objects should stay out of public prefixes, and object listing should not be exposed. Job and failure state lives primarily in SQLite; detailed failure payloads may also be stored in private Tigris prefixes if they are too large for SQLite summaries.

Public artifacts are immutable in normal operation. Cache invalidation means writing a new object key and updating manifests or SQLite index state to mark older artifacts as superseded. Do not overwrite public GLB, STEP, STL, or 3MF objects in place. Delete public artifacts only for explicit operator cleanup, legal or IP concerns, or storage-cost management.

## Data Contracts

The exact Rust types can evolve during implementation. The examples below describe the current/v1 contract and near-term gaps; v2 target schemas live in [Forward-Looking Cache Model](cache-model.md).

### Normalized Parameter Metadata

Current v1 direction:

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

Current branch status: normalized metadata keeps the implemented `ParameterSchema` shape, and unsupported or ambiguous parameter shapes are represented explicitly through `kind: unsupported` so validation fails before those parameters can enter `configHash`.

### Canonical Configuration Payload

Current v1 direction:

```json
{
  "canonicalizationVersion": 1,
  "sourceIdentity": {},
  "parameterSchemaVersion": 1,
  "values": {
    "parameter-id": {
      "kind": "quantity",
      "dimension": "length",
      "unit": "m",
      "numerator": "1",
      "denominator": "40"
    }
  }
}
```

The browser may submit parameter values, but the server must validate, apply defaults, canonicalize, and compute `configHash` before trusting any status or enqueue request.

Current branch status: server-side validation, typed canonical values, RFC 8785 canonicalization, canonical Onshape encoding-request projections, and split source/config/options hashes are all implemented. Supported length values normalize to exact rational meters before `configHash` and encoding reuse. Unitless numbers also use exact rational canonical values. Angle values accept `deg` and `rad` but do not canonicalize across those units yet.

### Manifest

A completed v1 configuration should have a manifest that points to every generated output.

Current v1 shape:

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
    "preview": {
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

The manifest is application state. Tigris object metadata is useful but should not be the only source of truth. In v2, initial public flow is expected to be database/status-route driven, with object-store manifests deferred as a future materialization of database state.

Current branch status: v2 status and product behavior are driven from database state rather than object-store manifests. Ready outputs still track public URL, SHA-256, producing job key, source/options hashes, stored configuration values, and byte length.

### Artifact Records

Current v1 ready artifact metadata:

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
  "contentDisposition": "attachment; filename=\"model.step\"",
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

Current branch status: SQLite artifact records include `artifact_key`, `model_slug`, `config_hash`, `output_kind`, `status`, `object_key`, `content_type`, `byte_len`, `sha256`, `producing_job_key`, `source_hash`, `options_hash`, `parameter_schema_version`, `config_values_json`, `created_at`, and `superseded_at`. Replacement pointers and supersession reasons are not yet stored. v2 replaces single artifact records with artifact sets and artifact files.

## Artifact Write Ordering

Expected artifact write ordering:

1. Upload the public artifact object under a new immutable key.
2. Verify the uploaded object is readable or can be inspected through object metadata.
3. Write or update the DB artifact state for the uploaded output.
4. Mark the corresponding SQLite job ready.

Recovery and reconciliation commands should handle partial writes, such as an uploaded object whose SQLite job was not marked ready before a restart.

Current branch status: upload verification, artifact publishing, and job-ready marking exist. Reconciliation for partial writes remains future work.

## Invalidation And Eviction

Normal invalidation must supersede rather than delete:

- Write a new artifact object under a new immutable key.
- Mark the old artifact record as `superseded` with a reason and replacement pointer when available.
- Update DB-backed status responses to point to the new ready artifact or show the old artifact as superseded.
- Keep public object deletion for explicit operator cleanup, legal/IP concerns, or storage-cost management.

Current branch status: `artifacts invalidate` and `artifacts prune` mark ready artifact records as `superseded`, preserve public object-store artifacts, and mark producing ready jobs superseded when known. A future explicit destructive cleanup command can handle legal/IP removal or storage-cost deletion separately.

## Job Records

SQLite job records are the current v1 coordination source of truth.

Current v1 planned shape:

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

Planned job kinds:

- `parameter_refresh`
- `configuration_encoding`
- `preview_export`
- `download_export`

Current branch job kinds:

- `parameter_refresh`
- `preview_export`
- `download_export`

Planned states:

- `queued`
- `running`
- `ready`
- `failed`
- `superseded`

Current branch states:

- `queued`
- `running`
- `ready`
- `failed`
- `superseded`

Expired leases are reclaimed by selecting `running` jobs whose `lease_until` timestamp has passed; `expired` is not a stored state.

Planned transitions:

- `queued -> running` when a worker claims the job lease.
- `running -> ready` after artifacts, manifests, and artifact index records are written.
- `running -> failed` after a terminal Onshape failure, upload failure, validation failure, or max attempts.
- `running -> queued` when the lease expires and attempts remain.
- `failed -> queued` only through explicit retry or retry cooldown policy.
- `ready -> superseded` when newer artifact identity replaces it in normal invalidation.

Validation should happen before enqueue, so terminal validation failures that are discovered later happen after a worker claim and use `running -> failed`. Active work should not move directly from `running -> superseded`; it should finish, fail, or return to `queued` on lease expiry. Workers must re-check artifact and index state before expensive Onshape work so obsolete queued or running work can exit without producing duplicate public artifacts.

Use a unique constraint on the deterministic `workKey` so only one job exists for each parameter refresh or export. Workers must claim jobs in short SQLite transactions and must not hold write transactions while calling Onshape.

Current v1 docs planned to store Onshape translation IDs and polling state in the job row so a restarted worker can resume polling instead of starting duplicate translations. In v2, `requestHash` deduplicates exact export requests, `translationId` records one Onshape attempt, and `runId` records one local worker execution.

Current branch gap: Onshape export calls run as one worker operation after claiming a job, but translation IDs and polling state are not persisted separately for crash recovery.

## Failure Records

Current planned shape:

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

Current branch gap: there is no separate failure table or failure class. Failed jobs store only a public summary string on the job row.

## Retry Policy

Start conservatively:

- Validation and unsupported-parameter failures are not retryable without input or catalog changes.
- Onshape rate limits, Onshape server errors, translation timeouts, external data download failures, and Tigris upload failures are retryable.
- Use `maxAttempts = 3` total attempts for the MVP.
- Retry with bounded exponential backoff starting at 30 seconds and capped at 5 minutes, using full jitter by scheduling a random delay from zero to the computed delay.
- Honor valid Onshape `Retry-After` guidance when it is longer than the computed delay.
- Persist the selected retry time in `nextRetryAt`; public status responses should derive `retryAfterSeconds` from that value.
- Public status responses should expose safe `errorCode`, `userMessage`, and `retryAfterSeconds` fields, not raw internal failure details.

Current branch status: worker failures automatically retry up to three attempts with bounded exponential full-jitter backoff stored in `nextRetryAt`, and operators can still manually retry failed jobs. Retryability classification, Onshape `Retry-After`, derived `retryAfterSeconds`, and stable public-safe error codes/messages remain TODO.

## SQLite Settings

Recommended SQLite durability settings for the MVP:

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

These PRAGMAs are applied by the current branch. The transaction constraint is equally important: no database write transaction may remain open across Onshape polling, external data download, or Tigris upload.

If SQLite-on-volume constraints become painful, replace it with Postgres before adding multi-machine workers.

## What Not To Cache

- Onshape credentials.
- Temporary Onshape external data URLs.
- Raw bearer tokens or API secrets.
- Workspace current microversion state, since the product only supports versions.
- Transient failures forever. Use retry cooldowns and expiration.
