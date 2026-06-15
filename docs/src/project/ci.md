# CI And Local Tooling

## Current Status

The repository has a real GitHub Actions workflow in `.github/workflows/ci.yml`.
It runs on pull requests and pushes to `main`, uses reusable `reflow-*.yml` workflows, and keeps the required aggregate status check named exactly `all`.

The aggregate `all` job currently requires these workflow groups:

- `mise`: verifies the checked-in `mise.lock` with `mise install --locked --dry-run`.
- `pre-commit`: runs `pre-commit run --show-diff-on-failure --color=always --all-files`.
- `docs`: runs `mise exec -- mdbook build docs` and `mise exec -- mdbook test docs`.
- `rust`: runs Rust formatting, clippy, and `cargo nextest run`.
- `security`: runs slower CI-native security scanners in parallel.

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
- Docker for Docker-backed security hooks.
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

Run fast local guardrails against staged changes before committing:

```sh
mise exec -- pre-commit run --show-diff-on-failure --color=always
```

Run the same hook set against branch changes before pushing:

```sh
mise exec -- pre-commit run --from-ref origin/main --to-ref HEAD --show-diff-on-failure --color=always
```

These commands avoid full repository history and dependency scans.
Secret scanning redacts findings, `zizmor` runs offline and filters
informational-only results, and file-specific hooks are skipped by `pre-commit`
when there are no matching local changes.

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
mise exec -- bash scripts/cargo-pinned.sh nextest run --locked --all-features
```

## Security CI

The security workflow is CI-native instead of a single local command so slow
scanners can run in parallel and upload independent structured reports.

Blocking security jobs are:

- `cargo-deny`: Rust dependency policy, advisories, licenses, duplicate-version policy, and source restrictions from `deny.toml`.
  Duplicate crate versions are CI-blocking except for documented `skip-tree` allowances around dependency families whose current semver-incompatible transitive dependency lines are not actionable in this repository.
- `cargo-audit`: RustSec advisories for `Cargo.lock`, with explicit ignores only for lockfile-only false positives that are not reachable in the active dependency graph.
- `osv-scanner`: OSV advisory matches for repository lockfiles and manifests. Its `osv-scanner.toml` ignores must include a reason.
- `trivy-fs`: filesystem vulnerability, misconfiguration, and secret scan, blocking on `MEDIUM`, `HIGH`, or `CRITICAL` findings. Trivy secret scanning is a CI-only second opinion alongside the faster `gitleaks` pre-commit check.
- `semgrep-ci`: checked-in local rules plus Semgrep's `p/security-audit` and `p/rust` rulesets. These broader rulesets run only in CI, not in pre-commit.

Each job writes a JSON or SARIF report under `target/security-audit/` and uploads it as a workflow artifact, even when the scanner exits nonzero.

## Pre-commit Hooks

The default hook set covers:

- Trailing whitespace.
- End-of-file fixer.
- TOML syntax check.
- YAML syntax check.
- Merge-conflict marker check.
- `gitleaks` for redacted staged secret scanning.
- `semgrep` with the checked-in lightweight local rules.
- `typos`.
- `markdownlint-cli2`.
- `lychee` for Markdown links.
- Mergify config validation.
- `actionlint`.
- `zizmor` for offline GitHub Actions security analysis.
- `action-validator`.
- `shellcheck`.
- `hadolint` for Dockerfile linting.
- `cargo fmt --all --check`.
- `cargo clippy --locked --all-targets --all-features -- -D warnings`.
- `mise install --locked --dry-run` when mise files change.
- `mdbook build` and `mdbook test` when docs change.

`cargo nextest` and `cargo deny` are configured as manual hooks because they are heavier checks.
The Rust CI workflow runs `cargo nextest` on every required CI run, and the security workflow runs `cargo deny`.

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
