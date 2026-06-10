# CI And Local Tooling

## Current Branch Status

The branch currently has `.github/workflows/ci.yml` with a temporary job named exactly `all`. That job exists only to satisfy the required aggregate GitHub check while the real workflow set is rebuilt.

The placeholder does not run Rust formatting, linting, tests, mdBook, pre-commit, Mergify validation, Renovate validation, or release checks. Do not treat a passing `all` check as implementation quality evidence yet.

Rust application code, Cargo files, migrations, scripts, and `mise.toml` already exist on this branch. The older main-branch wording that avoided Rust checks because the repository contained planning docs only is no longer accurate for this branch.

## Direction

Use `onshape-mcp` as the reference for GitHub repository policy, local checks, Mergify, Renovate, and reusable GitHub Actions patterns. Start with a smaller workflow set, then add Rust-specific checks now that a Rust crate exists.

The immediate workflow requirement is a GitHub status check named `all`. The durable target is for `all` to aggregate real required jobs with `re-actors/alls-green` or an equivalent aggregate-check pattern.

## Local Tooling Target

Use `mise` for reproducible local and CI tools.

Initial tools from the main plan:

- `python` for `pre-commit`.
- `pipx:pre-commit` for hook execution.
- `node` for JavaScript-based hook tools when required.
- `cargo:mdbook = "0.5.3"` for documentation builds and tests.
- `lychee` for Markdown link checking.

Run mdBook through the pinned tool once the mise configuration is verified:

```text
mise exec -- mdbook build docs
mise exec -- mdbook test docs
```

Direct `mdbook build docs` and `mdbook test docs` are acceptable local verification only when `mdbook` is already available outside mise.

## Pre-commit Target

Copy the general-purpose `onshape-mcp` hook set first:

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

Add local documentation hooks:

- `mdbook-build` with `entry: mise exec -- mdbook build docs`, `files: ^docs/`, and `pass_filenames: false`.
- `mdbook-test` with `entry: mise exec -- mdbook test docs`, `files: ^docs/`, and `pass_filenames: false`.

Add Rust hooks now that `Cargo.toml` exists:

- `cargo fmt --all --check`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- `cargo test --all-targets --all-features` or `cargo nextest run --all-features` once nextest is configured.

## GitHub Actions Target

The current committed workflow is intentionally smaller than the target. It should be expanded in a tooling-focused change.

Target jobs:

- `mise`: verify `mise.lock` with `mise install --locked --dry-run` once the lockfile is committed intentionally.
- `pre-commit`: run `pre-commit run --show-diff-on-failure --color=always --all-files`.
- `docs`: run `mise exec -- mdbook build docs` and `mise exec -- mdbook test docs`.
- `rust`: run formatting, clippy, and tests for the crate.
- `all`: aggregate the required jobs and remain named exactly `all`.

The top-level workflow should eventually run on pushes to `main` and on pull requests. It should keep the same concurrency behavior as `onshape-mcp`, cancelling older pull request runs for the same pull request.

## Deferred Workflows

Do not copy the full `onshape-mcp` workflow set yet.

Defer until the Rust checks are stable:

- `reflow-rust.yml` split workflows.
- `reflow-coverage.yml`.
- `deny.toml`.
- Rust setup composite actions.
- `cargo nextest` archive/test matrix.

Defer until package or binary release requirements exist:

- Release version verification.
- Release binary builds.
- Tag-release automation.
- Post-release PR automation.
- GitHub release publishing.
- npm publishing.
- crates.io publishing.
- npm staging cleanup.

## TODO Sequence

1. Keep the placeholder `all` check until a real aggregate workflow is ready.
2. Reconcile `mise.toml` and `mise.lock` intentionally before wiring CI to them.
3. Add `.pre-commit-config.yaml`, `.lychee.toml`, `.mergify.yml`, and Renovate config in a tooling-scoped change.
4. Add docs and Rust jobs under the aggregate `all` check.
5. Run local `mdbook build docs`, `mdbook test docs`, Rust checks, and pre-commit checks before requiring those jobs in repository rules.
