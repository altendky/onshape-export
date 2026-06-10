# Implementation Plan

## Execution Protocol

This plan is intended to support repeated prompts such as `implement the next phase of the plan`.

When executing that prompt:

1. Read this file and choose the first phase in the tracker whose status is not `done`.
2. Complete only that phase unless a prerequisite from an earlier phase is clearly missing.
3. Keep changes scoped to the selected phase.
4. Run the phase verification commands and any relevant local checks.
5. Update the phase tracker and checklist in this file.
6. If the user requested commits for phase execution, create exactly one signed git commit for the completed phase.

Do not skip a phase because a later phase looks more interesting. If a phase cannot be completed, leave it marked `blocked`, document the blocker, and do not start the next phase.

## Phase Tracker

| Phase | Status | Current Branch Summary |
| --- | --- | --- |
| Phase 0: Documentation And Decisions | `done` | Planning docs reconciled with implementation status and TODO gaps. |
| Phase 1: Tooling And Rust Skeleton | `in_progress` | Rust skeleton exists; real CI/tooling still TODO. |
| Phase 2: Contracts, Catalog, And Cache Keys | `in_progress` | `catalog/v1`, RFC 8785 hash helpers, and split identity hashes exist; typed/unit canonicalization and unsupported-parameter metadata are TODO. |
| Phase 3: SQLite Queue And Fake Worker | `in_progress` | SQLite queue/leases, retry backoff, and target states exist; failure table and persisted translation state are TODO. |
| Phase 4: Tigris Storage And Manifests | `in_progress` | Tigris writes, v1 keys, manifests, and ready metadata exist; upload verification, reconciliation, and full missing/superseded output history are TODO. |
| Phase 5: Onshape Auth And Metadata Read Path | `in_progress` | API-key signing and metadata path exist; real smoke tests and unsupported metadata are TODO. |
| Phase 6: Preview Vertical Slice | `in_progress` | Preview route, worker path, viewer, GLB handling, and single-asset Onshape zipped glTF fallback exist; real Onshape verification and full public-safe status contract are TODO. |
| Phase 7: Download Export Vertical Slice | `in_progress` | STEP/STL/3MF paths and ready metadata exist; real Onshape verification and recovery from partial writes are TODO. |
| Phase 8: Operational Commands | `in_progress` | CLI foundation and supersession invalidation/pruning exist; cache reconcile and target command shape are TODO. |
| Phase 9: Runtime Hardening | `in_progress` | Metrics, backup, Fly scaffold, worker concurrency, scheduled rebuilds, retry backoff, and DB lock tests exist; rate/lifecycle hardening is TODO. |

Status values:

- `pending`: not started.
- `in_progress`: partially implemented or currently being worked.
- `blocked`: cannot continue without a decision, dependency, credential, or external service.
- `done`: deliverables are complete, verification is recorded, and the phase commit exists if commits were requested.

## Commit Policy

For phase execution, prefer one commit per phase. The commit message should be concise and phase-scoped, for example:

```text
Implement phase 1 Rust skeleton
```

Before committing, inspect `git status`, `git diff`, and recent git history. Stage only files belonging to the completed phase. Commits must remain GPG signed according to local git configuration.

## Branch Implementation Snapshot

Implemented foundation:

- Single-crate Rust `axum` app.
- `GET /healthz`, `GET /`, `GET /metrics`, and model product routes.
- Environment-based runtime configuration.
- SQLite connection setup with migrations and MVP durability PRAGMAs.
- Tigris/S3-compatible client construction.
- Signed Onshape API-key client for configuration metadata and export calls.
- In-repo `catalog/v1` JSON loading and validation, with explicit legacy catalog compatibility.
- Onshape parameter metadata refresh, normalization, Tigris caching, and SQLite deduplication.
- RFC 8785 canonical JSON hashing for source, configuration, options, and work keys.
- Server-rendered model parameter controls and submitted-value validation.
- Background worker loop for queued parameter refreshes, previews, and downloads.
- Retry attempt limits, `nextRetryAt`, and bounded exponential full-jitter backoff.
- Strict GLB preview artifact handling.
- Supersession-based artifact invalidation and pruning.
- Worker-only runtime mode and configurable worker concurrency.
- CLI maintenance commands for catalog validation, parameter refresh, pre-generation, job/failure inspection and retry, artifact inspection/invalidation, pruning, and manifest rewrite.
- Catalog-defined parameter presets, UI overrides, preview options, and STEP export option defaults.
- Deploy-time `ops check` and operator-triggered SQLite backup snapshots.
- Temporary placeholder GitHub Actions job named exactly `all`.

Plan review TODOs that are not implemented yet:

- Replace string-only parameter canonicalization with typed values and unit normalization where needed.
- Add uploaded-object verification and cache reconciliation for partial writes.
- Add retryability classes, stable public-safe error codes, and user messages.
- Add a separate failure history table if job summaries stop being enough.
- Persist Onshape translation IDs and polling state for crash recovery.
- Honor Onshape `Retry-After` once the client exposes it.
- Materialize missing outputs, replacement pointers, and full supersession history in manifests.

## Phase 0: Documentation And Decisions

Purpose: keep planning docs accurate for the implemented branch and clearly mark target-plan deviations as TODO gaps.

Completed outputs:

- Architecture and product route status.
- Runtime options and SQLite transaction constraints.
- Onshape API flow and auth assumptions.
- Tigris cache contract and current deviations.
- Frontend preview behavior and GLB/glTF fallback target.
- Admin and catalog direction.
- CI and local tooling direction.
- Decisions and open questions deduplicated.

## Phase 1: Tooling And Rust Skeleton

Set up the project without implementing the export workflow.

Implemented on branch:

- Single Rust crate.
- `axum` service skeleton.
- Health endpoint.
- Configuration and secret-loading path.
- SQLite, Tigris, Onshape, catalog, and server-rendered UI foundations.
- Placeholder GitHub Actions workflow with required job `all`.

TODO gaps:

- Replace placeholder CI with real aggregate `all` over docs, Rust, and pre-commit jobs.
- Reconcile `mise.toml` and `mise.lock` intentionally before depending on them in CI.
- Add or confirm pre-commit, markdown, actionlint, mdBook, Rust fmt, clippy, and test checks.
- Add route tests for `/healthz` if not covered by integration tests.

Verification target:

- `cargo fmt --all --check`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- `cargo test --all-targets --all-features` or `cargo nextest run` once configured.
- `mdbook build docs` and `mdbook test docs` when mdBook is available.

## Phase 2: Contracts, Catalog, And Cache Keys

Make implementation contracts executable before relying on Onshape.

Implemented on branch:

- `catalog/v1/models.json` plus `catalog/v1/models/{slug}.json`, with explicit legacy catalog compatibility for operators that set `CATALOG_PATH` to the old single-file layout.
- Catalog validation for schema version, entry version, slugs, tags, duplicate source identities, duplicate formats, preview resolution, STEP option presence, preset slugs, override precision, override widgets, and cached-schema override IDs.
- `catalogSchemaVersion`, `entryVersion`, `published`, `tags`, `thumbnail`, optional `linkDocumentId`, and `parameterPolicy.autoRefresh` support.
- RFC 8785 canonical JSON hash helpers with golden tests.
- Split source, configuration, options, work-key, group, and artifact identity helpers.
- Hash tests for value ordering, domain separation, and option changes.
- Model page rendering from catalog and normalized metadata.

TODO gaps:

- Replace string-only parameter values with typed canonical values if the current representation proves ambiguous.
- Add unit synonym/conversion canonicalization where target models need it.
- Represent unsupported Onshape parameter types with `unsupportedReason`.

## Phase 3: SQLite Queue And Fake Worker

Build coordination before real external work.

Implemented on branch:

- SQLite migrations for `jobs`, `artifacts`, and `parameter_metadata`.
- Unique `work_key` queue deduplication.
- Lease claim and stale lease recovery by attempt fencing.
- Embedded bounded worker loop.
- `queued`, `running`, `ready`, `failed`, and `superseded` states.
- `maxAttempts`, `nextRetryAt`, and bounded exponential full-jitter retry backoff.
- Manual retry by all failures, work key, or job kind.
- Job listing and metrics.

TODO gaps:

- Add a separate `failures` table with error classes and public-safe summaries.
- Add retryability classes and expose Onshape `Retry-After` through the retry scheduler.
- Persist Onshape translation IDs and poll state in the job row.
- Add `jobs show` and `jobs retry` target commands if the CLI shape is changed.

## Phase 4: Tigris Storage And Manifests

Prove object storage and manifest coordination.

Implemented on branch:

- S3-compatible Tigris client configuration.
- Public URL generation.
- Preview and download uploads.
- Content type and download content disposition for downloads.
- Manifest materialization from SQLite artifact records.
- v1 object keys for parameter metadata, previews, downloads, and manifests.
- Ready artifact metadata including public URL, SHA-256, byte length, source/options hashes, producing job key, and supersession state.
- Manifest inspection and rewrite CLI.

TODO gaps:

- Verify uploaded objects before marking jobs ready.
- Add reconciliation for object-store/SQLite drift.
- Materialize missing outputs, replacement pointers, and full supersession history in manifests.
- Store content disposition in SQLite if operations need it outside object metadata.

## Phase 5: Onshape Auth And Metadata Read Path

Prove server-owned Onshape API key access and parameter discovery.

Implemented on branch:

- Server-side Onshape API-key request signing.
- Versioned configuration metadata fetch.
- Raw metadata storage in Tigris.
- Normalized metadata storage in Tigris.
- Metadata refresh through CLI and queued worker path.
- Model page rendering with normalized parameter controls.

TODO gaps:

- Record real-call smoke-test results for Part Studio and Assembly metadata.
- Confirm API-key permissions for linked-document assembly contexts.
- Add sanitized Onshape fixtures for all target parameter types.
- Preserve unsupported parameter types with `unsupportedReason`.
- Decide when to use `configurationencodings` for canonical Onshape configuration strings.

## Phase 6: GLB Preview Vertical Slice

Implement one complete preview path.

Implemented on branch:

- Preview generation route and status route.
- Server-side parameter validation before enqueue.
- Worker export request, polling, external data download, Tigris upload, artifact record, and manifest rewrite.
- `<model-viewer>` rendering for cached preview URLs.
- Status polling in the product page.
- GLB preview extraction from direct and zipped Onshape responses, plus single-asset zipped glTF fallback when Onshape does not return GLB.

TODO gaps:

- Add target public-safe status fields.
- Persist Onshape translation IDs and polling state for crash recovery.
- Verify GLB export behavior with real Onshape models.

## Phase 7: Download Export Vertical Slice

Start with STEP, then add STL and 3MF after the abstraction is proven.

Implemented on branch:

- STEP export path using format-specific endpoints.
- STL and 3MF export paths using generic translation endpoints.
- Download generation and status routes.
- Tigris upload with content type, content disposition, and immutable cache header.
- Public download links when `TIGRIS_PUBLIC_BASE_URL` is configured.
- Source/config/options hash identity and v1 download object keys.
- Ready artifact metadata in SQLite, manifests, and status responses.

TODO gaps:

- Verify STEP, STL, and 3MF export requests and response shapes with real Onshape models.
- Confirm exact generic `formatName` values.
- Add recovery for completed Onshape translations whose upload or SQLite ready mark failed.

## Phase 8: Operational Commands

Add enough CLI/Fly-run maintenance controls for MVP operations.

Implemented on branch:

- `catalog validate`.
- `parameters refresh`.
- `previews generate`.
- `exports generate`.
- `jobs list`.
- `failures list`.
- `failures retry`.
- `artifacts list`.
- `artifacts manifest`.
- `artifacts invalidate`.
- `artifacts prune`.
- `ops check`.
- `ops backup`.

TODO gaps:

- Add cache reconciliation.
- Decide whether to rename commands toward the target `validate-catalog`, `refresh-parameters`, `generate-preview`, `generate-export`, `jobs`, and `cache` shape.
- Add dry-run behavior for expensive generation where needed.

## Phase 9: Runtime Hardening

Add robustness only as needed after the vertical slices work.

Implemented on branch:

- Prometheus-style `/metrics` route.
- Worker-only runtime and `WORKER_ENABLED=false`.
- Explicit `WORKER_CONCURRENCY`, defaulting to `1`.
- Scheduled rebuild enqueueing through `REBUILD_INTERVAL_SECONDS`.
- Scheduled parameter refresh respects `parameterPolicy.autoRefresh`.
- SQLite backup snapshot through `ops backup`.
- Fly deployment scaffold with a single machine and mounted volume.
- Deploy-time `ops check` for catalog, SQLite, storage config, public URL config, and credential presence.
- Retry backoff and a DB lock regression test for long-running claimed jobs.

TODO gaps:

- Add request rate limits and queue depth limits if traffic requires them.
- Add runtime runbooks for stuck jobs, failed exports, and cache reconciliation.
- Add automated or platform backup policy if explicit snapshots are not enough.
- Move to Postgres or another shared coordination backend before multi-machine workers.
- Add web admin UI and authentication only if CLI operations become insufficient.
