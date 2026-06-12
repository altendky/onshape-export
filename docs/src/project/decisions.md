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

## Onshape Auth

**Decision:** Use server-owned Onshape API keys for the MVP, pending verification with real Onshape calls.

The Rust service signs Onshape API requests server-side. API keys are configured as deployment secrets and are never exposed to browsers. OAuth or user-delegated authorization is deferred unless API keys cannot access the required versioned configuration metadata, translation creation, polling, or external data download endpoints.

## Supported Model Types

**Decision:** Support both Part Studios and Assemblies.

The export layer needs element-kind-specific endpoint selection.

## Download Formats

**Decision:** Support STEP, STL, and 3MF downloads.

GLB is a preview format for the MVP and is not a supported user-download format; adding GLB downloads later requires a separate decision. It is still cached like every other artifact.

## Preview Format

**Decision:** Use GLB as the MVP browser preview artifact.

GLB is generated as a separate Onshape export for the same selected configuration. It is not derived locally from STEP, STL, or 3MF in the MVP. Documentation may mention glTF only as the broader format family or Onshape translation terminology.

Target cache language should treat preview output as a preview artifact set, not as one guaranteed `.glb` file. GLB remains the preferred/current single-file preview case.

Current branch status: direct GLB responses are accepted after GLB header validation, and direct glTF JSON responses are accepted as browser viewer artifacts. Zipped Onshape preview responses use exactly one valid `.glb` when present; otherwise a ZIP with exactly one `.gltf` publishes that viewer asset, safe sidecars, and the original `source.zip` under the preview identity. ZIPs with multiple `.gltf` files are rejected rather than showing a partial preview.

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

## Worker Topology

**Decision:** Run the MVP web server and worker loop in one Rust process by default.

The initial Fly deployment uses one Rust service process with the public `axum` server and a bounded embedded background worker loop. SQLite lives on the same Fly volume and provides transactional job coordination for that process. Separate worker process groups are supported by the branch but should stay an operational escape hatch until shared storage semantics are verified or the coordination backend moves to Postgres.

## Public Artifact Delivery

**Decision:** Completed artifacts are public.

The product is a public anonymous catalog, so completed GLB, STEP, STL, and 3MF outputs do not need signed or expiring URLs. Keep internal operational state out of public object prefixes and do not expose object listing.

## Artifact Invalidation

**Decision:** Normal MVP cache invalidation uses supersession, not overwrite or deletion.

Public artifact objects are immutable in normal operation. Exporter, schema, catalog, option, or parameter changes produce new artifact keys and update manifests or index state to point at newer outputs. Older public URLs may remain addressable and can be marked superseded for operational visibility. Deletion is reserved for explicit operator cleanup, legal or IP concerns, or storage-cost management.

Current branch status: `artifacts invalidate` and `artifacts prune` mark ready SQLite artifact records superseded and leave public object-store artifacts untouched. Destructive deletion remains reserved for a future explicit cleanup command.

## Admin Surface

**Decision:** Do not build a web admin UI in the MVP.

Maintenance operations should be CLI-only or run through Fly operational access. Add authenticated `/admin` routes only when browser-based admin workflows become necessary.
