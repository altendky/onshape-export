#!/usr/bin/env bash
set -euo pipefail

container_name="${MINIO_CONTAINER_NAME:-onshape-export-minio}"
bucket="${TIGRIS_BUCKET:-onshape-export}"
root_user="${MINIO_ROOT_USER:-minioadmin}"
root_password="${MINIO_ROOT_PASSWORD:-minioadmin}"
api_port="${MINIO_API_PORT:-9000}"
console_port="${MINIO_CONSOLE_PORT:-9001}"

if ! command -v docker >/dev/null 2>&1; then
  printf 'docker is required to run local MinIO\n' >&2
  exit 1
fi

if ! docker ps --format '{{.Names}}' | grep -qx "${container_name}"; then
  if docker ps -a --format '{{.Names}}' | grep -qx "${container_name}"; then
    docker start "${container_name}" >/dev/null
  else
    docker run -d \
      --name "${container_name}" \
      -p "${api_port}:9000" \
      -p "${console_port}:9001" \
      -e "MINIO_ROOT_USER=${root_user}" \
      -e "MINIO_ROOT_PASSWORD=${root_password}" \
      quay.io/minio/minio server /data --console-address ':9001' >/dev/null
  fi
fi

until docker run --rm --network "container:${container_name}" \
  quay.io/minio/mc alias set local http://127.0.0.1:9000 "${root_user}" "${root_password}" >/dev/null 2>&1; do
  sleep 1
done

docker run --rm --network "container:${container_name}" \
  quay.io/minio/mc alias set local http://127.0.0.1:9000 "${root_user}" "${root_password}" >/dev/null
docker run --rm --network "container:${container_name}" \
  quay.io/minio/mc mb --ignore-existing "local/${bucket}" >/dev/null
docker run --rm --network "container:${container_name}" \
  quay.io/minio/mc anonymous set download "local/${bucket}" >/dev/null

cat <<EOF
MinIO is running.

S3 endpoint: http://localhost:${api_port}
Console:     http://localhost:${console_port}
Bucket:      ${bucket}
Username:    ${root_user}
Password:    ${root_password}
EOF
