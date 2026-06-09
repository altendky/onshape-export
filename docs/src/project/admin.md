# Admin Operations

## Goals

Admin tools should make cache state visible and manageable without exposing a public API. The MVP does not include a web admin UI; these operations are CLI-only or run through Fly operational access.

Initial admin operations:

- Validate catalog entries against Onshape.
- Fetch and refresh parameter metadata.
- Generate missing GLB previews.
- Generate missing STEP, STL, and 3MF exports.
- Inspect job status and failures.
- Retry all failed jobs, one failed job by work key, or failed jobs by kind.
- Invalidate artifacts after exporter option changes, deleting the object-store
  object before removing the SQLite artifact record.
- Prune artifacts older than an explicit age threshold, with dry-run support.
- List cached outputs for a model.
- Inspect and optionally rewrite the manifest for a cached model configuration.

Implemented CLI commands:

```text
onshape-export catalog validate
onshape-export parameters refresh <slug|--all>
onshape-export previews generate <slug|--all> [default|preset-slug|--all-parameter-sets]
onshape-export exports generate <slug|--all> <step|stl|3mf|--all> [default|preset-slug|--all-parameter-sets]
onshape-export failures list
onshape-export failures retry [--all|<work-key>|--kind <job-kind>]
onshape-export artifacts list <slug|--all>
onshape-export artifacts manifest <slug> <config-hash> [--rewrite]
onshape-export artifacts invalidate <artifact-key>
onshape-export artifacts prune <slug|--all> --older-than-days <days> [--dry-run]
```

`default` uses Onshape parameter defaults. A preset slug targets a model's catalog-defined `parameterPresets` entry. `--all-parameter-sets` generates the default set plus every configured preset.

`failures retry` without arguments preserves the broad all-failures behavior.
Use a listed work key or `--kind <job-kind>` when only one failed operation class
should be retried.

`artifacts prune` uses SQLite artifact records as the source of truth, deletes
each matching object-store object, removes the artifact record, and rewrites the
affected manifest. Use `--dry-run` first to inspect matches without deleting
anything.

`artifacts manifest` renders the manifest that would be materialized from
SQLite artifact records for one model configuration. Use `--rewrite` to upload
that manifest to object storage after inspecting or repairing cache state.

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
