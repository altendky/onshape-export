# Onshape API Flow

## Source Model Identity

Initial model sources are immutable document versions:

```text
document id: did
version id: vid
```

Workspaces are intentionally out of scope for the first version.

For the implemented v2 cache model, catalog entries remain version-based for operator usability, but the service resolves the version to its immutable document microversion before computing `sourceHash`. The version-to-microversion mapping is stored for diagnostics and traceability, and multiple `versionId` aliases can point at the same resolved `sourceHash`.

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

The implemented v2 cache path always uses Onshape's encoding endpoint after local validation and typed canonicalization:

```text
POST /api/elements/d/{did}/e/{eid}/configurationencodings?versionId={vid}
```

Use `linkDocumentId` as well if the versioned element must be accessed through a linked document context.

Local validation confirms that every submitted parameter is supported by the normalized schema and that every value can be represented as a typed canonical value before any network call. Typed canonicalization normalizes those values into the same application payload that produces `configHash`, and equivalent supported length spellings reuse the same encoding request shape. A v2 export does not fall back to hand-built configuration strings if encoding fails; transient endpoint failures follow the normal retry policy, malformed or terminal responses are recorded in diagnostics, and the export stays failed until a valid Onshape encoding is available.

The app intentionally accepts only basic numeric values at this boundary. Dimensioned number controls submit a plain decimal value and a unit selected from the supported units for that parameter dimension. Onshape expressions such as `2 + 2` or `(4mm) / (1mm)` can be valid Onshape inputs, but they are not part of local canonicalization because evaluating them would require either reimplementing Onshape expression semantics or spending extra API calls. Generated canonical request values may still use simple parenthesized fractions, for example `(127/5000) m`, because those are produced by the app from exact rational values rather than accepted from users as free-form expressions.

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
    "resolution": "FINE"
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

The service owns retrieval and immutable retention of raw Onshape exports before
any local transformation. Onshape geometry 3MF is a geometry export, not a
slicer project 3MF. It is only one candidate input to the proposed external
slicer adapters; STEP, STL, or another source-neutral geometry package may prove
more suitable. Adapter input selection must not move Onshape credentials,
translation polling, or raw-payload ownership into an adapter. See
[Slicer Project 3MF Adapters](slicer-3mf-adapters.md).

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
  "grouping": true,
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false
}
```

The app explicitly requests grouped output for STEP exports. Live testing on `onshape-model` showed that omitting `grouping` returned a ZIP with one STEP file per surface, while `grouping: true` returned a single STEP file for the same configuration. STEP post-processing still checks the actual bytes: direct STEP is published as `.step`, a ZIP containing exactly one STEP file is extracted, and a multi-file STEP ZIP is preserved as `.zip` with `application/zip` instead of being mislabeled as plain STEP.

For STL and 3MF, use the generic translation endpoint unless format-specific async endpoints prove better:

```text
POST /api/partstudios/d/{did}/v/{vid}/e/{eid}/translations
POST /api/assemblies/d/{did}/v/{vid}/e/{eid}/translations
```

Generic 3MF body shape:

```json
{
  "formatName": "3MF",
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false,
  "configuration": "...",
  "grouping": true,
  "resolution": "fine"
}
```

Generic STL body shape:

```json
{
  "formatName": "STL",
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false,
  "configuration": "...",
  "grouping": true,
  "stlMode": "BINARY",
  "resolution": "fine"
}
```

The generic async translation endpoint documents lowercase `resolution` values for STL and 3MF: `coarse`, `medium`, and `fine`. This differs from the async GLB `meshParams.resolution` enum, which accepts uppercase values such as `FINE`, and from synchronous STL endpoints, which use query names such as `angleTolerance`, `chordTolerance`, `maxFacetWidth`, `minFacetWidth`, `units`, and `mode`.
The app also sends `grouping: true` for generic STL and 3MF translations because the product currently presents one grouped download artifact per requested format. Separate-per-part packages are deferred until there is a deliberate catalog option.

Current live-test evidence:

- STEP remains explicitly requested as `AP242` because omitting the value produced different bytes than an explicit AP242 request.
- STEP, STL, and 3MF exports request `grouping: true` explicitly. For STEP, omitting `grouping` produced a ZIP package for the tested multi-surface model; explicit `grouping: true` produced a plain STEP file.
- GLB preview accepts uppercase `FINE`; lowercase `fine` failed.
- 3MF generic translation accepts lowercase `fine`; uppercase `FINE` failed after the translation started.
- STL generic translation accepts `resolution: "fine"` and `stlMode: "BINARY"`; `stlMode` changed the output encoding and size, but generic async STL `resolution` did not affect tested outputs. The app still sends lowercase `fine` as catalog-requested high-quality intent.
- Numeric mesh tolerances are intentionally not implemented yet. They require more model-scale-specific testing before becoming catalog semantics or cache identity.

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
