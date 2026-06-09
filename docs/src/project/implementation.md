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

Remaining hardening:

- Verify normalized schema against real target model responses.
- Add UI-specific catalog overrides if models need labels, units, precision, or visibility changes.
- Decide whether refreshes should stay request-driven or move to a worker loop.

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

Remaining hardening:

- Verify GLB export request and translation response shapes against real Onshape models.
- Add richer preview status polling instead of returning a simple refresh message while request-driven generation runs.
- Add model-specific preview option overrides if tessellation, orientation, or grouping settings need to vary.

## Phase 4: Download Exports

Implement STEP, STL, and 3MF downloads.

Current phase foundation implemented.

Expected pieces:

- Format-specific export option defaults.
- Cache key and manifest entries per format.
- Onshape export and polling for each format.
- Tigris upload, content type, content disposition, and cache headers.
- Stable public Tigris download links after completion.

Remaining hardening:

- Verify STEP, STL, and 3MF export request and translation response shapes against real Onshape models.
- Add manifest materialization if SQLite artifact records are not sufficient for operational inspection.
- Add richer download status polling instead of returning a simple refresh message while request-driven generation runs.

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

- Add richer selectors for targeted parameter sets beyond default values.
- Add object-store deletion or tombstone handling if invalidation should remove Tigris objects, not only SQLite artifact records.
- Add structured output formats if operational automation needs JSON instead of tab-delimited text.

## Phase 6: Runtime Hardening

Add robustness only as needed.

Current phase foundation implemented.

Implemented pieces:

- Prometheus-style `/metrics` route with catalog, job, artifact, and artifact-byte gauges.
- Request handlers enqueue parameter, preview, and download jobs for a background worker instead of running Onshape calls inline.
- SQLite job leases allow queued or expired work to be claimed without starting duplicate work.

Possible additions:

- Separate Fly worker process group or worker-only runtime mode.
- Postgres if SQLite-on-volume limits become painful.
- Web admin UI and authentication if CLI operations become insufficient.
- Scheduled rebuilds.
- Cache eviction policies.
