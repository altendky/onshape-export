# Admin Operations

## Goals

Admin tools should make cache state visible and manageable without exposing a public API. The MVP does not include a web admin UI; these operations are CLI-only or run through Fly operational access.

Initial admin operations:

- Validate catalog entries against Onshape.
- Fetch and refresh parameter metadata.
- Generate missing GLB previews.
- Generate missing STEP, STL, and 3MF exports.
- Inspect job status and failures.
- Retry failed jobs.
- Invalidate artifacts after exporter option changes.
- List cached outputs for a model.

Implemented CLI commands:

```text
onshape-export catalog validate
onshape-export parameters refresh <slug|--all>
onshape-export previews generate <slug|--all>
onshape-export exports generate <slug|--all> <step|stl|3mf|--all>
onshape-export failures list
onshape-export failures retry
onshape-export artifacts list <slug|--all>
onshape-export artifacts invalidate <artifact-key>
```

## Web Admin Deferral

Do not add `/admin` routes in the MVP.

Reasons:

- Avoid authentication and CSRF scope before browser admin workflows exist.
- Keep maintenance access limited to local CLI credentials and Fly operational access.
- Reduce public attack surface.

If a web admin UI is added later, protect it with explicit authentication and CSRF protection. Do not rely on obscurity or unlinked routes.

## Future Candidate Routes

These are product-internal routes, not public API commitments:

```text
GET  /admin
GET  /admin/models
GET  /admin/models/{slug}
POST /admin/models/{slug}/validate
POST /admin/models/{slug}/refresh-parameters
POST /admin/models/{slug}/generate-preview
POST /admin/models/{slug}/generate-export
POST /admin/models/{slug}/rebuild-all
GET  /admin/jobs/{group_id}
POST /admin/jobs/{group_id}/retry
GET  /admin/cache/{group_id}
POST /admin/cache/{group_id}/invalidate
```

## Future Admin With SQLite

With SQLite coordination state and Tigris artifacts, future admin views should mostly operate by known keys from the catalog, job records, artifact records, and manifests.

Weaknesses:

- SQLite-on-volume is single-machine oriented.
- Long-term audit history is intentionally limited.
- Rich filtering and multi-admin workflows are out of scope.
- Concurrent admin edits are awkward.

If these limitations become painful, move coordination/admin state to Postgres before adding public API features.
