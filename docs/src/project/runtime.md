# Runtime Options

## Recommendation

Use a single-provider Fly-oriented MVP:

- Rust `axum` service on Fly.io for public pages, cache checks, queue submission, status routes, and Onshape orchestration.
- Tigris Object Storage via Fly for completed artifacts, manifests, and cached Onshape metadata.
- SQLite on a Fly volume for queue coordination, job uniqueness, artifact index state, and failure summaries.

This keeps the MVP on Fly/Tigris, avoids the fixed cost of Fly Managed Postgres, and still provides transactional coordination so duplicate Onshape parameter fetches and exports are prevented.

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

Best use:

- MVP and personal-project deployment.
- Deterministic queue coordination.
- Low-cost durable job state.
- Public artifact delivery through stable Tigris URLs.

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

- Whether the web server and worker loop run in one process or separate Fly process groups on the same machine.
- How many concurrent Onshape jobs are allowed initially.
- What Tigris public hostname or URL shape is used for stable artifact URLs.
- What backup/snapshot policy is enough for the SQLite volume.
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

Create the volume before first deploy:

```sh
fly volumes create onshape_export_data --size 1 --region ord
```

Set Onshape and Tigris credentials plus `TIGRIS_PUBLIC_BASE_URL` as Fly secrets or environment variables. Change `primary_region` and the volume region together if `ord` is not the intended deployment region.

Run a deploy-time readiness check after setting secrets or changing runtime configuration:

```sh
fly ssh console -C "/app/onshape-export ops check"
```

The check validates catalog loading, SQLite connectivity, storage client construction, Tigris public URL configuration, and required Onshape/Tigris credential presence without issuing Onshape or object-store API calls.
