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

| Phase | Status | Commit Scope |
| --- | --- | --- |
| Phase 0: Documentation And Decisions | `in_progress` | Planning docs only. |
| Phase 1: Tooling And Rust Skeleton | `pending` | Initial Rust project, health route, config, baseline CI. |
| Phase 2: Contracts, Catalog, And Cache Keys | `pending` | Catalog schema/loading, identity structs, hash tests. |
| Phase 3: SQLite Queue And Fake Worker | `pending` | Migrations, queue tables, fake worker, job CLI basics. |
| Phase 4: Tigris Storage And Manifests | `pending` | Storage adapter, object keys, manifests, artifact index. |
| Phase 5: Onshape Auth And Metadata Read Path | `pending` | Onshape API-key client, metadata fetch/normalize path. |
| Phase 6: GLB Preview Vertical Slice | `pending` | Preview routes, worker path, GLB artifact generation. |
| Phase 7: Download Export Vertical Slice | `pending` | STEP first, then STL/3MF download exports. |
| Phase 8: Operational Commands | `pending` | MVP CLI/Fly-run maintenance commands. |
| Phase 9: Runtime Hardening | `pending` | Backup, metrics, rate limits, lifecycle hardening as needed. |

Status values:

- `pending`: not started.
- `in_progress`: currently being worked.
- `blocked`: cannot continue without a decision, dependency, credential, or external service.
- `done`: deliverables are complete, verification is recorded, and the phase commit exists if commits were requested.

## Commit Policy

For phase execution, prefer one commit per phase. The commit message should be concise and phase-scoped, for example:

```text
Implement phase 1 Rust skeleton
```

Before committing, inspect `git status`, `git diff`, and recent git history. Stage only files belonging to the completed phase. Commits must remain GPG signed according to local git configuration.

## Phase 0: Documentation And Decisions

Current phase.

Outputs:

- Capture architecture and runtime options.
- Capture Onshape API flow.
- Capture Tigris cache contract.
- Capture SQLite queue and coordination direction.
- Capture frontend preview behavior.
- Capture admin and catalog direction.
- Capture CI and local tooling direction.

Completion checklist:

- [ ] Architecture, runtime, caching, Onshape API, frontend, catalog, admin, CI, implementation, decisions, and open-question docs are internally consistent.
- [ ] Resolved working decisions are captured in `decisions.md` and removed or reworded in `open-questions.md`.
- [ ] `git diff --check` passes.
- [ ] Docs build is run, or unavailable tooling is explicitly recorded.
- [ ] Phase tracker is updated.

## Phase 1: Tooling And Rust Skeleton

Set up the project without implementing the export workflow.

Deliverables:

- Rust workspace or single crate decision.
- `axum` service skeleton.
- Health endpoint.
- Configuration and secret loading.
- Minimal template/static asset direction.
- CI for formatting, linting, tests, and docs once `Cargo.toml` exists.

Verification:

- `cargo fmt --all --check`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- `cargo test --all-targets --all-features` or `cargo nextest run` once configured.
- Health route test.

Completion checklist:

- [ ] Rust project layout decision is implemented.
- [ ] Service starts locally and exposes `/healthz`.
- [ ] Configuration and secret-loading path exists without real secrets committed.
- [ ] Baseline CI/local check commands are documented and runnable.
- [ ] Phase verification results are recorded in the final response or commit notes.
- [ ] Phase tracker is updated.

## Phase 2: Contracts, Catalog, And Cache Keys

Make implementation contracts executable before calling Onshape.

Deliverables:

- Catalog schema and loader for `catalog/v1/models.json` and `catalog/v1/models/{slug}.json`.
- One minimal complete catalog entry.
- Identity structs and canonical hash helpers for source, config, options, group, work, artifact, and manifest IDs.
- Golden tests for canonical JSON and hash outputs.
- Product route table represented in code stubs where useful.

Verification:

- Catalog validation tests for slug rules, duplicate identities, and bad overrides.
- Hash golden tests.
- Route tests for catalog index and model page using fixture catalog data.

Completion checklist:

- [ ] Catalog files load from `catalog/v1/`.
- [ ] Invalid catalog entries fail validation tests.
- [ ] Identity and hash helpers have deterministic golden tests.
- [ ] Model page can render from fixture catalog data.
- [ ] Phase verification results are recorded.
- [ ] Phase tracker is updated.

## Phase 3: SQLite Queue And Fake Worker

Build coordination before real external work.

Deliverables:

- SQLite migration framework.
- `jobs`, `failures`, and `artifacts` tables.
- Unique index on `workKey`.
- Lease claim, lease expiry, retry, and status logic.
- Embedded bounded worker loop using fake job handlers.
- Basic CLI commands for `jobs list`, `jobs show`, and `jobs retry`.

Verification:

- Migration tests against temporary SQLite.
- PRAGMA enforcement test.
- Queue deduplication test.
- Lease-expiry recovery test.

Completion checklist:

- [ ] Migrations create `jobs`, `failures`, and `artifacts` tables.
- [ ] `workKey` uniqueness prevents duplicate jobs.
- [ ] Fake worker can claim, complete, fail, and retry jobs.
- [ ] Stale lease behavior is tested.
- [ ] Basic job CLI commands work against local SQLite.
- [ ] Phase tracker is updated.

## Phase 4: Tigris Storage And Manifests

Prove object storage and manifest coordination with fake artifacts.

Deliverables:

- S3-compatible Tigris client configuration.
- Object key helpers.
- Manifest read/write.
- Artifact index writes.
- Public URL generation.
- Reconciliation command skeleton.

Verification:

- Storage adapter tests against mock or local S3-compatible service where practical.
- Manifest serialization tests.
- Artifact write-ordering test with fake artifacts.

Completion checklist:

- [ ] Tigris/S3 client configuration is wired without committing credentials.
- [ ] Object key helpers match the documented layout.
- [ ] Manifest read/write supports ready, missing, and superseded outputs.
- [ ] Artifact index updates follow upload, verify, manifest, ready ordering.
- [ ] Phase verification results are recorded.
- [ ] Phase tracker is updated.

## Phase 5: Onshape Auth And Metadata Read Path

Prove server-owned Onshape API key access and parameter discovery.

Deliverables:

- Onshape client skeleton with signed API-key requests.
- Real-call smoke test checklist for one Part Studio and one Assembly.
- Fetch raw configuration metadata.
- Normalize metadata for UI rendering.
- Store raw and normalized metadata in Tigris.
- `refresh-parameters` CLI command.
- Model page rendering from normalized metadata.

Verification:

- Unit tests with sanitized Onshape fixtures.
- Manual integration checklist with real Onshape credentials.
- Validation tests for submitted parameter values.

Completion checklist:

- [ ] API-key request signing is implemented behind the Onshape client.
- [ ] Raw metadata fetch works with sanitized fixtures.
- [ ] Normalized metadata supports the observed target parameter types or records unsupported reasons.
- [ ] Parameter metadata can be refreshed through the queue/CLI path.
- [ ] Real-call smoke-test results are recorded, or missing credentials are documented as a blocker.
- [ ] Phase tracker is updated.

## Phase 6: GLB Preview Vertical Slice

Implement one complete preview path.

Deliverables:

- Preview status and enqueue routes.
- Server-side config canonicalization and `configHash` calculation.
- Preview `optionsHash` calculation.
- GLB export request, translation polling, external data download, Tigris upload, manifest update, and artifact record update.
- `<model-viewer>` integration for cached GLB URLs.

Verification:

- Route tests for status, enqueue, and polling responses.
- Worker tests with mocked Onshape translation lifecycle.
- Manual real Onshape/Tigris preview generation.
- Duplicate preview requests do not start duplicate jobs.

Completion checklist:

- [ ] Preview status route validates and canonicalizes parameters server-side.
- [ ] Preview enqueue route creates or finds the deterministic job.
- [ ] Worker stores GLB artifacts, manifests, and artifact records in order.
- [ ] UI can display a cached GLB URL with `<model-viewer>`.
- [ ] Real preview generation is tested or blocked on credentials/service access.
- [ ] Phase tracker is updated.

## Phase 7: Download Export Vertical Slice

Start with STEP, then add STL and 3MF after the abstraction is proven.

Deliverables:

- Export enqueue/status route for STEP.
- Format-specific defaults and `optionsHash`.
- STEP export, polling, download, upload, manifest, and artifact index.
- Content type, content disposition, and stable public URL policy.
- Add STL and 3MF using the same path.

Verification:

- STEP manual integration test first.
- STL and 3MF tests after STEP passes.
- Duplicate export requests do not start duplicate jobs.
- Manifest can represent preview plus multiple download outputs.

Completion checklist:

- [ ] STEP export route and worker path are complete.
- [ ] STEP artifact metadata includes content type, disposition, size, and hash.
- [ ] STL and 3MF use the proven export abstraction.
- [ ] Duplicate requests deduplicate by `workKey`.
- [ ] Download links are direct public artifact URLs.
- [ ] Phase tracker is updated.

## Phase 8: Operational Commands

Add enough CLI/Fly-run maintenance controls for MVP operations.

Deliverables:

- `validate-catalog`.
- `refresh-parameters`.
- `generate-preview`.
- `generate-export`.
- `jobs list/show/retry`.
- `cache list/invalidate/reconcile`.

Verification:

- CLI smoke tests against fixture data.
- Dry-run behavior where destructive or expensive actions are possible.
- Supersession invalidation test.

Completion checklist:

- [ ] Catalog validation command works.
- [ ] Parameter refresh, preview generation, and export generation commands work.
- [ ] Job inspection and retry commands work.
- [ ] Cache list, invalidate, and reconcile commands work.
- [ ] Invalidation marks superseded artifacts without deleting public objects.
- [ ] Phase tracker is updated.

## Phase 9: Runtime Hardening

Add robustness only as needed after the vertical slices work.

Possible additions:

- SQLite backup/snapshot runbook.
- Metrics and tracing.
- Rate limits and queue depth limits.
- Scheduled rebuilds.
- Cache lifecycle cleanup.
- Separate Fly worker process group only after moving to Postgres or another shared coordination backend.
- Web admin UI and authentication if CLI operations become insufficient.

Completion checklist:

- [ ] SQLite backup/snapshot expectations are documented or implemented.
- [ ] Minimal metrics/tracing are available for job IDs and work keys.
- [ ] Queue depth and request rate limits are defined if traffic requires them.
- [ ] Runtime runbooks cover stuck jobs, failed exports, and cache reconciliation.
- [ ] Any worker split or Postgres move is backed by a concrete need.
- [ ] Phase tracker is updated.
