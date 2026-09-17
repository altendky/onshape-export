# Architecture

## Proposed Shape

The recommended MVP shape stays on Fly/Tigris:

```text
Browser
  |
  | public pages, cache checks, status, downloads, preview artifact set
  v
Fly.io Rust axum app
  |\
  | \ stable public artifact URLs, metadata reads/writes
  |  v
  | Tigris Object Storage
  |  ^
  |  | artifact uploads, cached metadata
  |
  | live catalog, queue, job uniqueness, status, artifact index
  v
SQLite on Fly volume
  ^
  |
Worker loop in Fly app
  |
  | Onshape API calls, translation polling, external data download
  v
Onshape API
```

The public site should be able to serve cached content even when no export job is running. The Rust service owns catalog validation, queue submission, status routes, Onshape calls, translation polling, and Tigris uploads. SQLite owns live catalog data and transactional coordination so duplicate Onshape work is not started for the same deterministic key.

Future slicer project 3MF generation is proposed as a trusted external CLI step
after raw geometry retention. The three separately versioned Bambu Studio,
OrcaSlicer, and PrusaSlicer CLI generators in
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators)
would accept source-neutral geometry and return unvalidated candidate artifacts.
The service would retain ownership of orchestration, validation, cache identity,
and publication. The approved CLI is trusted like service code; the process
boundary preserves repository, source-ingress, provenance, release,
distribution, and license responsibilities, defines a source-neutral interface,
and does not provide runtime security isolation. See
[Slicer Project Generators](slicer-project-generators.md) for the proposed
boundary and unresolved details.

## Components

| Component | Responsibility |
| --- | --- |
| Public UI | Catalog browsing, parameter forms, preview viewer, export requests. |
| Fly Rust app | Public routing, validation, cache checks, queue submission, status routes, Onshape orchestration, Tigris writes. |
| SQLite on Fly volume | Live catalog rows, publication state, Onshape source IDs, export options, parameter presets/overrides, queue coordination, unique work keys, job status, artifact index, failure summaries. |
| Tigris Object Storage | Durable public artifacts, previews, raw Onshape responses, normalized parameter metadata. |
| Onshape API | Configuration discovery and export generation. |

Current Rust boundaries are still mostly in one crate and several responsibilities remain in `main.rs`. The intended internal boundaries are:

- `config`: environment, secrets, deployment settings.
- `catalog`: catalog types, JSON seed import, and validation.
- `routes`: product pages, enqueue/status handlers, health checks.
- `onshape`: signed Onshape API requests and response models.
- `jobs`: SQLite queue, leases, retries, failure records.
- `worker`: bounded background loop and Onshape polling orchestration.
- `storage`: Tigris/S3 reads, writes, metadata, public URLs.
- `cache_keys`: canonical identity and hash helpers.
- `templates`: server-rendered pages or static page helpers.

## Public User Flow

1. User opens a curated model page.
2. The page loads the live catalog snapshot from SQLite and normalized parameter metadata from Tigris when cached.
3. User changes parameter selections.
4. The site computes or requests a deterministic configuration hash.
5. The site calls a public app route to check whether a preview artifact set exists for that configuration.
6. If no preview artifact set exists, the user can request preview generation through the app route.
7. User chooses STEP, STL, or raw Onshape geometry 3MF for final download.
8. The app enqueues missing work through SQLite. A worker generates missing artifacts and stores them in Tigris.
9. The UI polls status until preview or download artifacts are ready.
10. Ready artifacts are served through stable public Tigris URLs.

Every parameter refresh and export has a deterministic unique work key. Request handlers must create or find the SQLite job row before any Onshape call starts.

The browser may submit parameter values, but the server owns validation, canonicalization, and hash calculation. Browser-computed hashes are advisory at most.

## Product Routes

These routes are product/UI behavior, not stable public API commitments.

Implemented routes on this branch:

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Catalog landing page. |
| `GET` | `/models/{slug}` | Model page with parameter controls. |
| `POST` | `/models/{slug}` | Validate submitted parameters and render normalized values or errors. |
| `POST` | `/models/{slug}/preview` | Validate submitted parameters and create or find a preview job. |
| `GET` | `/models/{slug}/preview/{config_hash}/status` | Poll preview status by server-computed hash. |
| `POST` | `/models/{slug}/exports/{format}` | Validate submitted parameters and create or find a STEP, STL, or raw Onshape geometry 3MF export job. |
| `GET` | `/models/{slug}/exports/{format}/{config_hash}/status` | Poll download export status by server-computed hash. |
| `GET` | `/healthz` | Process health check. |
| `GET` | `/metrics` | Prometheus-style operational metrics. |

Planned route and response gaps from the updated main plan:

- Add a normalized parameter metadata/status route such as `GET /models/{slug}/parameters` if the UI needs JSON parameter data.
- Ensure status checks and enqueue routes validate parameters through the same server path so status and job creation cannot disagree about `configHash`.
- Add public-safe status fields such as `jobId`, `groupId`, `readyOutputs`, `retryAfterSeconds`, `errorCode`, and `userMessage`.
- Add a stable job polling route such as `GET /jobs/{job_id}` only if product/UI polling needs an ID-based route.
- Keep public status handling aligned with supersession-based invalidation; normal invalidation should supersede records rather than delete public artifacts.

## Operational Flow

There is no web admin UI in the MVP. Maintenance operations are CLI-only or run through Fly operational access.

Initial operational commands:

- Import, list, show, and validate SQL-backed catalog entries.
- Fetch and cache parameter metadata for a model version.
- Generate or rebuild missing previews.
- Generate or rebuild download artifacts.
- Inspect artifacts, jobs, and failure records.
- Invalidate or supersede cached outputs after exporter changes.

Authenticated `/admin` routes can be added later when browser-based administration becomes necessary.

## Database Scope

SQLite is the MVP source of truth for catalog rows and coordination/index state. Large or blob-like cache data remains in Tigris; Tigris is not the catalog source of truth.

Postgres becomes useful when SQLite-on-volume constraints become painful, such as multi-machine workers, stronger managed backups, richer admin queries, editable catalog workflows, users, quotas, audit logs, analytics, or robust long-term job history.
