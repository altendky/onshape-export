# CI And Local Tooling

## Current Status

The repository has a real GitHub Actions workflow in `.github/workflows/ci.yml`.
It runs on pull requests and pushes to `main`, uses reusable `reflow-*.yml` workflows, and keeps the required aggregate status check named exactly `all`.

The aggregate `all` job currently requires these workflow groups:

- `mise`: verifies the checked-in `mise.lock` with `mise install --locked --dry-run`.
- `pre-commit`: runs `pre-commit run --show-diff-on-failure --color=always --all-files`.
- `docs`: runs `mise exec -- mdbook build docs` and `mise exec -- mdbook test docs`.
- `rust`: runs Rust formatting, clippy, `cargo deny check`, and `cargo nextest run`.

GitHub Actions are pinned to full commit SHAs.
Renovate is configured to maintain dependencies, pre-commit hooks, mise tools, and pinned action digests.

The repository also has a manual deploy workflow in `.github/workflows/deploy.yml`. It uses `workflow_dispatch`, the GitHub Environment named `production`, and `mise`-managed `flyctl`. Configure `FLY_API_TOKEN` as a `production` environment secret, and set `FLY_APP` as a repository or environment variable only if the Fly app name differs from `onshape-export`.

Manual deploys quiesce the single Fly app machine, run app-owned deploy maintenance, upload a SQLite backup to the private backup bucket, optionally apply explicit destructive reset modes, deploy the normal app, and run readiness checks. The quiesced machine receives non-secret `[env]` values from `fly.toml`, and failures before the final app deploy trigger redeployment of the previously running image. Destructive inputs default to `false` and require `confirm_destructive` to be `WIPE`.

## Local Tooling

Use `mise` for reproducible local and CI tools.
The tool versions are declared in `mise.toml` and locked in `mise.lock` for the supported reference platforms.

Configured tools include:

- `node` for JavaScript-based hook tools when required.
- `python` for `pre-commit`.
- `pipx:pre-commit` for hook execution.
- `cargo:mdbook` for documentation builds and tests.
- `cargo:lychee` for Markdown link checking.
- `github:nextest-rs/nextest` for Rust test execution.
- `aqua:EmbarkStudios/cargo-deny` for Rust dependency policy checks.
- `aqua:superfly/flyctl` for Fly operations.

Rust itself is pinned separately in `rust-toolchain.toml`.
`Cargo.toml` declares the same `rust-version` so package metadata matches the toolchain used by CI.

## Local Checks

Install tools and run the default hook set:

```sh
mise install --locked
mise exec -- pre-commit run --show-diff-on-failure --color=always --all-files
```

Run the manual Rust hooks that CI also requires:

```sh
mise exec -- pre-commit run --hook-stage manual --all-files
```

Equivalent direct checks are:

```sh
mise install --locked --dry-run
mise exec -- mdbook build docs
mise exec -- mdbook test docs
mise exec -- bash scripts/cargo-pinned.sh fmt --all --check
mise exec -- bash scripts/cargo-pinned.sh clippy --locked --all-targets --all-features -- -D warnings
mise exec -- bash scripts/cargo-pinned.sh deny check
mise exec -- bash scripts/cargo-pinned.sh nextest run --locked --all-features
```

## Pre-commit Hooks

The default hook set covers:

- Trailing whitespace.
- End-of-file fixer.
- TOML syntax check.
- YAML syntax check.
- Merge-conflict marker check.
- `typos`.
- `markdownlint-cli2`.
- `lychee` for Markdown links.
- Mergify config validation.
- `actionlint`.
- `action-validator`.
- `shellcheck`.
- `cargo fmt --all --check`.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`.
- `mise install --locked --dry-run` when mise files change.
- `mdbook build` and `mdbook test` when docs change.

`cargo nextest` and `cargo deny` are configured as manual hooks because they are heavier checks.
The Rust CI workflow still runs both on every required CI run.

## Renovate

Renovate is configured by `renovate.json5` and `.github/workflows/renovate.yml`.
The workflow runs every 6 hours and supports manual dispatch with optional debug logging.

The workflow expects repository or organization credentials for the Renovate GitHub App:

- `RENOVATE_CLIENT_ID` as a GitHub Actions variable.
- `RENOVATE_APP_PRIVATE_KEY` as a GitHub Actions secret.

Renovate is allowed to run mise lock update commands so `mise.lock` stays synchronized with `mise.toml` changes.

## Reference

`onshape-mcp` remains the reference for repository policy and reusable workflow patterns.
This repository intentionally uses a smaller Rust workflow than `onshape-mcp`: a single Ubuntu Rust job with `cargo nextest`, not the full archive/test platform matrix.

## Deferred Workflows

Defer until the Rust checks are stable or the project needs broader platform guarantees:

- Split Rust archive/test matrix.
- Coverage workflow.
- Additional Rust setup/resolve helpers for matrix builds.

Defer until package or binary release requirements exist:

- Release version verification.
- Release binary builds.
- Tag-release automation.
- Post-release PR automation.
- GitHub release publishing.
- npm publishing.
- crates.io publishing.
- npm staging cleanup.
