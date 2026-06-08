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

## MVP CLI Commands

Start with CLI or Fly-run commands. These commands may enqueue jobs rather than doing all work synchronously.

```text
onshape-export validate-catalog [--model <slug>]
onshape-export refresh-parameters <slug> [--wait]
onshape-export generate-preview <slug> [--config <json>] [--missing-only] [--wait]
onshape-export generate-export <slug> --format step|stl|3mf [--config <json>] [--missing-only] [--wait]
onshape-export jobs list [--status queued|running|ready|failed]
onshape-export jobs show <job-id>
onshape-export jobs retry <job-id>
onshape-export cache list <slug>
onshape-export cache invalidate <artifact-id-or-group-id> --reason <text>
onshape-export cache reconcile [--model <slug>]
```

Command behavior:

- `validate-catalog` checks schema, slug rules, duplicate source identities, override parameter IDs, and public-export suitability flags.
- `refresh-parameters` creates or finds a `parameter_refresh` job.
- `generate-preview` and `generate-export` create or find deterministic work keys.
- `jobs retry` respects retryability and max-attempt policy unless a future `--force` option is added.
- `cache invalidate` marks artifacts superseded and should not delete public objects during normal operation.
- `cache reconcile` repairs SQLite/Tigris drift such as uploaded objects whose jobs were not marked ready.

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
