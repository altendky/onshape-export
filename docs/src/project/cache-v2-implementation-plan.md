# Cache v2 Implementation Plan

This plan implements the forward-looking cache model in `docs/src/project/cache-model.md` as a hard cut. Existing v1 SQLite records, object keys, public URLs, manifests, helper names, and compatibility behavior may be deleted or replaced. No data or cache migration is required.

The implementation should prefer small, reviewable commits. Use subagents heavily for discovery, independent implementation slices, and post-change review. Each slice should include relevant tests with the implementation unless a live Onshape behavior question blocks testing; in that case, defer the live test behind a clear issue note and keep local unit or DB coverage where possible.

## Target Behavior

- Resolve catalog `versionId` to immutable `microversionId` before computing source identity.
- Compute `sourceHash` from resolved microversion identity, not catalog slug or `versionId`.
- Validate configuration values locally, canonicalize them as typed values, and compute `configHash` from the typed canonical payload.
- Use Onshape `configurationencodings` for every v2 export after local validation.
- Compute `optionsHash` from logical export intent only.
- Build the exact Onshape request before starting work and use `requestHash` as the authoritative export dedupe key.
- Persist translation start/final responses, result IDs, and response hashes for diagnostics and crash recovery.
- Always store successful Onshape downloads as private content-addressed raw payloads before local processing.
- Process public preview/download artifact sets from retained raw payloads.
- Treat artifact sets as the readiness and supersession unit, with primary files plus sidecars modeled together.
- Use DB-first product/status state. Do not require object-store public manifests in the initial v2 flow.
- Verify uploads before marking records ready. Use metadata verification for normal writes and reserve full read-back SHA-256 verification for repair, raw recovery, suspicious metadata, or strict modes.

## Non-Goals

- No migration from v1 tables or object keys.
- No compatibility layer for v1 routes, manifests, artifact keys, or work keys unless a current route shape is still useful for users.
- No workspace/latest source support in the first slice.
- No linked-document context unless live API testing proves it is needed.
- No ambiguous preview ZIP processing. Multi-scene or multi-result shapes should retain raw payloads and record a clear failure.

## Implementation Slices

These implementation slices are not individually tracked as GitHub issues. Use this document as the implementation checklist. GitHub issues are reserved for deferred validation work that needs live API access, production observation, or follow-up decisions.

Deferred validation issues:

- [#23 Cache v2 live validation: version to microversion resolution](https://github.com/altendky/onshape-export/issues/23)
- [#24 Cache v2 live validation: configurationencodings behavior](https://github.com/altendky/onshape-export/issues/24)
- [#25 Cache v2 live validation: export defaults and request repeatability](https://github.com/altendky/onshape-export/issues/25)
- [#26 Cache v2 live validation: translation result shapes](https://github.com/altendky/onshape-export/issues/26)
- [#27 Cache v2 live validation: external data headers and filenames](https://github.com/altendky/onshape-export/issues/27)

### 1. Replace v1 Schema With v2 Tables

Add migrations and Rust DB methods for the v2 model. Since this is a hard cut, remove or ignore v1 table assumptions in application code as each dependent slice lands.

Tables to add or replace:

- `source_resolutions`: catalog slug/source ids, `version_id`, resolved `microversion_id`, `source_hash`, diagnostics JSON, timestamps.
- `parameter_metadata`: keyed by `source_hash`, with raw configuration object key, normalized schema object key, schema hash/version, timestamps.
- `configuration_selections`: `source_hash`, `config_hash`, typed canonical values JSON, validation details JSON, timestamps.
- `configuration_encodings`: `source_hash`, `config_hash`, `encoded_id`, `query_param`, request JSON, response JSON, timestamps.
- `export_requests`: `request_hash`, `source_hash`, `config_hash`, `options_hash`, output kind/format, canonical request JSON, request builder version, status, timestamps.
- `translations`: `translation_id`, `request_hash`, start response JSON, final response JSON, poll state, result IDs JSON, response hashes, timestamps.
- `raw_payloads`: `raw_payload_hash`, private object key, content type, byte length, headers JSON, original filename, ZIP inventory JSON, timestamps.
- `postprocess_runs`: `postprocess_hash`, `raw_payload_hash`, processor name/version, policy JSON, status, log JSON, derived files JSON, timestamps.
- `artifact_sets`: `artifact_set_hash`, source/config/options/request/raw/postprocess hashes, output kind/format, status, primary object key, metadata JSON, timestamps, supersession fields.
- `artifact_files`: `artifact_set_hash`, role, logical path, object key, content type, byte length, sha256, metadata JSON.
- `jobs`: keep a queue table if useful, but export jobs must dedupe by `requestHash`, not v1 `workKey` identity.

Testing:

- DB migration smoke test.
- Insert/read/update tests for staged request, translation, raw payload, postprocess, artifact-set transitions.
- Dedupe tests for `requestHash`, `rawPayloadHash`, and artifact-set readiness.

### 2. Extract v2 Identity and Hashing

Move cache identity structs and helpers out of `main.rs` into a dedicated module such as `src/cache_model.rs`, keeping `src/cache_key.rs` as the canonical JSON/SHA-256 primitive.

Implement versioned preimage structs and helpers for:

- `source-v2`
- `config-v2`
- `options-v2`
- `request-v2`
- `response-v2`
- `postprocess-v2`
- `artifact-set-v2`

Testing:

- Golden vector tests for stable canonical payloads.
- Tests proving domain separation prevents cross-type collisions.
- Tests proving slug and public filename do not affect cache identity.
- Tests proving `versionId` does not participate in `sourceHash` once `microversionId` is resolved.

### 3. Resolve Version Sources to Microversions

Add `OnshapeClient::resolve_version_microversion` and route all cache work through a source-resolution step.

Implementation details:

- Keep catalog entries version-based for operator usability.
- Resolve `documentId + versionId` to `microversionId` before computing `sourceHash`.
- Persist resolution diagnostics.
- Fail clearly for workspace/latest sources until they are explicitly supported.

Testing:

- Unit tests for source identity preimages.
- DB tests for source-resolution reuse.
- Defer live endpoint assertions if credentials/API behavior are unavailable; document the required live test in the issue.

### 4. Typed Configuration Canonicalization

Update parameter validation so browser/catalog string submissions are converted into typed canonical values before hashing.

Initial supported values:

- quantity/number
- enum
- boolean
- text

Unsupported or ambiguous parameter types must fail explicitly or be represented with unsupported metadata. They must not silently participate in `configHash`.

Testing:

- Defaults are applied before hashing.
- Boolean/string/number canonicalization is stable.
- Unknown parameters honor `allow_unknown` policy but do not bypass typed canonicalization.
- Unsupported values fail with actionable diagnostics.

### 5. Onshape Configuration Encodings

Add `OnshapeClient::encode_configuration` using:

```text
POST /api/elements/d/{did}/e/{eid}/configurationencodings?versionId={vid}
```

Implementation details:

- Use typed canonical values as the application identity.
- Store `encodedId` and `queryParam` separately from `configHash`.
- Lookup `sourceHash + configHash` before calling Onshape.
- Use the returned encoded representation in export request bodies.

Testing:

- Request builder unit tests for endpoint, query string, and body.
- DB tests for encoding reuse.
- Defer live API shape validation if needed.

### 6. Canonical Export Request Builders

Refactor `onshape.rs` so request construction is separate from execution.

Each builder should return:

- operation name
- HTTP method
- path template and concrete identifiers
- query parameters
- body with explicit cache-relevant defaults
- encoded configuration string/id
- defaults policy version
- request builder version

Compute `requestHash` before starting an Onshape translation. Supported initial builders:

- preview glTF export
- STEP export
- generic STL translation
- generic 3MF translation

Testing:

- Hash changes when explicit body defaults change.
- Hash changes when request builder/defaults policy versions change.
- Hash does not include `translationId`, `externalDataId`, response JSON, raw byte hash, local processing, public object key, slug, or public filename.

### 7. Request-Hash Export Jobs

Refactor job enqueueing/execution so export jobs dedupe by `requestHash`.

Implementation details:

- Parameter refresh jobs may use a non-export job key.
- Preview/download export jobs should carry `requestHash`, source/config/options hashes, output kind/format, and model slug for UI grouping.
- Job status should reflect staged export progress where possible.

Testing:

- Repeated identical export requests enqueue once.
- Failed request jobs can be retried without changing cache identity.
- Ready request jobs are not requeued unless forced by explicit operator action.

### 8. Translation Persistence and Crash Recovery Foundations

Split Onshape export execution into staged calls:

- start translation and persist full start response plus `translationId`
- poll translation and persist final response plus result IDs
- download exactly one supported result or fail with a recorded diagnostic

Initial runtime behavior should fail clearly on unexpected multi-result shapes while keeping schemas capable of representing them later.

Testing:

- Unit tests for translation response parsing.
- DB tests for start/final response persistence.
- Failure tests for missing, failed, or multi-result responses.

### 9. Raw Payload Storage

Always store downloaded Onshape bytes privately before local processing.

Implementation details:

- Compute `rawPayloadHash` as SHA-256 of exact bytes.
- Store content-addressed raw object.
- Capture content type, byte length, response headers, original filename, and ZIP inventory.
- Dedupe raw payload records by `rawPayloadHash`.

Suggested object keys:

```text
onshape/raw/v1/{hashPrefix}/{rawPayloadHash}/{storageSafeOriginalFilename}
onshape/raw/v1/{hashPrefix}/{rawPayloadHash}/payload.bin
```

Testing:

- Raw payload hash is exact byte hash.
- Duplicate bytes reuse the raw payload record.
- ZIP inventory rejects unsafe paths and records safe entries.

### 10. Post-Processing From Raw Payloads

Move preview processing out of the current direct-export flow and make it consume retained raw payloads.

Strict initial preview input shapes:

- direct GLB
- direct glTF JSON
- ZIP with exactly one GLB
- ZIP with exactly one glTF viewer asset plus sidecars

Ambiguous or multi-scene ZIPs should retain raw payloads and record post-processing failure.

For STEP/STL/3MF downloads, initial post-processing can be identity processing with preserved metadata.

Testing:

- Direct GLB validation.
- Direct glTF validation.
- Single-GLB ZIP extraction.
- Single-glTF ZIP extraction with sidecars.
- Rejection for multiple GLBs, multiple glTF files, unsafe paths, and invalid files.
- `postprocessHash` changes when processor policy/version changes, not when public object key changes.

### 11. Artifact Sets and Public Object Publishing

Publish artifact sets as immutable public units.

Implementation details:

- Compute `artifactSetHash` from output kind/format, `sourceHash`, `configHash`, `optionsHash`, `requestHash`, `rawPayloadHash`, `postprocessHash`, and artifact-set schema version.
- Upload primary and sidecar public files under v2 paths.
- Store every file in `artifact_files` with role, logical path, object key, content type, byte length, and SHA-256.
- Mark the artifact set ready only after uploads verify.
- Supersession updates DB state rather than overwriting normal public objects.

Suggested object keys:

```text
previews/v2/{artifactSetHash}/{logicalPath}
artifacts/v2/{artifactSetHash}/{downloadFilename}
```

Testing:

- Artifact set is the unit of readiness.
- Sidecar failure prevents ready status.
- Artifact-set hash changes with relevant identity fields.
- Public object key and cosmetic download filename do not affect artifact-set identity.

### 12. DB-First Product, Status, and Manifest Behavior

Update server routes and rendering to read v2 DB state.

Implementation details:

- Product pages should query ready artifact sets for the selected source/configuration group.
- Status routes should report request/job/artifact-set state from DB.
- Do not require object-store public manifests for initial v2 flow.
- Remove v1 manifest rewrite assumptions once routes are DB-first.

Testing:

- Ready preview/download status returns public URLs from artifact files.
- Missing/queued/running/failed statuses are distinguishable.
- Superseded artifact sets no longer appear as active outputs.

### 13. Upload Verification and Repair Hooks

Add storage verification primitives and wire normal upload metadata verification into artifact readiness.

Implementation details:

- Add `StorageClient::head_object` or equivalent.
- Verify expected content length and available metadata before marking records ready.
- Keep full read-back SHA-256 verification available for repair/strict modes, but do not block initial normal writes on it unless configured.
- Add DB status fields that can represent raw-payload corruption, postprocess failure, upload failure, and repair-required states.

Testing:

- Metadata verification success/failure paths.
- Artifact set remains non-ready if verification fails.
- Strict/full read-back helper computes expected SHA-256 when used in tests.

### 14. Remove v1 Implementation Paths

After v2 paths are wired through application behavior, delete obsolete v1 helpers and assumptions.

Candidates include:

- v1 artifact keys and object keys
- v1 manifest generation and rewrites
- v1 `sourceHash` from `versionId`
- v1 hand-built Onshape configuration strings for exports
- v1 direct bytes-to-public-artifact flow
- v1 artifact table accessors no longer used by routes

Testing:

- Full `cargo fmt`.
- Full `cargo test`.
- Full `cargo clippy --all-targets --all-features` if available in the repo/toolchain.

## Future Session Prompt

Use this prompt in a fresh implementation session:

```text
We need to implement the hard-cut cache v2 model for this repository. Read `docs/src/project/cache-model.md` and `docs/src/project/cache-v2-implementation-plan.md` first. Treat v1 cache data, object keys, manifests, and compatibility behavior as disposable; no migration is required. Match the design doc.

Work slice-by-slice from `docs/src/project/cache-v2-implementation-plan.md`. The GitHub issues linked in that file are only for deferred live/API validation or follow-up decisions, not for the main implementation phases. Use subagents as much as possible:

- Before each substantial slice, launch an `explore` subagent to identify relevant files, current patterns, risks, and tests.
- For independent parts of a slice, launch `general` subagents in parallel when they can inspect or implement non-overlapping files safely.
- After each meaningful edit, launch a `general` subagent review focused on correctness, regressions, missing tests, and consistency with the plan/design doc.

For each implementation slice:

1. Read the relevant section of `docs/src/project/cache-v2-implementation-plan.md` and any deferred validation issue that applies to the slice.
2. Implement the smallest correct hard-cut v2 change; delete v1 code when it is replaced and no longer needed.
3. Add or update tests in the same slice. If live Onshape behavior blocks a test, defer only that live check by referencing or updating the relevant GitHub issue and keep deterministic local coverage.
4. Run targeted tests, then broader verification when the slice is complete.
5. Inspect `git status`, `git diff`, and recent commits before committing.
6. Commit each completed slice separately with a GPG-signed commit. Do not skip signing. If signing fails because approval is needed, ask whether to retry.
7. Reference deferred validation issues only when the commit directly addresses or updates that deferred work.

Do not create v1 compatibility shims unless a current route shape remains useful for user-facing behavior. Do not preserve v1 SQLite records, object keys, public URLs, or manifests. Keep product/status behavior DB-first and do not require public object-store manifests for the initial v2 implementation.
```

## Verification Command Set

Run these as applicable:

```text
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```
