# Onshape Export

A planned Rust-based website for exporting curated, highly parameterized Onshape models.

The service is aimed at models where publishing every parameter combination ahead of time is impractical. Users select parameters in the browser, preview the resulting model, and download generated exports. The service owns the Onshape integration and cache, so users do not need Onshape accounts.

## Current Status

This repository contains planning documentation and an initial Rust service. The implementation has moved past the original docs-only plan; the remaining gaps are mostly hardening and live Onshape verification details.

Implemented foundation:

- Single-crate Rust `axum` app.
- `GET /healthz`, `GET /`, model pages, generation routes, and status polling routes.
- Environment-based runtime configuration.
- SQLite connection setup with migrations and MVP durability PRAGMAs.
- Tigris/S3-compatible client construction.
- Signed Onshape API-key client for configuration metadata reads and export calls.
- SQLite-backed live catalog loading, validation, and CLI import/list/show operations.
- Onshape parameter metadata refresh, normalization, Tigris caching, and SQLite deduplication.
- RFC 8785 JSON canonicalization for source, configuration, options, and work-key hash preimages.
- Server-rendered model parameter controls and submitted-value validation.
- Background worker loop for queued parameter refreshes, previews, and downloads.
- Persisted retry attempt limits, `nextRetryAt`, and exponential full-jitter backoff for failed worker jobs.
- Preview artifacts prefer GLB, but direct glTF JSON is accepted; a ZIP with exactly one Onshape glTF viewer asset is extracted into a published viewer entry plus sidecars, while the original ZIP is retained privately as a raw payload.
- Supersession-based artifact invalidation and pruning that leave public object-store artifacts immutable.
- Worker-only runtime mode for separate Fly process groups.
- Configurable worker concurrency through `WORKER_CONCURRENCY`.
- CLI maintenance commands for catalog import/validation/list/show, deploy maintenance, parameter refresh, pre-generation, job/failure inspection and retry, and artifact inspection/invalidation.
- Catalog-defined parameter presets for targeted preview/export pre-generation.
- Catalog-defined parameter UI overrides and preview/STEP export option defaults.
- Deploy-time `ops check` command for SQL-backed catalog, SQLite, storage, public URL, and credential readiness.
- Operator-triggered SQLite backup snapshots through `ops backup <destination.db>` and deploy-time private backup-bucket uploads.
- GitHub Actions CI with Rust, docs, pre-commit, mise lockfile checks, and an aggregate job named exactly `all`.
- Scheduled Renovate workflow and repository tooling configuration.

Known plan gaps:

- Onshape translation IDs, polling state, and `Retry-After` values are not persisted for crash-resume yet.
- Failure records and public status errors still need stable public-safe error codes and user messages.
- Uploaded-object verification and cache reconciliation for partial writes are not implemented yet.
- Public manifests are not part of the initial v2 flow; DB-backed status and artifact state are the source of truth.

Local verification:

```sh
mise install --locked
mise exec -- pre-commit run --show-diff-on-failure --color=always
mise exec -- pre-commit run --from-ref origin/main --to-ref HEAD --show-diff-on-failure --color=always
mise exec -- pre-commit run --show-diff-on-failure --color=always --all-files
mise exec -- pre-commit run --hook-stage manual --all-files
```

Local run:

```sh
cargo run
```

Local run with MinIO S3-compatible storage:

```sh
mise run local-s3
mise run local-run
```

If you are not using mise, run the same scripts directly:

```sh
scripts/local-s3.sh
scripts/run-local.sh
```

`scripts/run-local.sh` loads optional `.env.local` first. Put Onshape credentials there for local export testing:

```sh
ONSHAPE_ACCESS_KEY=...
ONSHAPE_SECRET_KEY=...
```

MinIO runs at `http://localhost:9000`; its console is at `http://localhost:9001` with `minioadmin` / `minioadmin` by default.

Worker-only run:

```sh
cargo run -- worker
```

Set `WORKER_ENABLED=false` when running a web process that should not also claim queued work.
Set `WORKER_CONCURRENCY` to control how many queued jobs a worker process may run at once; the default is `1`.

The default local database is `onshape-export.db`. Set `DATABASE_URL` for deployment, for example to a SQLite file on a Fly volume.

The runtime catalog is stored in SQLite. To seed a new local or deployed database from the checked-in fixture, run:

```sh
cargo run -- catalog import catalog/v1/models.json
```

Fly deployment foundation:

```sh
fly volumes create onshape_export_data --size 1 --region ord
fly storage create --name onshape-export --public
fly storage create --name onshape-export-backup
fly secrets set ONSHAPE_ACCESS_KEY=... ONSHAPE_SECRET_KEY=... AWS_ACCESS_KEY_ID=... AWS_SECRET_ACCESS_KEY=...
fly secrets set BACKUP_AWS_ACCESS_KEY_ID=... BACKUP_AWS_SECRET_ACCESS_KEY=...
fly deploy
fly console --image amazon/aws-cli:latest \
  --file-local /tmp/apply-tigris-cors.sh=scripts/apply-tigris-cors.sh \
  --file-local /tmp/tigris-cors.json=scripts/tigris-cors.json \
  -C "sh /tmp/apply-tigris-cors.sh /tmp/tigris-cors.json"
fly ssh console -C "/app/onshape-export ops check"
```

The included `fly.toml` runs a single web machine with the in-process worker enabled so SQLite coordination stays on one mounted Fly volume at `/data`. Non-secret Tigris settings live in `fly.toml`; keep only Onshape and Tigris/S3 credentials as Fly secrets. Set Tigris bucket CORS before browser preview testing because previews are loaded directly from public Tigris URLs. Keep the backup bucket private and use per-bucket backup credentials.

Manual deploys use `.github/workflows/deploy.yml` and require GitHub Environment approval for `production`. Every workflow run builds a Fly image, quiesces the app machine, uploads a SQLite backup to the private backup bucket, optionally runs destructive reset modes, deploys the normal app, and runs `ops check` plus `/healthz`. The destructive workflow inputs default to `false` and require `confirm_destructive` to equal `WIPE` when enabled.

## Product Direction

- Curated model catalog, not arbitrary Onshape URL export.
- Onshape document versions only, not mutable workspaces.
- Anonymous end users, using server-owned Onshape credentials.
- Download formats: STEP, STL, and raw Onshape geometry 3MF.
- Preview format: cached GLB or single glTF viewer asset shown in a browser 3D viewer.
- Runtime: Fly.io Rust app at `https://onshape-export.fly.dev` if the app name is available.
- Cache backend: Tigris Object Storage via Fly, with public stable artifact URLs.
- Coordination database: SQLite on a Fly volume for queue/job uniqueness.
- Catalog database: SQLite live application data; checked-in JSON is only a seed/test fixture.
- No public API initially; expose product/UI routes only.
- No web admin UI initially; maintenance operations are CLI or Fly operational commands.

## Documentation

Project documentation is under `docs/src/project/`.

- [Project Overview](docs/src/project/index.md)
- [Architecture](docs/src/project/architecture.md)
- [Runtime Options](docs/src/project/runtime.md)
- [Caching](docs/src/project/caching.md)
- [Onshape API Flow](docs/src/project/onshape-api.md)
- [Slicer Project Generators](docs/src/project/slicer-project-generators.md)
- [Slicer Project Generator Integration Policy](docs/src/project/slicer-project-generator-integration.md)
- [Frontend and Preview](docs/src/project/frontend-preview.md)
- [Catalog](docs/src/project/catalog.md)
- [Admin Operations](docs/src/project/admin.md)
- [CI And Local Tooling](docs/src/project/ci.md)
- [Library Reuse](docs/src/project/library-reuse.md)
- [Decisions](docs/src/project/decisions.md)
- [Open Questions](docs/src/project/open-questions.md)

## Reference Projects

- `~/repos/onshape-mcp`: Onshape auth/client patterns and request modeling.
- `~/repos/onshape3mf`: Python proof-of-concept for configuration discovery and async export calls.
- [`slicer-project-generators`](https://github.com/altendky/slicer-project-generators): canonical target-side generator project.
- [Pinned generator provenance policy](https://github.com/altendky/slicer-project-generators/blob/7650510c72ef5af05b0d62388020f525cface0d9/docs/src/project/slicer-project-generator-provenance.md): source-access, provenance, build, and generator-release authority.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution licensing terms.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.
