# Architecture

## Proposed Shape

The recommended MVP shape stays on Fly/Tigris:

```text
Browser
  |
  | public pages, cache checks, status, downloads, GLB preview
  v
Fly.io Rust axum app
  |\
  | \ stable public artifact URLs, metadata reads/writes
  |  v
  | Tigris Object Storage
  |  ^
  |  | artifact uploads, manifests, cached metadata
  |
  | queue, job uniqueness, status, artifact index
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

The public site should be able to serve cached content even when no export job is running. The Rust service owns catalog validation, queue submission, status routes, Onshape calls, translation polling, and Tigris uploads. SQLite owns transactional coordination so duplicate Onshape work is not started for the same deterministic key.

## Components

| Component | Responsibility |
| --- | --- |
| Public UI | Catalog browsing, parameter forms, preview viewer, export requests. |
| Fly Rust app | Public routing, validation, cache checks, queue submission, status routes, Onshape orchestration, Tigris writes. |
| SQLite on Fly volume | Queue coordination, unique work keys, job status, artifact index, failure summaries. |
| Tigris Object Storage | Durable public artifacts, previews, manifests, raw Onshape responses, normalized parameter metadata. |
| Onshape API | Configuration discovery and export generation. |

## Public User Flow

1. User opens a curated model page.
2. The page loads normalized parameter metadata from repo data or Tigris.
3. User changes parameter selections.
4. The site computes or requests a deterministic configuration hash.
5. The site calls a public app route to check whether a GLB preview exists for that configuration.
6. If no preview exists, the user can request preview generation through the app route.
7. User chooses STEP, STL, or 3MF for final download.
8. The app enqueues missing work through SQLite. A worker generates missing artifacts and stores them in Tigris.
9. The UI polls status until preview or download artifacts are ready.
10. Ready artifacts are served through stable public Tigris URLs.

Every parameter refresh and export has a deterministic unique work key. Request handlers must create or find the SQLite job row before any Onshape call starts.

## Operational Flow

There is no web admin UI in the MVP. Maintenance operations are CLI-only or run through Fly operational access.

Initial operational commands:

- Validate curated catalog entries.
- Fetch and cache parameter metadata for a model version.
- Generate or rebuild missing previews.
- Generate or rebuild download artifacts.
- Inspect manifests, jobs, and failure records.
- Invalidate or supersede cached outputs after exporter changes.

Authenticated `/admin` routes can be added later when browser-based administration becomes necessary.

## Database Scope

SQLite is included in the MVP only for coordination and lightweight index state. The catalog remains in the repository, and large/permanent cache data remains in Tigris.

Postgres becomes useful when SQLite-on-volume constraints become painful, such as multi-machine workers, stronger managed backups, richer admin queries, editable catalog workflows, users, quotas, audit logs, analytics, or robust long-term job history.
