# Onshape API Flow

## Source Model Identity

Initial model sources are immutable document versions:

```text
document id: did
version id: vid
```

Workspaces are intentionally out of scope for the first version.

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

If hand-building this string is insufficient for some parameter types, use Onshape's encoding endpoint:

```text
POST /api/elements/d/{did}/e/{eid}/configurationencodings?versionId={vid}
```

Use `linkDocumentId` as well if the versioned element must be accessed through a linked document context.

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

Encoded configuration results should be cached by source identity and `config_hash`.

## Preview Export

Preview is a GLB/glTF export for the selected configuration. It is separate from the user's final download format.

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

Async GLB body shape should include the configuration under `advancedParams.configuration`:

```json
{
  "advancedParams": {
    "configuration": "..."
  },
  "meshParams": {
    "resolution": "MEDIUM"
  },
  "storeInDocument": false,
  "notifyUser": false,
  "triggerAutoDownload": false
}
```

Preview cache identity must include all preview-affecting options, including mesh settings, orientation flags such as `isYAxisUp`, grouping behavior, and exporter version.

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
| GLB/glTF | Preview | `.glb` or `.gltf` |
| STEP | Download | `.step` |
| STL | Download and fallback preview | `.stl` |
| 3MF | Download | `.3mf` |

Prefer GLB for browser preview even when the user downloads STL or 3MF.
