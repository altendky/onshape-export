# Frontend and Preview

## Frontend Direction

Start with server-rendered or static pages plus minimal JavaScript for parameter interaction, cache checks, status polling, and the 3D viewer.

For the viewer, start with `<model-viewer>` consuming cached preview artifact sets: preferably GLB files, or Onshape-provided glTF asset sets when GLB is not returned. Move to three.js only if the product needs custom CAD-like interactions such as measurements, section cuts, custom annotations, or advanced selection.

## Preview Is A Separate Export

The preview is not generated from STEP, STL, or 3MF locally. It is another Onshape export for the same selected configuration:

```text
selected configuration -> Onshape GLB export -> Tigris -> browser viewer
```

The preferred MVP preview artifact is GLB. In practice, Onshape can return direct glTF JSON or zipped glTF asset sets from the same preview endpoint, so the branch supports direct glTF and ZIPs that contain exactly one `.gltf` viewer asset by publishing that entry and its sidecars under the same immutable preview identity. Target cache language should call this a preview artifact set.

Current branch status: preview handling writes direct or zipped GLB as `preview.glb`, and direct glTF JSON as `preview.gltf`. If a ZIP has no GLB but contains exactly one glTF file, it writes that `.gltf`, uploads all safe sidecar asset paths, and retains the original ZIP privately as a raw payload for debugging/reprocessing. ZIPs with multiple `.gltf` files are rejected until the app can merge them.

The final download is independently cached:

```text
selected configuration -> Onshape STEP/STL/3MF export -> Tigris -> download
```

## Interaction Model

Avoid generating previews on every tiny input change.

Initial policy:

- Dropdown and checkbox changes can check cache immediately.
- Numeric inputs should check on blur or Enter.
- Slider inputs should check when the drag ends.
- If the preview is cached, the app returns the active preview artifact URL and the page swaps it immediately.
- If the preview is missing, show a `Generate Preview` action.
- Later, selected models can opt into debounce-based auto-generation.

Cached preview and download URLs are stable public Tigris artifact URLs. The Fly app should not proxy artifact bytes in the intended production path.

## User Flow

1. Model page loads default parameters.
2. Page calls an app cache/status route for the default preview artifact set.
3. User changes parameters.
4. Page calls an app cache/status route, which rebuilds the current canonical preview request for the selected `config_hash` and checks readiness for that exact `requestHash`.
5. If cached for that exact request, the app returns the active preview artifact URL and the viewer updates.
6. If missing, the user can generate the preview.
7. User selects STEP, STL, or 3MF for download.
8. Missing download artifacts are generated and cached.

## Preview Quality

Preview exports should use web-friendly settings, which may differ from final export settings.

Preview defaults:

- GLB format.
- Medium or coarse tessellation until quality requirements are known.
- Stable orientation and scale.
- Cache identity should keep the layers from [Forward-Looking Cache Model](cache-model.md) separate: preview quality, orientation, scale, and grouping affect `optionsHash`; exact request shape, explicit defaults, defaults-policy version, and request-builder version affect `requestHash`; local extraction, validation, packing, and tool versions affect `postprocessHash`; the published preview artifact set combines those identities into `artifactSetHash`.

Final export defaults:

- STEP: AP242 unless a model overrides it.
- STL: model-defined resolution defaults.
- 3MF: model-defined resolution defaults.

## Viewer Fallbacks

Fallback order:

1. Cached preview artifact set, preferably GLB.
2. Static thumbnail if available.
3. Download-only state with clear messaging.
