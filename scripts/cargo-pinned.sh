#!/usr/bin/env bash
set -euo pipefail

toolchain="$(awk -F'"' '/^channel = / { print $2; exit }' rust-toolchain.toml)"
if [[ -z "$toolchain" ]]; then
  printf 'failed to read Rust toolchain channel from rust-toolchain.toml\n' >&2
  exit 1
fi

rustup_bin="$(command -v rustup || true)"
if [[ -z "$rustup_bin" && -x "$HOME/.cargo/bin/rustup" ]]; then
  rustup_bin="$HOME/.cargo/bin/rustup"
fi

if [[ -z "$rustup_bin" ]]; then
  printf 'failed to find rustup\n' >&2
  exit 1
fi

toolchain_rustc="$("$rustup_bin" which --toolchain "$toolchain" rustc)"
toolchain_bin="${toolchain_rustc%/*}"

PATH="$toolchain_bin:$PATH" exec "$rustup_bin" run "$toolchain" cargo "$@"
