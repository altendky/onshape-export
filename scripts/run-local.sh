#!/usr/bin/env bash
set -euo pipefail

if [[ -f .env.local ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env.local
  set +a
fi

export TIGRIS_ENDPOINT_URL="${TIGRIS_ENDPOINT_URL:-http://localhost:9000}"
export TIGRIS_BUCKET="${TIGRIS_BUCKET:-onshape-export}"
export TIGRIS_PUBLIC_BASE_URL="${TIGRIS_PUBLIC_BASE_URL:-http://localhost:9000/${TIGRIS_BUCKET}}"
export TIGRIS_FORCE_PATH_STYLE="${TIGRIS_FORCE_PATH_STYLE:-true}"
export AWS_ACCESS_KEY_ID="${AWS_ACCESS_KEY_ID:-minioadmin}"
export AWS_SECRET_ACCESS_KEY="${AWS_SECRET_ACCESS_KEY:-minioadmin}"
export AWS_REGION="${AWS_REGION:-auto}"
export DATABASE_URL="${DATABASE_URL:-sqlite://onshape-export.db?mode=rwc}"

exec cargo run -- "$@"
