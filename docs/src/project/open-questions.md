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
- How should multiple `resultExternalDataIds` be represented and served?

## Parameter Handling

- Which Onshape configuration parameter types appear in target models?
- Can all parameter types be encoded safely without calling `configurationencodings`?
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
- What exact manifest/index fields should mark superseded artifacts after exporter version changes?
- Which current deletion-based cleanup commands should remain after normal invalidation moves to supersession?

## Plan Gaps

- How should RFC 8785 canonical JSON be introduced without breaking existing cache keys?
- Should existing `catalog/models.json` data migrate directly to `catalog/v1/`, or should source changes land first?
- What public status route shape should expose `jobId`, `groupId`, ready outputs, retry hints, and safe failure messages?

## Resolved Working Decisions

Resolved or working MVP decisions are tracked in [Decisions](decisions.md).
