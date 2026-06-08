# Frontend and Preview

## Frontend Direction

Start with server-rendered or static pages plus minimal JavaScript for parameter interaction, cache checks, status polling, and the 3D viewer.

For the viewer, start with `<model-viewer>` consuming cached GLB files. Move to three.js only if the product needs custom CAD-like interactions such as measurements, section cuts, custom annotations, or advanced selection.

## Preview Is A Separate Export

The preview is not generated from STEP, STL, or 3MF locally. It is another Onshape export for the same selected configuration:

```text
selected configuration -> Onshape GLB/glTF export -> Tigris -> browser viewer
```

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
- If the preview is cached, the app returns the cached GLB URL and the page swaps it immediately.
- If the preview is missing, show a `Generate Preview` action.
- Later, selected models can opt into debounce-based auto-generation.

Cached preview and download URLs are stable public Tigris artifact URLs. The Fly app should not proxy artifact bytes in the intended production path.

## User Flow

1. Model page loads default parameters.
2. Page calls an app cache/status route for the default GLB preview.
3. User changes parameters.
4. Page calls an app cache/status route to check whether a preview exists for the new `config_hash`.
5. If cached, the app returns the cached GLB URL and the viewer updates.
6. If missing, the user can generate the preview.
7. User selects STEP, STL, or 3MF for download.
8. Missing download artifacts are generated and cached.

## Preview Quality

Preview exports should use web-friendly settings, which may differ from final export settings.

Preview defaults:

- GLB format.
- Medium or coarse tessellation until quality requirements are known.
- Stable orientation and scale.
- Cache key includes preview options and exporter version.

Final export defaults:

- STEP: AP242 unless a model overrides it.
- STL: model-defined resolution defaults.
- 3MF: model-defined resolution defaults.

## Viewer Fallbacks

Fallback order:

1. Cached GLB preview.
2. Cached STL preview if GLB generation fails.
3. Static thumbnail if available.
4. Download-only state with clear messaging.
