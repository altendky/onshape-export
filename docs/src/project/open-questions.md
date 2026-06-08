# Open Questions

## Runtime

- Should the web server and worker loop run in one process or separate Fly process groups on the same machine?
- What initial Onshape export concurrency limit is safe?
- What backup or snapshot policy is enough for the SQLite Fly volume?
- What Tigris public URL shape should be used for stable artifact links?
- When should SQLite be replaced with Postgres?

## Onshape Auth

- Should the service use Onshape API keys, OAuth client credentials, or another server-owned auth flow?
- Which scopes are required for versioned configuration reads and exports?
- Should the existing `onshape-mcp` auth/client crates be extracted or reused directly?

## Onshape Export Details

- Which exact `formatName` values should be used for STL, 3MF, and GLB on generic translation endpoints?
- Are format-specific endpoints better than generic translation endpoints for STEP and GLB in practice?
- Does synchronous Part Studio glTF export produce suitable GLB previews faster than async export?
- Do Assemblies and Part Studios need different default export options?
- How should multiple `resultExternalDataIds` be represented and served?

## Parameter Handling

- Which Onshape configuration parameter types appear in target models?
- Can all parameter types be encoded safely without calling `configurationencodings`?
- What numeric precision or step rules should each model expose?
- How should hidden or conditionally visible parameters be handled?

## Preview UX

- Which models should auto-generate previews after debounce versus requiring an explicit button?
- What preview tessellation defaults keep GLB files small enough?
- Should thumbnails be generated in addition to GLB previews?

## Cache And Admin

- How long should failed jobs suppress retries?
- Should admin rebuilds generate all formats or only missing artifacts?
- How should exporter version changes invalidate old artifacts?
- What CLI maintenance commands are needed before a web admin UI exists?
