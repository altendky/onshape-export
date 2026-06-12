# Onshape API Flow

## Source Model Identity

Initial model sources are immutable document versions:

```text
document id: did
version id: vid
```

Workspaces are intentionally out of scope for the first version.

For the v2 cache model, catalog entries may remain version-based, but the service should resolve the version to its immutable document microversion before computing `sourceHash`. Store the version-to-microversion mapping for diagnostics and traceability.

Resolution must complete before writing source-scoped cache records or artifacts. Transient resolution failures should follow the Onshape retry/backoff policy and record diagnostics or `failureReason`; terminal failures should abort the export before any partial cache artifacts are published. If a previously stored `versionId` to `microversionId` mapping later resolves differently or becomes inconsistent, compute a new `sourceHash` from the new microversion and mark the old mapping/cache state stale or orphaned for reconciliation diagnostics instead of mutating existing artifacts in place.

## Authentication

The MVP assumes server-owned Onshape API keys configured as deployment secrets. The Rust service signs requests server-side, and credentials are never exposed to browsers.

This assumption must be verified with real calls before the export vertical slice is considered complete:

- Fetch versioned configuration metadata for a Part Studio.
- Fetch versioned configuration metadata for an Assembly.
- Create, poll, and download a GLB export.
- Create, poll, and download STEP, STL, and 3MF exports.
- Confirm required access for linked-document assembly contexts.

Current branch status: API-key signing is implemented, but the docs do not record successful real Onshape smoke-test results yet.

## Parameter Discovery

Fetch configuration parameters for a Part Studio or Assembly:

```text
GET /api/elements/d/{did}/v/{vid}/e/{eid}/configuration
```

The raw response should be cached in Tigris and normalized into a UI schema. The normalized schema should preserve enough source data to reconstruct or encode Onshape configuration values later.

## Configuration Encoding

Configuration values may be represented as an Onshape configuration string:

```text
parameterId=value;other=value
```

Current v1 code may hand-build this string for simple cases. The v2 cache model should always use Onshape's encoding endpoint after local validation and typed canonicalization:

```text
POST /api/elements/d/{did}/e/{eid}/configurationencodings?versionId={vid}
```

Use `linkDocumentId` as well if the versioned element must be accessed through a linked document context.

Local validation should confirm that every submitted parameter is supported by the normalized schema and that every value can be represented as a typed canonical value before any network call. Typed canonicalization should normalize those values into the same application payload that produces `configHash`. A v2 export must not fall back to hand-built configuration strings if encoding fails; retry transient endpoint failures through the normal Onshape retry policy, record malformed or terminal responses in diagnostics, and surface the export as failed until a valid Onshape encoding is available. The hand-built path remains v1 compatibility only.

Request body shape from the OpenAPI schema:

```json
{
  "parameters": [
    {
      "parameterId": "...",
      "parameterValue": "..."
    }
  ]
}
```

Encoded configuration results should be cached by source identity, `configHash`, and normalized encoding request context. The encoding response is Onshape's request representation; it does not replace the application's canonical `configHash`.

The encoding request context should include only fields that can affect the encoding result, such as the endpoint/spec version when relevant, resolved source access context, `linkDocumentId`, and the canonical encoding request body. Normalize and hash that context into an `encodingContextHash`; avoid raw headers, user/session identifiers, or tenant data unless live testing proves they affect encoding. The cache key should combine `sourceHash`, `configHash`, and `encodingContextHash`, and obsolete context variants should be pruned or expired to avoid unbounded cache growth.

## Preview Export

Preview is a GLB export for the selected configuration. It is separate from the user's final download format.

Part Studio options:

```text
GET /api/partstudios/d/{did}/v/{vid}/e/{eid}/gltf?configuration=...
POST /api/partstudios/d/{did}/v/{vid}/e/{eid}/export/gltf
```

Assembly option:

```text
POST /api/assemblies/d/{did}/v/{vid}/e/{eid}/export/gltf
```

The async GLB path is more consistent across Part Studios and Assemblies. The synchronous Part Studio endpoint may be useful later if it proves faster and reliable.

Preferred preview requirement: even if Onshape endpoint names use `gltf`, request grouped output and cache GLB when Onshape provides it. If Onshape returns direct glTF JSON, publish it as the viewer artifact. If Onshape returns a ZIP with exactly one glTF viewer asset instead, extract and publish the `.gltf` plus sidecars under the same preview identity and retain the original ZIP as an operational sidecar. A glTF viewer asset is the single primary `.gltf` file whose JSON can be parsed as glTF and whose referenced external buffers/images are present as safe sidecars. Safe sidecars are ZIP entries with normalized relative paths that stay inside the artifact set and serve viewer dependencies, such as `.bin`, image files, or validated metadata. The target cache model should treat the primary viewer file plus sidecars as one preview artifact set.

Current branch status: zipped Onshape preview responses use exactly one valid `.glb` when present. Otherwise, a ZIP with exactly one `.gltf` entry publishes that viewer object and safe sidecar files beside it. ZIPs with multiple GLB or `.gltf` viewer entries, no viewer entry, unsafe paths, corrupt archives, malformed viewer files, or missing referenced files should fail with a clear post-processing error and preserve diagnostics. The original ZIP is retained privately as a raw payload rather than relying on a public fixed filename.

Async GLB body shape should include the configuration under `advancedParams.configuration`:

```json
{
  "advancedParams": {
    "configuration": "..."
  },
  "meshParams": {
    "resolution": "MEDIUM"
  },
  "grouping": true,
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false
}
```

Preview cache identity must keep cache layers separate. Logical Onshape export intent such as mesh settings, grouping behavior, and future orientation options belongs in `optionsHash`; `isYAxisUp` is an illustrative orientation option, not a currently implemented schema field. Exact request defaults and builder changes belong in `requestHash` through fields such as `defaultsPolicyVersion` and `requestBuilderVersion`, the camelCase field name for the request builder version described in the cache model. Local extraction, fallback, merge, packing, validation, and tool versions belong in `postprocessHash`.

## Download Exports

Supported user download formats:

- STEP
- STL
- 3MF

For STEP, use format-specific async endpoints where available:

```text
POST /api/partstudios/d/{did}/v/{vid}/e/{eid}/export/step
POST /api/assemblies/d/{did}/v/{vid}/e/{eid}/export/step
```

STEP body shape should include the configuration under `advancedParams.configuration`:

```json
{
  "advancedParams": {
    "configuration": "..."
  },
  "stepVersionString": "AP242",
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false
}
```

For STL and 3MF, use the generic translation endpoint unless format-specific async endpoints prove better:

```text
POST /api/partstudios/d/{did}/v/{vid}/e/{eid}/translations
POST /api/assemblies/d/{did}/v/{vid}/e/{eid}/translations
```

Generic body shape:

```json
{
  "formatName": "3MF",
  "storeInDocument": false,
  "configuration": "..."
}
```

Exact `formatName` values should be verified with:

```text
GET /api/translations/translationformats
```

## Async Translation Polling

Async export endpoints return a translation id. Poll until complete:

```text
GET /api/translations/{tid}
```

Terminal states:

- `DONE`
- `FAILED`

When done, the response includes `resultExternalDataIds`. Download the first result for single-file exports, while keeping the manifest schema able to represent multiple outputs:

```text
GET /api/documents/d/{did}/externaldata/{fid}
```

## Polling Policy

Use conservative polling with backoff:

```text
2s, 4s, 8s, 15s, 30s max interval
```

Back off on Onshape rate limits and server errors. Record failure details in SQLite and, when useful, Tigris for operational inspection.

## Content Types

Expected outputs:

| Format | Browser Use | Storage Extension |
| --- | --- | --- |
| GLB | Preview | `.glb` |
| STEP | Download | `.step` |
| STL | Download | `.stl` |
| 3MF | Download | `.3mf` |

Prefer GLB for browser preview even when the user downloads STL or 3MF.
