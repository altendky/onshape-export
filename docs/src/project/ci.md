# CI And Local Tooling

## Direction

Use `onshape-mcp` as the reference for GitHub repository policy, local checks, Mergify, Renovate, and reusable GitHub Actions patterns. Start with a smaller workflow set because this repository currently contains planning docs only.

The immediate goal is to make pull requests mergeable under the required GitHub ruleset check named `all` while avoiding Rust, npm, and release automation before application code exists.

## Initial Local Tooling

Use `mise` for reproducible local and CI tools.

Initial tools:

- `python` for `pre-commit`.
- `pipx:pre-commit` for hook execution.
- `node` for JavaScript-based hook tools when required.
- `cargo:mdbook = "0.5.3"` for documentation builds and tests, matching the mdBook setup added to `onshape-mcp` in PR 515.
- `lychee` for Markdown link checking.

Run mdBook through `mise exec` in hooks and CI so the pinned tool is used consistently:

```text
mise exec -- mdbook build docs
mise exec -- mdbook test docs
```

Keep `docs/book.toml` checked in. It should set `src = "src"`, avoid creating missing files during builds, and keep generated output under `docs/book`.

Defer Rust-specific tools until a Rust workspace exists:

- `cargo-nextest`.
- `cargo-deny`.
- `cargo-llvm-cov`.
- Rust toolchain resolution helpers.

## Pre-commit Hooks

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

`onshape-mcp` currently added only the `mdbook-build` hook in PR 515. This project should also include `mdbook-test` because documentation snippets should stay executable when practical.

Do not add active Cargo hooks yet. When the Rust skeleton exists, add:

- `cargo fmt --all --check`.
- `cargo clippy --all-targets --all-features -- -D warnings`.
- Manual `cargo nextest run --all-features`.
- Manual `cargo deny check`.

## Initial GitHub Actions

Add a trimmed workflow set based on `onshape-mcp`:

- `.github/workflows/ci.yml`.
- `.github/workflows/reflow-mise.yml`.
- `.github/workflows/reflow-pre-commit.yml`.
- `.github/workflows/reflow-docs.yml`.
- `.github/actionlint.yaml`.

The top-level `ci.yml` should run on pushes to `main` and on pull requests. It should keep the same concurrency behavior as `onshape-mcp`, cancelling older PR runs for the same PR.

Initial jobs:

- `mise`: verify `mise.lock` with `mise install --locked --dry-run`.
- `pre-commit`: run `pre-commit run --show-diff-on-failure --color=always --all-files`.
- `docs`: run `mise exec -- mdbook build docs` and `mise exec -- mdbook test docs`.
- `all`: aggregate the required jobs with `re-actors/alls-green`.

The aggregate job must be named exactly `all` because the repository ruleset requires that status check.

## Mergify

Add `.mergify.yml` using the same queue pattern as `onshape-mcp`:

- Queue name: `default`.
- Queue condition: PR has the `enqueue` label.
- Base branch: `main`.
- Draft PRs are excluded.
- Merge method: `merge`.
- Merge protections reported by deployments.
- `max_parallel_checks: 1`.

Keep the Renovate auto-approval rule if Renovate is enabled in the same step. Do not add post-release auto-approval until release automation exists.

## Renovate

Add a trimmed `renovate.json5`:

- Extend `config:recommended`.
- Extend `helpers:pinGitHubActionDigests`.
- Enable the pre-commit manager.
- Manage `mise.toml` and `mise.lock` updates.

Defer Rust-specific custom managers until these files exist:

- `Cargo.toml`.
- `rust-toolchain.toml`.

The repository already has the Renovate GitHub App variable and private key secret configured. The Renovate workflow can be added once the configuration files are present.

## Deferred Workflows

Do not copy the full `onshape-mcp` workflow set yet.

Defer until Rust code exists:

- `reflow-rust.yml`.
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

## First Implementation Sequence

1. Add `mise.toml`, `mise.lock`, `.pre-commit-config.yaml`, `.lychee.toml`, `.mergify.yml`, and `renovate.json5`.
2. Add the minimal GitHub workflow files.
3. Run local `mise install --locked --dry-run`.
4. Run `mise exec -- mdbook build docs` and `mise exec -- mdbook test docs` locally.
5. Run `pre-commit run mdbook-build --all-files`, `pre-commit run mdbook-test --all-files`, and then `pre-commit run --all-files`.
6. Open a pull request with the `enqueue` label.
7. Confirm the required `all` check passes and Mergify can queue the PR.

Because `main` is protected by repository rules, CI/tooling changes should go through a pull request rather than direct pushes.
