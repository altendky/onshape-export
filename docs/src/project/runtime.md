# Runtime Options

## Recommendation

Use a single-provider Fly-oriented MVP:

- Rust `axum` service on Fly.io for public pages, cache checks, queue submission, status routes, and Onshape orchestration.
- A bounded embedded worker loop in the same Rust process for MVP export and metadata jobs.
- Tigris Object Storage via Fly for completed artifacts, manifests, and cached Onshape metadata.
- SQLite on a Fly volume for queue coordination, job uniqueness, artifact index state, and failure summaries.

This keeps the MVP on Fly/Tigris, avoids the fixed cost of Fly Managed Postgres, and still provides transactional coordination so duplicate Onshape parameter fetches and exports are prevented.

The MVP assumes one Fly machine running one Rust service process. The public web server and worker loop share the same local SQLite database on the attached Fly volume. A separate worker process group is deferred until shared coordination is introduced or Fly volume sharing semantics are explicitly verified.

## Critical Runtime Constraints

- Keep SQLite transactions short and never hold a database write transaction while calling Onshape.
- Treat this as a mandatory implementation and test requirement for code paths that both update SQLite coordination state and call Onshape.

Rationale:

- Slow Onshape calls under a SQLite write transaction would block other workers and request handlers that need queue state.
- SQLite's single-writer locking model makes long write transactions harmful to queue progress and duplicate-work prevention.
- Network timeouts during Onshape calls must not extend database write locks until the timeout completes.

Implementation tests should verify mocked slow Onshape calls do not hold SQLite write locks and that duplicate requests still deduplicate through short job-row transactions.

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
- Web and worker restarts are coupled until the worker is split out later.

Best use:

- MVP and personal-project deployment.
- Deterministic queue coordination.
- Low-cost durable job state.
- Public artifact delivery through stable Tigris URLs.

Initial worker policy:

- Run a bounded worker loop inside the Rust service process.
- Start with conservative Onshape concurrency and increase only after real API behavior is measured.
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

- How many concurrent Onshape jobs are allowed initially.
- What Tigris public hostname or URL shape is used for stable artifact URLs.
- What backup/snapshot policy is enough for the SQLite volume.
- When, if ever, SQLite should be replaced with Postgres.
