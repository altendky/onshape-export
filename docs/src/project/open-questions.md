# Open Questions

## Runtime

- What initial Onshape export concurrency limit is safe beyond the current default of `1`?
- Is the current explicit `ops backup` SQLite snapshot enough, or are platform snapshots or Postgres needed before production use?
- What Tigris public URL shape should be used for stable artifact links?
- When should SQLite be replaced with Postgres?

## Onshape Auth

- Which API key permissions are required for versioned configuration reads and exports?
- Should the existing `onshape-mcp` auth/client crates be extracted or reused directly?

## Onshape Export Details

- Which exact `formatName` values should be used for STL, Onshape geometry 3MF, and GLB on generic translation endpoints?
- Are format-specific endpoints better than generic translation endpoints for STEP and GLB in practice?
- Does synchronous Part Studio glTF/GLB export produce suitable single-file GLB previews faster than async export?
- Do Assemblies and Part Studios need different default export options?
- Which exports return multiple `resultExternalDataIds` in practice? The
  [geometry input characterization](onshape-geometry-input-characterization.md)
  observed exactly one in its controlled matrix; zero, multiple, and duplicate
  result shapes remain unproven and fail closed.
- Does Onshape expose a reliable version-to-microversion resolution path for every versioned Part Studio and Assembly source we need?
- Which download response headers are reliable enough for diagnostics or conditional requests, such as `ETag`, `Last-Modified`, `Content-Disposition`, and content length?

## Parameter Handling

- Which Onshape configuration parameter types appear in target models?
- Does `configurationencodings` behave consistently for default, explicit-default, and non-default values across Part Studios and Assemblies?
- What numeric precision or step rules should each model expose?
- How should hidden or conditionally visible parameters be handled?
- Which unsupported Onshape parameter types need explicit `unsupportedReason` metadata?

## Preview UX

- Which models should auto-generate previews after debounce versus requiring an explicit button?
- What preview tessellation defaults keep GLB files small enough?
- Should thumbnails be generated in addition to GLB previews?

## Cache And Admin

- How long should failed jobs suppress retries?
- Should admin rebuilds generate all formats or only missing artifacts?
- What exact v2 database schema names and relationships should represent requests, translations, raw payloads, post-processing, artifact sets, artifact files, and failure events?
- When should future public object-store manifests be materialized from DB state, and how much metadata should they expose?
- Reconsider v2 repair and overwrite semantics before production use: raw payload repair, public artifact repair, DB/object drift, corrupt-but-public objects, missing sidecars, partial uploads, concurrent repair races, CDN behavior, and supersede-versus-repair boundaries.
- What live experiments must pass before locking v2 request defaults, result cardinality, raw payload retention, and post-processing behavior?

## Service-Owned Generator Integration

- Which Onshape export or neutral geometry representation best preserves the
  geometry, units, object identity, assemblies, and metadata generators need?
- Which additional controlled sources can prove or reject the mappings left
  unproven by the
  [geometry input characterization](onshape-geometry-input-characterization.md),
  including duplicate names, nested/suppressed occurrences, non-default
  configurations, and selected Part Studio parts?
- What exact CLI invocation, file transport, JSON schema, error model, and
  atomic-write contract should the prototype use?
- Which generator, protocol, dialect, and slicer-version compatibility windows are
  supportable, and how should incompatibility be reported?
- Which independent service validation and normalization guarantees are required
  before exact candidate artifact bytes may be published?
- How should exact target-side validation inputs or tools be packaged, approved,
  and invoked without moving target schemas or fixtures here?
- How are released generator packages discovered, acquired, approved, installed,
  verified, retained for rollback, revoked, distributed, and deployed?

These questions are governed by the local
[Slicer Project Generator Integration Policy](slicer-project-generator-integration.md).

## Generator-Project-Owned Questions

- Must each generator's output be byte-deterministic, canonically equivalent
  after normalization, or semantically equivalent in supported slicer versions?
- Which Bambu Studio, OrcaSlicer, and PrusaSlicer features form a genuinely
  shared subset, and which require separate schema, implementation, provenance,
  and validation?
- Which target-derived schemas, fixtures, evidence, and release checks support
  each capability and slicer-version claim?

These questions belong in
[`slicer-project-generators`](https://github.com/altendky/slicer-project-generators)
and are governed by its pinned
[Slicer Project Generator Provenance Policy](https://github.com/altendky/slicer-project-generators/blob/ced6585d5a8e1a47690e7eabdf92beaa7fea7fc4/docs/src/project/slicer-project-generator-provenance.md).

## Plan Gaps

- What public status route shape should expose `jobId`, `groupId`, ready outputs, retry hints, and safe failure messages?

## Resolved Working Decisions

Resolved or working MVP decisions are tracked in [Decisions](decisions.md).
