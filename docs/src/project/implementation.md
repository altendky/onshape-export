# Implementation Plan

## Phase 0: Documentation And Decisions

Completed.

Outputs:

- Capture architecture and runtime options.
- Capture Onshape API flow.
- Capture Tigris cache contract.
- Capture SQLite queue and coordination direction.
- Capture frontend preview behavior.
- Capture admin and catalog direction.

## Phase 1: Project Skeleton

Set up the Rust project without implementing every feature.

Completed.

Expected pieces:

- Rust workspace or single crate decision.
- `axum` service skeleton.
- Configuration and secret loading.
- Health endpoint.
- Tigris/S3-compatible client configuration.
- SQLite connection, migrations, and robust PRAGMA setup.
- Onshape client skeleton.
- In-repo catalog representation.
- Basic template/static UI direction.

## Phase 2: Onshape Read Path

Implement parameter discovery.

Current phase foundation implemented.

Expected pieces:

- Fetch raw configuration metadata for Part Studios and Assemblies.
- Normalize metadata for UI rendering.
- Enqueue parameter refreshes through SQLite so duplicate Onshape calls are not started.
- Store raw and normalized parameter metadata in Tigris.
- Render a model page with controls.
- Validate submitted parameter values against normalized metadata.

Needs live Onshape verification:

- Verify normalized schema against real target model responses.

Implemented hardening:

- UI-specific catalog `parameterOverrides` support public labels, help text,
  hidden parameters, numeric precision, and preferred basic widgets.
- Parameter refreshes run through the SQLite-backed worker path instead of
  running inline from request handlers.

## Phase 3: Preview Path

Implement GLB preview generation and display.

Current phase foundation implemented.

Expected pieces:

- Compute canonical `config_hash`.
- Check Tigris and SQLite artifact records for cached preview.
- Request Onshape GLB export on cache miss.
- Poll translation and download external data.
- Store GLB in Tigris.
- Return stable public Tigris preview URLs.
- Show cached GLB with `<model-viewer>`.

Needs live Onshape verification:

- Verify GLB export request and translation response shapes against real Onshape models.

Implemented hardening:

- Preview generation pages poll deterministic JSON status endpoints until the
  worker produces an artifact or failure.
- Catalog `previewOptions.resolution` allows model-specific GLB tessellation
  tuning, and preview options participate in cache identity.

## Phase 4: Download Exports

Implement STEP, STL, and 3MF downloads.

Current phase foundation implemented.

Expected pieces:

- Format-specific export option defaults.
- Cache key and manifest entries per format.
- Onshape export and polling for each format.
- Tigris upload, content type, content disposition, and cache headers.
- Stable public Tigris download links after completion.

Needs live Onshape verification:

- Verify STEP, STL, and 3MF export request and translation response shapes against real Onshape models.

Implemented hardening:

- Successful generation, invalidation, and manifest rewrite commands materialize
  manifests from SQLite artifact records.
- Download generation pages poll deterministic JSON status endpoints until the
  worker produces an artifact or failure.
- Catalog `downloadOptions.stepVersionString` allows model-specific STEP
  defaults, and download options participate in cache identity.

## Phase 5: Operational Commands

Add CLI or Fly-run maintenance controls. Do not add a web admin UI until browser-based admin workflows are needed.

Current phase foundation implemented.

Expected pieces:

- Catalog validation.
- Parameter refresh.
- Preview/export pre-generation.
- Failure inspection.
- Retry and invalidate actions.
- Operational access through local CLI credentials or Fly access.

Remaining hardening:

- Add object-store deletion or tombstone handling if invalidation should remove Tigris objects, not only SQLite artifact records.

Implemented hardening:

- Add catalog-defined `parameterPresets` and CLI selectors for default, one preset, or all parameter sets during preview/export pre-generation.
- Add structured JSON output for `failures list` and `artifacts list` operational commands.
- Add `jobs list` with text and JSON output for recent queued, running, ready,
  and failed jobs.
- `artifacts invalidate` now deletes the object-store artifact before removing
  the SQLite artifact record.
- Successful preview/export generation now rewrites a Tigris manifest for the
  selected model configuration from SQLite artifact records; invalidation
  rewrites the same manifest after deleting the artifact record.
- `artifacts manifest` renders a SQLite-derived manifest for one model
  configuration and can rewrite that manifest to Tigris with `--rewrite`.
- `failures retry` supports targeted retries by work key or job kind, while
  preserving all-failures retry as the default.
- `jobs list` exposes recent queued, running, ready, and failed job state with
  text and JSON output for operational inspection.

## Phase 6: Runtime Hardening

Add robustness only as needed.

Current phase foundation implemented.

Implemented pieces:

- Prometheus-style `/metrics` route with catalog, job, artifact, and artifact-byte gauges.
- Request handlers enqueue parameter, preview, and download jobs for a background worker instead of running Onshape calls inline.
- SQLite job leases allow queued or expired work to be claimed without starting duplicate work.
- Expired running job leases are reclaimable, and job completion is fenced by claim attempt to avoid stale worker status updates.
- Worker-only runtime mode through `onshape-export worker`, plus `WORKER_ENABLED=false` for web-only `serve` processes.
- Explicit worker concurrency through `WORKER_CONCURRENCY`, defaulting to one claimed job at a time for the MVP.
- Opt-in scheduled rebuilds through `REBUILD_INTERVAL_SECONDS` enqueue catalog parameter refreshes and missing default artifacts from the worker runtime.
- Preview and download generation pages expose deterministic JSON status endpoints and poll queued jobs until the artifact is ready or failed.
- Age-based cache eviction is available through `artifacts prune`, deleting
  matching object-store artifacts and rewriting affected manifests.
- Operator-triggered SQLite snapshots are available through `ops backup <destination.db>`.
- Fly deployment scaffolding is available through `Dockerfile`, `.dockerignore`,
  and `fly.toml`, using a single machine with the in-process worker enabled so
  SQLite coordination stays on one mounted volume.
- `ops check` validates deployment-critical configuration before serving
  traffic: catalog loading, SQLite connectivity, Tigris client construction,
  Tigris public URL configuration, and Onshape/Tigris credential presence.
- Artifact cache identity now includes the exporter package version plus the
  preview/download option version strings, so option or code changes naturally
  generate fresh cache entries instead of reusing stale exports.
- Catalog validation rejects unsafe slugs, duplicate download formats, invalid
  preview resolutions, invalid parameter override precision, and unsupported
  parameter override widgets.
- Catalog-defined preview and download option values are included in cache
  identity, so model-specific tuning produces fresh artifact sets.

Possible additions:

- Postgres if SQLite-on-volume limits become painful.
- Web admin UI and authentication if CLI operations become insufficient.
