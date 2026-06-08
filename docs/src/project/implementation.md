# Implementation Plan

## Phase 0: Documentation And Decisions

Current phase.

Outputs:

- Capture architecture and runtime options.
- Capture Onshape API flow.
- Capture Tigris cache contract.
- Capture SQLite queue and coordination direction.
- Capture frontend preview behavior.
- Capture admin and catalog direction.

## Phase 1: Project Skeleton

Set up the Rust project without implementing every feature.

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

Expected pieces:

- Fetch raw configuration metadata for Part Studios and Assemblies.
- Normalize metadata for UI rendering.
- Enqueue parameter refreshes through SQLite so duplicate Onshape calls are not started.
- Store raw and normalized parameter metadata in Tigris.
- Render a model page with controls.
- Validate submitted parameter values against normalized metadata.

## Phase 3: Preview Path

Implement GLB preview generation and display.

Expected pieces:

- Compute canonical `config_hash`.
- Check Tigris and SQLite artifact records for cached preview.
- Request Onshape GLB export on cache miss.
- Poll translation and download external data.
- Store GLB in Tigris.
- Return stable public Tigris preview URLs.
- Show cached GLB with `<model-viewer>`.

## Phase 4: Download Exports

Implement STEP, STL, and 3MF downloads.

Expected pieces:

- Format-specific export option defaults.
- Cache key and manifest entries per format.
- Onshape export and polling for each format.
- Tigris upload, content type, content disposition, and cache headers.
- Stable public Tigris download links after completion.

## Phase 5: Operational Commands

Add CLI or Fly-run maintenance controls. Do not add a web admin UI until browser-based admin workflows are needed.

Expected pieces:

- Catalog validation.
- Parameter refresh.
- Preview/export pre-generation.
- Failure inspection.
- Retry and invalidate actions.
- Operational access through local CLI credentials or Fly access.

## Phase 6: Runtime Hardening

Add robustness only as needed.

Possible additions:

- Separate Fly worker process group.
- Postgres if SQLite-on-volume limits become painful.
- Web admin UI and authentication if CLI operations become insufficient.
- Scheduled rebuilds.
- Metrics and tracing.
- Cache eviction policies.
