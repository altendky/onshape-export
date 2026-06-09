# Onshape Export

A planned Rust-based website for exporting curated, highly parameterized Onshape models.

The service is aimed at models where publishing every parameter combination ahead of time is impractical. Users select parameters in the browser, preview the resulting model, and download generated exports. The service owns the Onshape integration and cache, so users do not need Onshape accounts.

## Current Status

This repository contains planning documentation and an initial Rust service with parameter discovery.

Implemented foundation:

- Single-crate Rust `axum` app.
- `GET /healthz`, `GET /`, and placeholder `GET /models/{slug}` routes.
- Environment-based runtime configuration.
- SQLite connection setup with migrations and MVP durability PRAGMAs.
- Tigris/S3-compatible client construction.
- Signed Onshape API-key client for configuration metadata reads.
- In-repo catalog JSON loading and validation.
- Onshape parameter metadata refresh, normalization, Tigris caching, and SQLite deduplication.
- Server-rendered model parameter controls and submitted-value validation.
- Background worker loop for queued parameter refreshes, previews, and downloads.
- Worker-only runtime mode for separate Fly process groups.
- Configurable worker concurrency through `WORKER_CONCURRENCY`.
- CLI maintenance commands for catalog validation, parameter refresh, pre-generation, failure retry, and artifact inspection/invalidation.
- Catalog-defined parameter presets for targeted preview/export pre-generation.

Local run:

```sh
cargo run
```

Worker-only run:

```sh
cargo run -- worker
```

Set `WORKER_ENABLED=false` when running a web process that should not also claim queued work.
Set `WORKER_CONCURRENCY` to control how many queued jobs a worker process may run at once; the default is `1`.

The default local database is `onshape-export.db`. Set `DATABASE_URL` for deployment, for example to a SQLite file on a Fly volume.

Fly deployment foundation:

```sh
fly volumes create onshape_export_data --size 1 --region ord
fly secrets set ONSHAPE_ACCESS_KEY=... ONSHAPE_SECRET_KEY=... AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=... TIGRIS_BUCKET=...
fly deploy
```

The included `fly.toml` runs a single web machine with the in-process worker enabled so SQLite coordination stays on one mounted Fly volume at `/data`.

## Product Direction

- Curated model catalog, not arbitrary Onshape URL export.
- Onshape document versions only, not mutable workspaces.
- Anonymous end users, using server-owned Onshape credentials.
- Download formats: STEP, STL, and 3MF.
- Preview format: cached GLB/glTF export shown in a browser 3D viewer.
- Runtime: Fly.io Rust app at `https://onshape-export.fly.dev` if the app name is available.
- Cache backend: Tigris Object Storage via Fly, with public stable artifact URLs.
- Coordination database: SQLite on a Fly volume for queue/job uniqueness.
- No public API initially; expose product/UI routes only.
- No web admin UI initially; maintenance operations are CLI or Fly operational commands.

## Documentation

Project documentation is under `docs/src/project/`.

- [Project Overview](docs/src/project/index.md)
- [Architecture](docs/src/project/architecture.md)
- [Runtime Options](docs/src/project/runtime.md)
- [Caching](docs/src/project/caching.md)
- [Onshape API Flow](docs/src/project/onshape-api.md)
- [Frontend and Preview](docs/src/project/frontend-preview.md)
- [Catalog](docs/src/project/catalog.md)
- [Admin Operations](docs/src/project/admin.md)
- [Library Reuse](docs/src/project/library-reuse.md)
- [Implementation Plan](docs/src/project/implementation.md)
- [Decisions](docs/src/project/decisions.md)
- [Open Questions](docs/src/project/open-questions.md)

## Reference Projects

- `~/repos/onshape-mcp`: Onshape auth/client patterns and request modeling.
- `~/repos/onshape3mf`: Python proof-of-concept for configuration discovery and async export calls.
