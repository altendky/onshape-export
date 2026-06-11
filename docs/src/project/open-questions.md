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

- Which exact `formatName` values should be used for STL, 3MF, and GLB on generic translation endpoints?
- Are format-specific endpoints better than generic translation endpoints for STEP and GLB in practice?
- Does synchronous Part Studio glTF/GLB export produce suitable single-file GLB previews faster than async export?
- Do Assemblies and Part Studios need different default export options?
- Which exports return multiple `resultExternalDataIds` in practice, and which should initial v2 reject as unsupported multi-result shapes?
- Does Onshape expose a reliable version-to-microversion resolution path for every versioned Part Studio and Assembly source we need?
- Which download response headers are reliable enough for diagnostics or conditional requests, such as `ETag`, `Last-Modified`, `Content-Disposition`, and content length?

## Parameter Handling

- Which Onshape configuration parameter types appear in target models?
- What exact typed canonical form should v2 use for each supported Onshape parameter type?
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

## Plan Gaps

- What public status route shape should expose `jobId`, `groupId`, ready outputs, retry hints, and safe failure messages?

## Resolved Working Decisions

Resolved or working MVP decisions are tracked in [Decisions](decisions.md).
