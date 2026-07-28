# Runtime Options

## Recommendation

Use a single-provider Fly-oriented MVP:

- Rust `axum` service on Fly.io for public pages, cache checks, queue submission, status routes, and Onshape orchestration.
- A bounded embedded worker loop in the same Rust process for the safest MVP deployment.
- Tigris Object Storage via Fly for completed artifacts and cached Onshape metadata.
- SQLite on a Fly volume for live catalog data, queue coordination, job uniqueness, artifact index state, and failure summaries.

This keeps the MVP on Fly/Tigris, avoids the fixed cost of Fly Managed Postgres, and still provides transactional coordination so duplicate Onshape parameter fetches and exports are prevented.

SQLite is also the runtime catalog source of truth. `catalog/v1` JSON files are retained as a seed/test fixture and can be imported with `onshape-export catalog import catalog/v1/models.json`; normal `serve` and `worker` startup do not read `CATALOG_PATH`.

The preferred MVP deployment is one Fly machine running one Rust service process. The public web server and worker loop share the same local SQLite database on the attached Fly volume.

The branch also includes `onshape-export worker` and `WORKER_ENABLED=false` for split web/worker process groups. Treat that as an operational escape hatch, not the default scaling model. Before running independent workers on multiple machines, verify shared storage semantics explicitly or move coordination to Postgres or another shared database.

## Critical Runtime Constraints

- Keep SQLite transactions short.
- Never hold a SQLite write transaction while calling Onshape, Tigris, or another network service.
- Treat this as a mandatory implementation and test requirement for paths that both update SQLite coordination state and call Onshape.
- Use SQLite only for local single-writer coordination until a concrete need justifies Postgres.

Rationale:

- Slow Onshape calls under a SQLite write transaction would block other workers and request handlers that need queue state.
- SQLite's single-writer locking model makes long write transactions harmful to queue progress and duplicate-work prevention.
- Network timeouts during Onshape calls must not extend database write locks until the timeout completes.

TODO: add tests that mocked slow Onshape calls do not hold SQLite write locks and that duplicate requests still deduplicate through short job-row transactions.

## Proposed Slicer Adapter Runtime

Slicer project 3MF adapters are proposed and not implemented. A future runtime
would discover explicitly configured adapter executables or immutable packages,
inspect their protocol and capability metadata, and select only an adapter whose
dialect, versions, provenance set, and requested capabilities are compatible.
The service must also match package/build identity, protocol version, dialect
revision, provenance-set version, and capability metadata to a service-owned
approved-adapter manifest; adapter self-reporting is not a trust decision.
Discovery must not search arbitrary writable paths or download adapters during a
job. Configuration and installation details remain open.

If an adapter is absent, incompatible, unreviewed, or missing a requested
capability, the job should fail with a stable unsupported/unavailable result. It
must not silently substitute another slicer dialect, omit settings, or fall back
to an Onshape geometry 3MF while labeling it as a slicer project.

Execution should use a fresh restricted work directory, declared read-only
inputs, no service credentials, no network access, bounded diagnostics, and
limits for elapsed time, CPU, memory, disk, file/member count, output size, and
subprocesses. Candidate output remains private until service-side archive,
schema, dialect, and compatibility validation passes and the service independently
hashes it and matches that value to the adapter's validation report. The
containment mechanism is intentionally unresolved pending a prototype; see
[Slicer Project 3MF Adapters](slicer-3mf-adapters.md).

Initial public hostname:

```text
https://onshape-export.fly.dev
```

## Option: Fly App With SQLite And Tigris

This is the preferred starting point.

Benefits:

- Normal Rust runtime with no Worker/WASM constraints.
- Straightforward async polling for Onshape translation jobs.
- Tigris provides public object delivery with no object egress charge.
- SQLite provides cheap transactional uniqueness on a Fly volume.
- One operational provider surface for app, object storage, and database volume.

Costs:

- Single-machine SQLite is not highly available.
- Fly volumes are region and machine scoped.
- Recovery and backup policy must be explicit if job history becomes important.
- Multi-machine scaling requires redesigning coordination, likely Postgres.
- Web and worker restarts are coupled in the default single-process deployment.

Best use:

- MVP and personal-project deployment.
- Deterministic queue coordination.
- Low-cost durable job state.
- Public artifact delivery through stable Tigris URLs.

Initial worker policy:

- Run a bounded worker loop inside the Rust service process by default.
- Default `WORKER_CONCURRENCY` to `1` and increase only after real API behavior is measured.
- Replace SQLite with Postgres or another shared coordination backend before adding multi-machine workers.

## Option: Fly Managed Postgres

Fly Managed Postgres would provide stronger managed durability, backups, high availability, and easier future multi-machine scaling.

Benefits:

- Native transactional queue and job history.
- Clear upgrade path for multiple workers and richer admin queries.
- Managed backups and operational support.

Costs:

- Fixed monthly cost is too high for the MVP personal-project budget.
- Adds production database operations before the product needs user/catalog relational data.

Best use:

- Future upgrade if SQLite-on-volume constraints become painful.
- Future multi-machine workers, richer admin UI, or stronger durability needs.

## Option: Cloudflare Or R2 Hybrid

Cloudflare Workers/Pages plus R2 would be viable, but it is not the MVP direction.

Benefits:

- Excellent edge delivery and R2 object storage.
- Useful if the product later needs Cloudflare-specific DNS, CDN, WAF, or Access features.

Costs:

- Splits the MVP across providers.
- Worker-only orchestration is awkward for long-running Onshape polling.
- Adds platform-specific moving parts before proving the core export workflow.

Best use:

- Future reconsideration if public traffic or edge controls justify Cloudflare.

## Initial Runtime Decision Points

- What initial Onshape export concurrency limit is safe beyond the default of `1`.
- What Tigris public hostname or URL shape should be used for stable artifact URLs.
- Whether explicit operator snapshots are enough for the SQLite volume or platform backups are required.
- When, if ever, SQLite should be replaced with Postgres.

## Worker Runtime

The default `serve` process starts the public web app and an in-process background worker. For separate Fly process groups, run the web process with `WORKER_ENABLED=false` and run a worker process with:

```sh
onshape-export worker
```

Scheduled rebuilds are opt-in. Set `REBUILD_INTERVAL_SECONDS` on a process with a worker to periodically enqueue parameter refreshes for every catalog model and enqueue missing default preview/download artifacts once cached parameter metadata exists. A missing or `0` value disables scheduled rebuilds.

Worker concurrency is explicit through `WORKER_CONCURRENCY`. The default is `1`, which keeps the MVP conservative for Onshape API and translation load. Increase it only after observing real export latency, API limits, and SQLite volume behavior.

## Fly Deployment Scaffold

The repository includes a Dockerfile and `fly.toml` for the initial Fly deployment. The default config runs one app machine with `WORKER_ENABLED=true`, so public routes and queued work share the same SQLite database on the mounted `/data` volume.

`fly.toml` owns non-secret app configuration, including the Tigris endpoint, bucket name, public artifact base URL, and AWS region. Keep only credentials in Fly secrets. Bucket-level settings such as public access and CORS are Tigris/S3 configuration and cannot be expressed in `fly.toml`.

Create the volume before first deploy:

```sh
fly volumes create onshape_export_data --size 1 --region ord
```

Create or confirm the public Tigris bucket. For a new bucket matching the default `fly.toml` values:

```sh
fly storage create --name onshape-export --public
```

For an existing bucket, confirm it is public:

```sh
fly storage status onshape-export
```

Create or confirm the private backup bucket. Do not make this bucket public and do not apply browser CORS to it:

```sh
fly storage create --name onshape-export-backup
```

Set Onshape and Tigris credentials as Fly secrets. Use separate credentials for the public artifact bucket and the private backup bucket when possible. Do not set non-secret Tigris names, endpoint URLs, public base URLs, or regions as secrets unless they differ from `fly.toml` intentionally:

```sh
fly secrets set ONSHAPE_ACCESS_KEY=... ONSHAPE_SECRET_KEY=... AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=...
fly secrets set BACKUP_AWS_ACCESS_KEY_ID=... BACKUP_AWS_SECRET_ACCESS_KEY=...
```

Change `primary_region` and the volume region together if `ord` is not the intended deployment region. If the app name, bucket name, public Tigris hostname, or local development port changes, update `fly.toml` and `scripts/tigris-cors.json` together.

Set bucket CORS before browser preview testing. The Fly app returns public Tigris URLs for cached previews, and `<model-viewer>` fetches GLB, glTF, and glTF sidecar assets cross-origin. A missing CORS policy can make a ready preview artifact load as an empty viewer in the browser.

After credentials are set, this command runs a temporary AWS CLI machine with the app environment and secrets injected. It applies `scripts/tigris-cors.json` without printing secret values:

```sh
fly console --image amazon/aws-cli:latest \
  --file-local /tmp/apply-tigris-cors.sh=scripts/apply-tigris-cors.sh \
  --file-local /tmp/tigris-cors.json=scripts/tigris-cors.json \
  -C "sh /tmp/apply-tigris-cors.sh /tmp/tigris-cors.json"
```

Verify CORS with an existing public preview artifact URL:

```sh
curl -fsSI -H "Origin: https://onshape-export.fly.dev" "https://onshape-export.t3.tigrisfiles.io/path/to/preview.glb"
```

The response should include `Access-Control-Allow-Origin: https://onshape-export.fly.dev`. Add any custom production domain and active local development origins to `scripts/tigris-cors.json` before using them.

Run a deploy-time readiness check after setting secrets or changing runtime configuration:

```sh
fly ssh console -C "/app/onshape-export ops check"
```

The check validates non-empty catalog loading, SQLite connectivity, storage client construction, Tigris public URL configuration, and required Onshape/Tigris credential presence without issuing Onshape or object-store API calls.

For a new database, seed the catalog before expecting public model pages to appear:

```sh
fly ssh console -C "/app/onshape-export catalog import catalog/v1/models.json"
```

## Deploy Workflow

Manual deploys run through `.github/workflows/deploy.yml`. Configure the `production` GitHub Environment with required reviewers and set `FLY_API_TOKEN` as an environment secret. Optionally set repository or environment variable `FLY_APP` if the Fly app name is not `onshape-export`.

The workflow follows one operational pattern for every manual deploy:

1. Build and push the target image.
2. Update the single app machine to run `sleep infinity`, which leaves the `/data` volume mounted while the public HTTP service is down. The workflow explicitly applies the non-secret `[env]` values from `fly.toml` to this quiesced machine so maintenance sees the same runtime configuration as the deployed app. This update retries briefly because Fly's Machines API can see a just-pushed image tag before the corresponding registry manifest is fully available.
3. Execute `/app/onshape-export ops deploy-maintenance` inside that quiesced machine.
4. Upload a SQLite backup to the private backup bucket.
5. Apply selected reset options.
6. Deploy the normal app command from the same image.
7. Run `ops check` and `/healthz`.

Before quiescing the machine, the workflow records the current app image. If the workflow fails before the normal app deploy succeeds, a recovery step redeploys that previous image so the app machine is not left running `sleep infinity`.

The workflow currently expects exactly one Fly app machine. If the app is later split into separate web and worker machines or process groups, update the deployment workflow before enabling multi-machine production deployment.

Destructive workflow inputs default to `false` and require `confirm_destructive` to be `WIPE`:

- `reset_generated_state`: deletes generated Tigris object prefixes and generated SQLite cache/job state while preserving the live catalog.
- `reset_catalog_from_seed`: replaces live SQLite catalog rows from `catalog/v1/models.json`.
- `fresh_database`: deletes `/data/onshape-export.db`, `/data/onshape-export.db-wal`, and `/data/onshape-export.db-shm`, recreates the schema, and imports `catalog/v1/models.json` during maintenance.

Generated Tigris prefixes are `onshape/source/v2/`, `onshape/raw/v2/`, `previews/v2/`, and `artifacts/v2/`. The backup bucket is not touched by generated-state resets.

## SQLite Backups

The MVP backup policy is an explicit SQLite snapshot before deployments or cache maintenance that could affect job/artifact state. The deploy workflow uploads this backup to the private backup bucket before any selected reset. Operators can still create a local volume snapshot manually:

```sh
fly ssh console -C "/app/onshape-export ops backup /data/backups/onshape-export-$(date +%Y%m%d%H%M%S).db"
```

The command uses SQLite `VACUUM INTO` through the live database connection, so the result is a consistent standalone database file. Create the destination directory first and copy completed backups off the volume according to operational needs. If backups need to become automatic or point-in-time, move this concern to platform snapshots or Postgres.
