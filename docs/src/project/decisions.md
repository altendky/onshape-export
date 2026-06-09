# Decisions

Resolved or working decisions for the initial design.

## Curated Catalog

**Decision:** Start with a curated catalog, not arbitrary Onshape URL export.

This keeps credentials, access control, cache identity, and abuse risk manageable.

## Versioned Documents Only

**Decision:** Support Onshape document versions only for end-user exports.

Workspaces are mutable and complicate cache correctness. Versions give stable source identity.

## Anonymous End Users

**Decision:** Users should not need Onshape accounts.

The service uses server-owned Onshape credentials for approved models. Onshape credentials never reach the browser.

## Supported Model Types

**Decision:** Support both Part Studios and Assemblies.

The export layer needs element-kind-specific endpoint selection.

## Download Formats

**Decision:** Support STEP, STL, and 3MF downloads.

GLB/glTF is a preview format, not initially a user download format, though it is still cached like every other artifact.

## Preview Format

**Decision:** Use GLB/glTF as the browser preview format.

GLB is generated as a separate Onshape export for the same selected configuration. It is not derived locally from STEP, STL, or 3MF in the MVP.

## Public API

**Decision:** Do not expose a stable public API initially.

Use product/UI routes only. Add an API later only with explicit keys, quotas, and idempotency.

## Cache Backend

**Decision:** Use Tigris Object Storage via Fly as the artifact/cache backend.

Tigris stores completed artifacts, previews, manifests, raw Onshape responses, and normalized parameter metadata. Completed public artifacts are durable cache outputs and should be served directly from stable, non-expiring Tigris URLs.

Fly app egress is metered, so the Rust app should not proxy GLB, STEP, STL, or 3MF bytes in the steady state.

## Job Coordination

**Decision:** Use SQLite on a Fly volume for MVP job coordination.

Duplicate Onshape parameter fetches and export requests are not acceptable. SQLite provides transactional uniqueness for deterministic work keys without the fixed cost of Fly Managed Postgres. Tigris remains the permanent object store; SQLite stores queue, status, artifact index, and failure summary state.

Use robust SQLite settings such as WAL mode, `synchronous = FULL`, foreign keys, and short transactions. Do not hold database write transactions while calling Onshape.

## Catalog Storage

**Decision:** Start with in-repo catalog data and documentation.

Move to Tigris JSON or a relational database only when admin-editable catalog requirements exist.

## Runtime Shape

**Decision:** Use a Fly-hosted Rust service for the MVP.

The initial service is a Rust `axum` app on Fly.io, served at `https://onshape-export.fly.dev` if that app name is available. It handles public pages, parameter validation, queue submission, status routes, Onshape orchestration, and Tigris object writes.

Worker processes default to one claimed job at a time. `WORKER_CONCURRENCY` can raise concurrency explicitly after observing real Onshape API behavior, translation latency, and SQLite volume performance.

## Public Artifact Delivery

**Decision:** Completed artifacts are public.

The product is a public anonymous catalog, so completed GLB, STEP, STL, and 3MF outputs do not need signed or expiring URLs. Keep internal operational state out of public object prefixes and do not expose object listing.

## Admin Surface

**Decision:** Do not build a web admin UI in the MVP.

Maintenance operations should be CLI-only or run through Fly operational access. Add authenticated `/admin` routes only when browser-based admin workflows become necessary.
