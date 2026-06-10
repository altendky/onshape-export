# Forward-Looking Cache Model

This document describes the target cache model for Onshape previews and downloads. It is forward-looking design guidance. The current implementation snapshot remains in [Caching](caching.md).

The main design goal is to keep every cache boundary explicit: selected source, selected configuration, logical export options, exact Onshape request, Onshape translation response, raw Onshape bytes, local post-processing, public artifact sets, and manifests.

## Reference Sources

- Onshape OpenAPI: `https://cad.onshape.com/api/openapi`
- Onshape import/export guide: `https://onshape-public.github.io/docs/api-adv/translation/`
- Onshape configuration guide: `https://onshape-public.github.io/docs/api-adv/configs/`
- Onshape metadata guide: `https://onshape-public.github.io/docs/api-adv/metadata/`
- Current export client: `src/onshape.rs`
- Current hash helpers: `src/cache_key.rs`
- Current artifact, manifest, and worker flow: `src/main.rs`
- Current SQLite records: `src/db.rs`

## Principles

- Cache identity must not depend on model slugs or public filenames.
- Onshape source identity and selected configuration identity are separate from export request identity.
- Onshape translation IDs and external data IDs identify one attempt/result, not a reusable deterministic cache key.
- Raw Onshape payloads are distinct from locally processed viewer or download artifacts.
- Local post-processing changes should not require another Onshape export when the raw payload is retained.
- Artifact sets must model primary files plus sidecars as one unit.
- Preserve Onshape-provided filenames and ZIP entry names whenever available. Store roles in metadata instead of renaming raw files to fixed names like `source.zip`.
- Public artifact objects are immutable in normal operation. Supersession updates indexes and manifests rather than overwriting existing public files.

## Cache Layers

| Layer | Purpose | Key Inputs | Stored Outputs |
| --- | --- | --- | --- |
| Source identity | Identify the immutable Onshape element being exported. | `documentId`, `versionId` or resolved `microversionId`, `elementId`, `elementKind`, link-document context. | `sourceHash`, source metadata, resolved version/microversion diagnostics. |
| Parameter metadata | Preserve and normalize Onshape configuration schema. | `sourceHash`, Onshape configuration response, local schema version. | Raw configuration JSON, normalized parameter schema, schema hash/version. |
| Configuration selection | Identify validated parameter values. | `sourceHash`, normalized schema hash/version, typed canonical values after defaults and overrides. | `configHash`, canonical values, validation details. |
| Configuration encoding | Cache Onshape's encoded configuration string. | `sourceHash`, `configHash`, encoding request body, `linkDocumentId`. | `encodedId`, `queryParam`, decoded parameters when available. |
| Export options | Identify user/catalog-visible export intent. | Format, preview/download settings, orientation, grouping, selection filters, options schema version. | `optionsHash`, logical options payload. |
| Onshape request | Identify the exact wire request sent to Onshape. | Source/config/options, endpoint, method, path, full body with known defaults filled in, defaults policy version. | `requestHash`, canonical request JSON, request builder version. |
| Translation attempt | Resume and diagnose an Onshape translation. | `requestHash`, `translationId`. | Start/final response JSON, poll state, result IDs, `responseHash`. |
| Raw payload | Preserve exact downloaded Onshape bytes. | Downloaded bytes and response headers. | `rawPayloadHash`, raw object, content type, length, headers, original filename, ZIP inventory. |
| Post-processing | Identify local transformation from raw payload to artifacts. | `rawPayloadHash`, processor version, processing policy and tool versions. | `postprocessHash`, processing log, derived file manifest. |
| Artifact set | Publish one logical output as primary plus sidecars. | `sourceHash`, `configHash`, `optionsHash`, `requestHash`, `rawPayloadHash`, `postprocessHash`. | Primary artifact, sidecar artifacts, file hashes, `artifactSetHash`, status. |
| Manifest | Present application state for one source/configuration group. | `groupId`, ready/missing/superseded artifact sets. | Public manifest JSON, active output pointers, supersession history. |

## Identity Model

### `sourceIdentity` And `sourceHash`

`sourceIdentity` should identify the Onshape geometry source independently of the catalog slug.

For versioned sources, include:

- `documentId`
- `versionId`
- `elementId`
- `elementKind`
- `linkDocumentId` when needed
- resolved version `microversion` as stored metadata, not necessarily as part of the v1 hash

For any future workspace source, resolve the mutable workspace to a current document microversion before caching exports. Onshape exposes `GET /documents/d/{did}/{wv}/{wvid}/currentmicroversion`, whose response includes `microversion`. Workspace exports should key by the resolved microversion, not by the mutable workspace ID alone.

### `configHash`

`configHash` identifies one validated parameter selection for one source.

Include:

- `sourceIdentity` or `sourceHash`
- parameter schema hash or schema version
- configuration canonicalization version
- typed canonical values after server-side validation
- defaults applied from Onshape configuration metadata
- catalog overrides that affect accepted values or default values
- unit normalization version when quantity values are normalized

Current code still stores parameter values as strings. The target model should move to typed values such as quantity, enum, boolean, and text before relying on `configHash` for long-lived compatibility.

### Configuration Encoding Identity

Onshape configuration strings can be hand-built for simple cases, but the robust target is to use Onshape's encoding endpoint:

```text
POST /api/elements/d/{did}/e/{eid}/configurationencodings?versionId={vid}
```

Store both returned values:

- `encodedId`, used in async export bodies.
- `queryParam`, used by query-string APIs.

The encoding response does not replace `configHash`. It is an Onshape representation of the selected configuration, while `configHash` is the application identity for validated values.

### `optionsHash`

`optionsHash` should identify logical export intent, not the exact wire request and not local processing.

Include:

- output family: preview, STEP, STL, 3MF
- format-specific logical options, such as STEP version
- preview quality, mesh resolution or explicit tolerances
- orientation and scale choices
- grouping policy
- hidden-entity policy
- selected part IDs or assembly occurrences when supported
- option schema version

Do not include:

- `translationId`
- `externalDataId`
- response JSON
- raw byte hash
- local glTF extraction/merge/packing recipe
- public object key

### `requestHash`

`requestHash` identifies the exact Onshape request we intend to send. It should be the dedupe key for starting Onshape work.

Include:

- API base host class when behavior may differ, such as Onshape enterprise domains
- OpenAPI/spec version observed by the implementation when available
- operation name, such as `createPartStudioExportGltf`
- HTTP method
- path template and concrete path identifiers
- query parameters
- request body after local default filling
- encoded configuration string
- defaults policy version
- request builder version

Use the same RFC 8785/JCS canonical JSON hashing approach used by current `sourceHash`, `configHash`, and `optionsHash` helpers.

### `responseHash`

`responseHash` is for diagnostics and recovery. Hash canonical start and final translation response JSON separately or together.

Do not use `responseHash` as the deterministic export cache key. Repeated identical requests are expected to produce different `translationId` values.

### `rawPayloadHash`

`rawPayloadHash` is the SHA-256 of exact downloaded bytes from Onshape external data or blob element download.

This is the strongest identity for raw Onshape output. It should be used to avoid storing duplicate raw payloads and to re-run local processing without another Onshape request.

### `postprocessHash`

`postprocessHash` identifies the local processing recipe applied to a raw payload.

Include:

- processor name and version
- ZIP extraction policy
- accepted input shape policy: GLB, direct glTF, ZIP with GLB, ZIP with single glTF, ZIP with multiple glTF files
- glTF merge policy
- URI rewrite policy
- GLB packing policy
- validation policy and tool versions
- compression policy
- image/buffer transformation policy
- safe-path policy for extracted entries

When this hash changes, derived viewer artifacts should be regenerated from retained raw payloads. Onshape should not be called unless the raw payload is missing.

### `artifactSetHash`

`artifactSetHash` identifies one immutable set of public files for one logical output.

Hash:

- output kind and format
- `sourceHash`
- `configHash`
- `optionsHash`
- `requestHash`
- `rawPayloadHash`
- `postprocessHash`
- artifact-set schema version

The artifact set, not the individual primary file, is the unit of readiness and supersession.

## Onshape Defaults

Onshape does not expose effective/defaulted export parameters in translation responses. The create/poll response type `BTTranslationRequestInfo` contains result handles such as `id`, `requestState`, `resultExternalDataIds`, and `resultElementIds`, but does not echo the effective export body.

The OpenAPI schema and guide can disagree. For example, OpenAPI lists `storeInDocument` defaults on some export schemas, while the import/export guide says asynchronous exports default to `false` for external files. For cache correctness:

- Set every cache-relevant request field explicitly when we know the desired value.
- Record a `defaultsPolicyVersion` for local default filling.
- Do not assume omitted and explicit-default requests are equivalent until proven by live experiments.
- Prefer explicit numeric mesh tolerances over named resolution when stable geometry output matters.
- Keep observed API spec version and docs source in request metadata for diagnostics.

## Onshape Result Metadata

Known useful fields from translation responses:

- `id`
- `href`
- `requestState`
- `requestElementId`
- `documentId`
- `workspaceId`
- `versionId`
- `resultDocumentId`
- `resultWorkspaceId`
- `resultElementIds`
- `resultExternalDataIds`
- `failureReason`
- `name`
- `exportRuleFileName`
- `viewRef`

Use these for:

- crash recovery
- polling resume
- diagnostics
- associating raw payloads with Onshape attempts
- operator support

Do not use these as the primary deterministic cache identity.

`GET /api/documents/d/{did}/externaldata/{fid}` documents `If-None-Match`, so external data downloads may provide useful HTTP cache headers in practice. The public OpenAPI does not document stable response `ETag`, `Last-Modified`, size, or checksum fields. Capture headers if present, but compute our own byte hash.

## Raw Payload Retention

Every completed Onshape download should be retained before local processing.

Store metadata:

- `rawPayloadHash`
- `requestHash`
- `translationId`
- `externalDataId` or blob `elementId`
- result index when multiple result IDs exist
- response headers
- content type
- byte length
- original filename when available
- filename source, such as `Content-Disposition`, translation `name`, `exportRuleFileName`, blob element metadata, or fallback
- detected payload kind: `glb`, `gltf_json`, `zip`, `step`, `stl`, `3mf`, `unknown`
- ZIP inventory when payload is a ZIP

Preserve Onshape-provided filenames when available. Preserve ZIP entry names as logical paths. A storage key may still be content-addressed and sanitized, but metadata should record the original name separately.

Example raw payload metadata:

```json
{
  "role": "raw_payload",
  "requestHash": "...",
  "translationId": "...",
  "externalDataId": "...",
  "rawPayloadHash": "...",
  "originalFilename": "Part Studio 1.zip",
  "filenameSource": "content-disposition",
  "objectKey": "onshape/raw/v1/ab/abcdef.../Part_Studio_1.zip",
  "contentType": "application/zip",
  "sizeBytes": 123456,
  "headers": {}
}
```

If no filename is available, store `originalFilename: null` and use a neutral fallback object key such as:

```text
onshape/raw/v1/{hashPrefix}/{rawPayloadHash}/payload.bin
```

The fallback filename is not part of cache identity.

## Post-Processed Artifacts

Preview behavior may produce several shapes:

- Direct GLB from Onshape.
- Direct glTF JSON from Onshape.
- ZIP containing one GLB.
- ZIP containing one glTF asset set.
- Future ZIP containing multiple glTF asset sets that are merged locally.
- Future locally packed GLB from glTF assets.

The viewer artifact should be described by role and content, not by assuming it is always `preview.glb`.

Common file roles:

- `viewer_entry`
- `download`
- `buffer`
- `image`
- `material`
- `auxiliary`
- `raw_payload_reference`
- `validation_report`

Example extracted ZIP file metadata:

```json
{
  "role": "viewer_entry",
  "zipEntryName": "scene/model.gltf",
  "logicalPath": "scene/model.gltf",
  "objectKey": "previews/v2/.../scene/model.gltf",
  "contentType": "model/gltf+json",
  "sha256": "...",
  "sizeBytes": 1234
}
```

The same raw ZIP can yield multiple artifact sets over time as local processing improves. For example, one artifact set may expose extracted glTF sidecars, while a later one exposes a packed GLB. These should share `rawPayloadHash` and differ by `postprocessHash` and `artifactSetHash`.

## Artifact Sets

An artifact set is ready only when every required file is uploaded and verified.

Suggested artifact-set shape:

```json
{
  "artifactSetHash": "...",
  "status": "ready",
  "outputKind": "preview",
  "format": "gltf_asset_set",
  "sourceHash": "...",
  "configHash": "...",
  "optionsHash": "...",
  "requestHash": "...",
  "rawPayloadHash": "...",
  "postprocessHash": "...",
  "primary": {
    "role": "viewer_entry",
    "logicalPath": "scene/model.gltf",
    "objectKey": "previews/v2/.../scene/model.gltf",
    "contentType": "model/gltf+json",
    "sha256": "...",
    "sizeBytes": 1234
  },
  "files": [],
  "createdAt": "...",
  "supersededAt": null,
  "supersededBy": null,
  "supersessionReason": null
}
```

For single-file STEP/STL/3MF outputs, the artifact set still has one primary file. Keeping the same shape avoids special cases and allows future sidecars such as validation reports.

## Manifest Model

The manifest is application state for a source/configuration group. It should point at artifact sets, not individual files.

Suggested manifest shape:

```json
{
  "manifestSchemaVersion": 2,
  "groupId": "group-v2:...",
  "sourceHash": "...",
  "configHash": "...",
  "configuration": {
    "values": {}
  },
  "outputs": {
    "preview": {
      "status": "ready",
      "activeArtifactSetHash": "...",
      "artifactSets": [
        {
          "artifactSetHash": "...",
          "status": "ready",
          "format": "gltf_asset_set",
          "primaryUrl": "https://...",
          "requestHash": "...",
          "rawPayloadHash": "...",
          "postprocessHash": "..."
        }
      ]
    },
    "step": {
      "status": "missing"
    }
  }
}
```

Manifests should materialize missing outputs, active outputs, and superseded outputs. SQLite or a future server database remains the coordination source of truth; object-store manifests are public/cacheable application state.

## Supersession

Supersession operates at artifact-set level.

Supersede a ready artifact set when:

- logical options change
- exact request construction changes
- raw payload changes for the active request
- local post-processing changes
- validation policy changes
- a primary or required sidecar object is missing or corrupt
- an operator invalidates or prunes the output

Record:

- `supersededAt`
- `supersededBy`
- `supersessionReason`
- optional operator/job identifier

Normal invalidation writes new artifact set objects and updates records. It should not overwrite public files in place.

## Reconciliation

Reconciliation should be able to recover from partial work.

Required checks:

- job exists but no translation ID
- translation ID exists but no final response
- final response exists but raw payload missing
- raw payload exists but post-processed artifact set missing
- artifact set exists but primary file missing
- artifact set exists but sidecar file missing
- manifest does not match active database records

Recovery rules:

- If raw payload exists, regenerate local artifacts from raw payload.
- If only translation final response exists, try downloading result external data again.
- If translation is active, resume polling by `translationId`.
- If request exists but no recoverable Onshape result exists, start a new translation for the same `requestHash`.
- If an artifact set is incomplete and cannot be repaired, mark it superseded or failed and leave public objects untouched.

## Schema Suggestions

Initial tables can be added alongside the current `jobs` and `artifacts` tables.

```sql
CREATE TABLE export_requests (
    request_hash TEXT PRIMARY KEY,
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    options_hash TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    request_json TEXT NOT NULL,
    defaults_policy_version TEXT NOT NULL,
    request_builder_version TEXT NOT NULL,
    api_spec_version TEXT,
    created_at TEXT NOT NULL
);
```

```sql
CREATE TABLE translations (
    translation_id TEXT PRIMARY KEY,
    request_hash TEXT NOT NULL,
    state TEXT NOT NULL,
    start_response_json TEXT,
    final_response_json TEXT,
    response_hash TEXT,
    result_external_data_ids_json TEXT,
    result_element_ids_json TEXT,
    failure_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

```sql
CREATE TABLE raw_payloads (
    raw_payload_hash TEXT PRIMARY KEY,
    object_key TEXT NOT NULL,
    content_type TEXT,
    byte_len INTEGER NOT NULL,
    original_filename TEXT,
    filename_source TEXT,
    detected_kind TEXT NOT NULL,
    zip_manifest_json TEXT,
    created_at TEXT NOT NULL
);
```

```sql
CREATE TABLE raw_payload_sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    request_hash TEXT NOT NULL,
    translation_id TEXT,
    external_data_id TEXT,
    result_index INTEGER,
    response_headers_json TEXT,
    etag TEXT,
    raw_payload_hash TEXT NOT NULL
);
```

```sql
CREATE TABLE artifact_sets (
    artifact_set_hash TEXT PRIMARY KEY,
    output_kind TEXT NOT NULL,
    format TEXT NOT NULL,
    source_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    options_hash TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    raw_payload_hash TEXT,
    postprocess_hash TEXT,
    status TEXT NOT NULL,
    primary_file_id INTEGER,
    created_at TEXT NOT NULL,
    superseded_at TEXT,
    superseded_by TEXT,
    supersession_reason TEXT
);
```

```sql
CREATE TABLE artifact_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    artifact_set_hash TEXT NOT NULL,
    role TEXT NOT NULL,
    logical_path TEXT NOT NULL,
    original_path TEXT,
    object_key TEXT NOT NULL,
    content_type TEXT NOT NULL,
    byte_len INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    public INTEGER NOT NULL
);
```

## Object Key Suggestions

Keep raw payloads private or operational unless explicitly intended for public diagnostics.

```text
onshape/source/v1/{sourceHash}/configuration.raw.json
onshape/source/v1/{sourceHash}/parameters.normalized/{parameterSchemaHash}.json
config-encodings/v1/{sourceHash}/{configHash}.json

onshape/requests/v1/{requestHash}/request.json
onshape/translations/v1/{translationId}/start.json
onshape/translations/v1/{translationId}/final.json

onshape/raw/v1/{rawPayloadHashPrefix}/{rawPayloadHash}/{storageSafeOriginalFilename}
onshape/raw/v1/{rawPayloadHashPrefix}/{rawPayloadHash}/payload.bin
onshape/raw-index/v1/{requestHash}/{translationId}/{resultIndex}.json

previews/v2/{sourceHash}/{configHash}/{requestHash}/{postprocessHash}/{artifactSetHash}/{logicalPath}
artifacts/v2/{sourceHash}/{configHash}/{format}/{requestHash}/{artifactSetHash}/{filename}
manifests/v2/{groupId}.json
```

`storageSafeOriginalFilename` is a sanitized storage path segment. The original filename remains in metadata. If no original filename exists, use `payload.bin` or another neutral fallback that is not part of cache identity.

## Live Experiments

Run these against a real multi-part Part Studio and a real Assembly before locking v2 cache semantics.

1. Fetch `/configuration` and record `currentConfiguration`, defaults, `sourceMicroversion`, `serializationVersion`, and `libraryVersion`.
2. Encode empty config, explicit default config, and non-default config. Decode each and compare explicit/default flags when available.
3. Export glTF with omitted defaults and explicit defaults. Compare translation responses, external data headers, raw byte hashes, and derived artifacts.
4. Repeat identical glTF requests several times. Compare `translationId`, `externalDataId`, raw headers, ZIP entry order, ZIP timestamps, raw byte hash, and processed artifact hash.
5. Test Part Studio and Assembly glTF with `grouping=true` and `grouping=false`. Record direct GLB, direct glTF, ZIP with GLB, ZIP with one glTF, and ZIP with multiple glTF behavior.
6. Test `resolution=MEDIUM` versus explicit mesh tolerances and unit.
7. Test hidden parts, `partIds`, `partsExportFilter`, and assembly `occurrencesToExport`.
8. Test STEP with omitted versus explicit `stepVersionString=AP242`.
9. Test generic STL and 3MF translations with explicit resolution, tolerances, and unit.
10. Capture external data response headers and retry with `If-None-Match` if an `ETag` is returned.
11. Test `storeInDocument=true` and inspect `resultElementIds`, document element metadata, `foreignDataId`, and `microversionId`.
12. List `/translations/d/{did}` after exports to see whether completed translations can support crash recovery.
13. Reprocess one retained raw ZIP into extracted glTF, merged glTF, and packed GLB. Verify that only `postprocessHash` and artifact set keys change.

## Risks And Unknowns

- Onshape may change translator behavior without exposing a translator version.
- Server defaults may change and are not echoed in translation responses.
- ZIP payload bytes may be nondeterministic even when extracted geometry is equivalent.
- External data retention and response headers need real validation.
- Filename sources need live confirmation for external data downloads.
- Multiple `resultExternalDataIds` need first-class handling.
- Current public preview keys can be overwritten for the same logical identity because they do not include raw/content/artifact-set hashes.
- Current manifests cannot verify or repair sidecar sets.

## Next Implementation Slice

1. Add canonical request builders and compute `requestHash` before starting translations.
2. Persist translation start/final responses and result IDs for crash recovery.
3. Store every raw Onshape payload content-addressed with headers and original filename metadata.
4. Introduce artifact sets and artifact files for preview sidecars and raw-payload references.
5. Add `postprocessHash` and reprocess-from-raw flow before adding glTF merge or GLB packing.
