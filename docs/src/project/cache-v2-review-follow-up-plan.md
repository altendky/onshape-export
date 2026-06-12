# Cache v2 Review Follow-Up Plan

This page records the review findings that followed the main cache v2 hard cut on branch `cache-v2-hard-cut`. The follow-up plan is now complete on this branch; the sections below are retained as historical implementation notes and acceptance criteria for the landed fixes.

## Goals

- Make `requestHash` authoritative for export readiness, dedupe, and status.
- Ensure configuration encodings are derived from the same canonical typed values used for `configHash`.
- Prevent unsafe or corrupt raw payload reuse.
- Fail clearly for unsupported parameter shapes and incomplete preview ZIPs.
- Finish the hard cut by blocking or removing legacy queued export work.
- Align docs and tests with the implemented v2 behavior.

## Non-Goals

- No reintroduction of v1 manifests, v1 artifact keys, or v1 request identity.
- No migration of v1 cache data into v2 identities.
- No new live Onshape feature work beyond what is needed to validate the fixes below.

## Findings To Address

1. Ready artifact lookups can bypass `requestHash`.
2. Configuration encoding uses non-canonical submitted values.
3. Unsafe ZIP inventory failures are persisted before rejection.
4. Unsupported or ambiguous parameter types silently become text.
5. Legacy v1 export jobs can still execute.
6. Retained raw payload bytes are not verified against `rawPayloadHash` before post-processing.
7. Zipped glTF sidecar completeness is not validated.
8. `optionsHash` includes exporter package version.
9. `source_resolutions` cannot retain multiple version aliases for one microversion.
10. Translation response hashing does not use the canonical `response-v2` helper.
11. Several docs still describe completed v2 work as missing.

## Implementation Slices

### 1. Make `requestHash` The Readiness Key

Update preview/download readiness and status paths so a ready artifact only satisfies the exact canonical request that produced it.

Implementation details:

- Thread `requestHash` through preview/download page rendering, direct generation helpers, worker execution, and status lookups.
- Replace logical-output-only ready lookups with exact request-based lookups where export execution or status is concerned.
- Keep logical output grouping for UI display only where that does not affect dedupe or execution decisions.
- Ensure a request builder or defaults-policy change produces a new request, a new job key, and a cache miss until the new artifact set is ready.

Testing:

- A ready artifact for an older request must not satisfy a newer `requestHash`.
- Queued/running status must report against the latest exact request, not any matching logical output.
- Repeated identical requests still dedupe to one job and one ready artifact set.

### 2. Canonicalize Configuration Encoding Inputs

Ensure Onshape configuration encoding requests are built from typed canonical values, not the first raw spelling submitted by a user.

Implementation details:

- Introduce a canonical request projection for validated parameter values.
- Use that projection when calling `encode_configuration` and when persisting any cached encoding request payload.
- Keep `configHash` and encoding reuse keyed by the same canonical meaning.
- Preserve user-facing display values separately from encoding identity if needed.

Testing:

- Equivalent numeric spellings such as `1`, `1.0`, and `01.0` produce identical encoding request payloads.
- Unit-bearing defaults and submitted values normalize to one encoding request shape.
- Reordering or cosmetic input changes do not alter cached encoding reuse.

### 3. Reject Unsupported Parameter Shapes Explicitly

Stop silently treating unknown or ambiguous Onshape parameter metadata as free-form text.

Implementation details:

- Extend normalized parameter metadata with an explicit unsupported state or failure reason.
- Fail validation for unsupported schema entries unless a clearly defined temporary operator policy says otherwise.
- Ensure `allow_unknown` does not let unsupported typed parameters bypass canonicalization into `configHash`.
- Record actionable diagnostics for unsupported parameter types.

Testing:

- Unknown parameter schema shapes fail with a clear diagnostic.
- `allow_unknown` preserves only intentionally unknown submitted names, not unsupported normalized schema entries.
- Unsupported parameters do not participate in `configHash`.

### 4. Harden Raw Payload Persistence And Reuse

Do not persist reusable raw payload/source mappings until ZIP safety checks succeed, and verify retained bytes before reuse.

Implementation details:

- Run ZIP inventory and safety validation before inserting reusable raw payload source mappings.
- Decide whether failed ZIP inspection should leave a raw payload record without a reusable source link, or avoid persistence entirely.
- Recompute SHA-256 when loading persisted raw payload bytes and compare it to `rawPayloadHash` before post-processing.
- Add explicit DB-visible status or diagnostic fields for raw corruption, unsafe ZIPs, and repair-required states.

Testing:

- A ZIP containing one valid asset plus an unsafe extra entry never becomes ready, including after retry.
- Corrupt or replaced stored raw bytes are detected before post-processing.
- Duplicate valid raw payloads still dedupe by exact byte hash.

### 5. Validate glTF ZIP Sidecar Completeness

Require a zipped `.gltf` preview to include all referenced safe sidecar assets before publishing.

Implementation details:

- Parse `.gltf` JSON enough to collect referenced buffer and image URIs.
- Reject absolute, parent-traversing, data-URI, or missing referenced assets unless explicitly supported.
- Confirm every referenced sidecar exists in the ZIP with a safe normalized path.
- Continue rejecting ambiguous multi-viewer ZIPs.

Testing:

- A `.gltf` ZIP with all referenced sidecars succeeds.
- Missing referenced `.bin` or image assets fail clearly.
- Unsafe referenced paths fail clearly.

### 6. Finish The Hard Cut For Queued Export Jobs

Prevent old queued export jobs from running outside the v2 request-hash execution path.

Implementation details:

- Choose one hard-cut behavior and document it in code and migration notes:
  - delete or supersede legacy export jobs during migration/startup, or
  - refuse to execute non-`work-v2:export:` export jobs in the worker.
- Keep non-export parameter refresh jobs working as needed.
- Ensure status reporting does not imply v1 export jobs are part of current execution state.

Testing:

- Pre-v2 queued preview/download jobs are ignored, retired, or converted exactly as designed.
- Only request-hash export work keys are executable for preview/download jobs.

### 7. Tighten Identity Metadata And Hashing

Close the remaining identity-model mismatches.

Implementation details:

- Remove package `EXPORTER_VERSION` from `optionsHash`; keep only logical export intent plus explicit options/defaults policy versions.
- Revisit `source_resolutions` keying so multiple `versionId` values can map to the same resolved `sourceHash` without overwriting each other.
- Persist canonical `response-v2` hashes for translation diagnostics instead of raw final-response SHA-256 only.

Testing:

- Package version changes alone do not change `optionsHash`.
- Multiple version aliases for one microversion can coexist in `source_resolutions`.
- Response hash tests prove canonical helper usage and domain separation.

### 8. Update Docs To Match The Implemented State

Bring high-level docs back in sync once the above fixes land.

Files to update:

- `README.md`
- `docs/src/project/index.md`
- `docs/src/project/caching.md`
- `docs/src/project/implementation.md`
- Any design page whose current-status notes were invalidated by the hard cut.

Testing:

- Manual doc review for contradictions about typed canonicalization, upload verification, translation persistence, and v2 readiness.

## Recommended Order

1. Slice 1: request-hash readiness.
2. Slice 2: canonical configuration encoding inputs.
3. Slice 4: raw payload persistence and reuse hardening.
4. Slice 6: legacy queued export job hard cut.
5. Slice 3: unsupported parameter handling.
6. Slice 5: glTF ZIP sidecar completeness.
7. Slice 7: remaining identity metadata and hashing cleanup.
8. Slice 8: docs synchronization.

This order fixes the highest-risk cache correctness and unsafe reuse issues first, then cleans up schema/identity edges and documentation.

## Verification Command Set

Run these as applicable after each slice and again after the full set lands:

```text
cargo fmt
cargo test
cargo clippy --all-targets --all-features
```

Add targeted tests for each slice before relying on broad verification alone.

## Completion Status

- Slice 1 landed: `requestHash` is authoritative for preview/download readiness, worker dedupe, and status.
- Slice 2 landed: configuration encoding requests are derived from canonical typed values, not raw submitted spellings.
- Slice 3 landed: unsupported or ambiguous normalized parameter shapes fail validation explicitly and do not enter `configHash`.
- Slice 4 landed: raw payload source mappings are persisted only after ZIP safety checks succeed, and retained bytes are re-hashed before reuse.
- Slice 5 landed: zipped glTF previews require referenced safe sidecars and fail clearly for missing or unsafe references.
- Slice 6 landed: legacy non-`work-v2:export:` preview/download jobs are retired in the worker hard cut.
- Slice 7 landed: `optionsHash` excludes package version noise, source resolution aliases can coexist for one microversion, and translation diagnostics use canonical `response-v2` hashing.
- Slice 8 landed: high-level docs were updated to match the implemented v2 behavior.
