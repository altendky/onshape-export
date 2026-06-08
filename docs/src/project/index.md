# Onshape Export

Onshape Export is a planned Rust-based website for exporting curated Onshape models with user-selected configuration parameters.

The primary use case is a highly parameterized model that cannot reasonably be exported for every possible combination before publishing. Instead, the website presents validated parameter controls, generates exports on demand, caches the results, and serves later requests from cache.

## Scope

Initial scope:

- Curated list of models maintained by the project.
- Onshape document versions only.
- Part Studios and Assemblies.
- Anonymous end users.
- Server-owned Onshape API access.
- STEP, STL, and 3MF downloads.
- GLB browser preview for selected configurations.
- Fly.io Rust app at `https://onshape-export.fly.dev` if the app name is available.
- Tigris artifact cache with stable public artifact URLs.
- SQLite on a Fly volume for queue coordination and job uniqueness.
- Documentation-first planning before implementation.

Out of initial scope:

- Arbitrary user-provided Onshape URLs.
- User Onshape login.
- Mutable workspace exports.
- Public API commitments.
- Database-backed catalog, users, analytics, or billing.

## Design Priorities

- Keep Onshape credentials server-side.
- Avoid unnecessary Onshape API calls through deterministic caching.
- Make preview and download artifacts independently cacheable.
- Keep the first implementation small and reversible.
- Prefer boring Rust service code for long-running export orchestration.
- Keep the MVP on Fly/Tigris unless another provider provides a clear benefit.
- Avoid duplicate Onshape parameter fetches and export requests through transactional job coordination.
