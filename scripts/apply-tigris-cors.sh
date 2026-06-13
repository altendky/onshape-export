#!/usr/bin/env sh
set -eu

cors_file="${1:-scripts/tigris-cors.json}"

: "${TIGRIS_ENDPOINT_URL:?set TIGRIS_ENDPOINT_URL}"
: "${AWS_REGION:?set AWS_REGION}"
: "${TIGRIS_BUCKET:?set TIGRIS_BUCKET}"

aws s3api put-bucket-cors \
  --endpoint-url "$TIGRIS_ENDPOINT_URL" \
  --region "$AWS_REGION" \
  --bucket "$TIGRIS_BUCKET" \
  --cors-configuration "file://$cors_file"
