# Admin Operations

## Goals

Admin tools should make cache state visible and manageable without exposing a public API. The MVP does not include a web admin UI; these operations are CLI-only or run through Fly operational access.

Initial admin operations:

- Validate catalog entries against Onshape.
- Import, list, show, and validate live SQL catalog entries.
- Fetch and refresh parameter metadata.
- Generate missing GLB previews.
- Generate missing STEP, STL, and 3MF exports.
- Inspect recent job status and failures.
- Create a consistent SQLite backup snapshot for Fly volume recovery.
- Run deploy maintenance with private backup upload and explicit reset modes.
- Retry all failed jobs, one failed job by work key, or failed jobs by kind.
- Invalidate artifacts after exporter option changes by superseding old public artifacts in normal operation.
- Prune artifacts older than an explicit age threshold, with dry-run support.
- List cached outputs for a model.

Implemented CLI commands:

```text
onshape-export catalog validate
onshape-export catalog import <models.json>
onshape-export catalog list [--json]
onshape-export catalog show <slug>
onshape-export ops check
onshape-export ops backup <destination.db>
onshape-export ops deploy-maintenance [--reset-generated-state] [--reset-catalog-from-seed] [--fresh-database] [--confirm WIPE]
onshape-export parameters refresh <slug|--all>
onshape-export previews generate <slug|--all> [default|preset-slug|--all-parameter-sets]
onshape-export exports generate <slug|--all> <step|stl|3mf|--all> [default|preset-slug|--all-parameter-sets]
onshape-export jobs list [--json]
onshape-export failures list [--json]
onshape-export failures retry [--all|<work-key>|--kind <job-kind>]
onshape-export artifacts list <slug|--all>
onshape-export artifacts invalidate <artifact-key>
onshape-export artifacts prune <slug|--all> --older-than-days <days> [--dry-run]
```

`default` uses Onshape parameter defaults. A preset slug targets a model's catalog-defined `parameterPresets` entry. `--all-parameter-sets` generates the default set plus every configured preset.

`catalog import` replaces the live SQLite catalog from a JSON catalog index such as `catalog/v1/models.json`. Runtime `serve`, `worker`, and `catalog validate` read from SQLite, not from `CATALOG_PATH` or Tigris.

`ops backup` writes a consistent SQLite snapshot to a new local database file using SQLite's native online backup path. On Fly, run it through `fly ssh console` to a path on the mounted volume or a temporary path that can be copied out separately. The command refuses to overwrite an existing destination.

`ops deploy-maintenance` is intended for the manual GitHub deploy workflow while the Fly machine is quiesced. It always attempts to upload a SQLite backup to the private backup bucket configured by `BACKUP_TIGRIS_BUCKET`, `BACKUP_AWS_ACCESS_KEY_ID`, and `BACKUP_AWS_SECRET_ACCESS_KEY`. It can also apply explicit reset modes. Destructive reset modes require `--confirm WIPE`.

SQLite state domains:

- Schema: migration bookkeeping and table definitions, recreated by migrations.
- Live catalog: `catalog_models`, `catalog_parameter_overrides`, `catalog_parameter_presets`, and `catalog_parameter_preset_values`.
- Generated source and parameter cache: `source_resolutions`, `source_resolution_aliases`, and `parameter_metadata`.
- Generated export cache: `configuration_selections`, `configuration_encodings`, `export_requests`, `translations`, `raw_payloads`, `raw_payload_sources`, and `postprocess_runs`.
- Artifact index and queue state: `artifact_sets`, `artifact_files`, legacy `artifacts`, and `jobs`.

`--reset-generated-state` clears generated source/parameter cache, generated export cache, artifact index, queue state, and generated Tigris object prefixes while preserving live catalog rows. `--reset-catalog-from-seed` replaces live catalog rows from `catalog/v1/models.json`. `--fresh-database` removes the SQLite database file and sidecars, recreates schema through migrations, and imports `catalog/v1/models.json` before the normal app restarts.

`failures retry` without arguments preserves the broad all-failures behavior.
Use a listed work key or `--kind <job-kind>` when only one failed operation class
should be retried.

`artifacts invalidate` and `artifacts prune` supersede SQLite artifact records, preserve immutable public object-store artifacts, and mark the producing ready job superseded when known. Keep deletion for a future explicit operator cleanup command covering legal/IP concerns or storage-cost management.

`artifacts prune` uses SQLite artifact records as the source of truth and supersedes each matching ready record. Use `--dry-run` first to inspect matches without changing artifact state.

## Target Command Gaps

The updated main plan expects several behaviors that are not implemented yet:

- `jobs show <job-id>` and `jobs retry <job-id>` style job commands; current retry commands live under `failures retry` and use work keys or job kinds.
- `cache reconcile` to repair SQLite/Tigris drift such as uploaded objects whose jobs were not marked ready.
- v2 cache commands should be human-first by model/configuration/output, with advanced hash-based selectors such as `requestHash`, `rawPayloadHash`, and `artifactSetHash` for debugging and repair.
- Retryability classes and public-safe error codes/messages.
- Replacement pointers and supersession reasons for artifacts.
- Catalog validation against live Onshape metadata for a no-cache `catalog validate` mode.
- Content disposition in SQLite artifact metadata.

Future v2 repair behavior should distinguish repair from semantic supersession. If raw payload bytes are missing or corrupt, try to re-download from stored translation result IDs before starting a new Onshape translation; this repair path is available only after translation result IDs and polling state are persisted. If public artifact files are missing or corrupt and the raw payload exists, repair by re-running the same post-processing recipe and restoring the intended artifact-set object keys. Intended bytes are bytes that match recorded metadata and hashes for the same raw payload, or a verifiable derivation from the same persisted translation result. Overwriting object keys should be allowed only for verified repair of intended bytes, not for normal cache invalidation or post-processing semantic changes that should create a superseding artifact set.

Follow-up ticket: reconsider v2 repair and overwrite semantics before production use, including translation result ID persistence, intended-byte verification, raw payload repair, public artifact repair, DB/object drift, corrupt-but-public objects, missing sidecars, partial uploads, concurrent repair races, CDN behavior, and cases that should supersede instead of overwrite.

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

With SQLite coordination state and Tigris artifacts, future admin views should mostly operate by known keys from the catalog, job records, and artifact records.

Weaknesses:

- SQLite-on-volume is single-machine oriented.
- Long-term audit history is intentionally limited.
- Rich filtering and multi-admin workflows are out of scope.
- Concurrent admin edits are awkward.

If these limitations become painful, move coordination/admin state to Postgres before adding public API features.
