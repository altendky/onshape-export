# Onshape Export

A planned Rust-based website for exporting curated, highly parameterized Onshape models.

The service is aimed at models where publishing every parameter combination ahead of time is impractical. Users select parameters in the browser, preview the resulting model, and download generated exports. The service owns the Onshape integration and cache, so users do not need Onshape accounts.

## Current Status

This repository currently contains planning documentation only. No application code has been added yet.

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
