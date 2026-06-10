# Admin Operations

## Goals

Admin tools should make cache state visible and manageable without exposing a public API. The MVP does not include a web admin UI; these operations are CLI-only or run through Fly operational access.

Initial admin operations:

- Validate catalog entries against Onshape.
- Fetch and refresh parameter metadata.
- Generate missing GLB previews.
- Generate missing STEP, STL, and 3MF exports.
- Inspect recent job status and failures.
- Create a consistent SQLite backup snapshot for Fly volume recovery.
- Retry all failed jobs, one failed job by work key, or failed jobs by kind.
- Invalidate artifacts after exporter option changes by superseding old public artifacts in normal operation.
- Prune artifacts older than an explicit age threshold, with dry-run support.
- List cached outputs for a model.
- Inspect and optionally rewrite the manifest for a cached model configuration.

Implemented CLI commands:

```text
onshape-export catalog validate
onshape-export ops check
onshape-export ops backup <destination.db>
onshape-export parameters refresh <slug|--all>
onshape-export previews generate <slug|--all> [default|preset-slug|--all-parameter-sets]
onshape-export exports generate <slug|--all> <step|stl|3mf|--all> [default|preset-slug|--all-parameter-sets]
onshape-export jobs list [--json]
onshape-export failures list [--json]
onshape-export failures retry [--all|<work-key>|--kind <job-kind>]
onshape-export artifacts list <slug|--all>
onshape-export artifacts manifest <slug> <config-hash> [--rewrite]
onshape-export artifacts invalidate <artifact-key>
onshape-export artifacts prune <slug|--all> --older-than-days <days> [--dry-run]
```

`default` uses Onshape parameter defaults. A preset slug targets a model's catalog-defined `parameterPresets` entry. `--all-parameter-sets` generates the default set plus every configured preset.

`ops backup` writes a consistent SQLite snapshot to a new local database file using SQLite's native online backup path. On Fly, run it through `fly ssh console` to a path on the mounted volume or a temporary path that can be copied out separately. The command refuses to overwrite an existing destination.

`failures retry` without arguments preserves the broad all-failures behavior.
Use a listed work key or `--kind <job-kind>` when only one failed operation class
should be retried.

`artifacts invalidate` and `artifacts prune` supersede SQLite artifact records, preserve immutable public object-store artifacts, mark the producing ready job superseded when known, and rewrite the affected manifest from remaining ready records. Keep deletion for a future explicit operator cleanup command covering legal/IP concerns or storage-cost management.

`artifacts prune` uses SQLite artifact records as the source of truth and supersedes each matching ready record. Use `--dry-run` first to inspect matches without changing artifact state.

`artifacts manifest` renders the manifest that would be materialized from
SQLite artifact records for one model configuration. Use `--rewrite` to upload
that manifest to object storage after inspecting or repairing cache state.

## Target Command Gaps

The updated main plan expects several behaviors that are not implemented yet:

- `jobs show <job-id>` and `jobs retry <job-id>` style job commands; current retry commands live under `failures retry` and use work keys or job kinds.
- `cache reconcile` to repair SQLite/Tigris drift such as uploaded objects whose jobs were not marked ready.
- Retryability classes and public-safe error codes/messages.
- Replacement pointers and supersession reasons for artifacts and manifests.
- Catalog validation against live Onshape metadata for a no-cache `catalog validate` mode.
- Content disposition in SQLite artifact metadata.

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
