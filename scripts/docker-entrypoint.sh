#!/bin/sh
# First-boot init + stumble-api for container hosts (Coolify, Compose, etc.).
# Persistent data must live on a volume mounted at /data.
set -eu

DATA_DIR="${STUMBLE_DATA_DIR:-/data/node}"
CRED_DIR="${STUMBLE_CREDENTIAL_STORE_DIR:-/data/credentials}"
export STUMBLE_DATA_DIR="$DATA_DIR"
export STUMBLE_CREDENTIAL_STORE_DIR="$CRED_DIR"

mkdir -p "$DATA_DIR" "$CRED_DIR"
if [ "$(id -u)" -eq 0 ]; then
  chown -R stumble:stumble /data
  run_as="runuser -u stumble --"
else
  run_as=""
fi

if ! $run_as stumble node show >/dev/null 2>&1; then
  echo "initializing Home Node at $DATA_DIR"
  $run_as stumble node init
fi

BOOTSTRAP_FLAG=""
INDEX_FLAG=""
RELAY_FLAG=""
case "${STUMBLE_BOOTSTRAP:-1}" in 0|false|FALSE|off|OFF) ;; *) BOOTSTRAP_FLAG="--bootstrap" ;; esac
case "${STUMBLE_INDEX:-1}" in 0|false|FALSE|off|OFF) ;; *) INDEX_FLAG="--index" ;; esac
case "${STUMBLE_RELAY:-1}" in 0|false|FALSE|off|OFF) ;; *) RELAY_FLAG="--relay" ;; esac

if [ -z "${STUMBLE_BASE_URL:-}" ]; then
  echo "STUMBLE_BASE_URL is required (e.g. https://bootstrap.stumble.network)" >&2
  exit 1
fi

exec $run_as stumble-api \
  $BOOTSTRAP_FLAG $INDEX_FLAG $RELAY_FLAG \
  --bind 0.0.0.0:8787 \
  --base-url "$STUMBLE_BASE_URL"
