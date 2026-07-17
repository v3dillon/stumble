#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABEL="${STUMBLE_LAUNCHD_LABEL:-io.stumble.node}"
INTERVAL_SECONDS="${STUMBLE_WATCH_INTERVAL_SECONDS:-2}"
PLIST="${STUMBLE_LAUNCHD_PLIST:-$HOME/Library/LaunchAgents/$LABEL.plist}"

fingerprint() {
  (
    cd "$ROOT"
    find Cargo.toml Cargo.lock crates migrations -type f \
      \( -name '*.rs' -o -name 'Cargo.toml' -o -name '*.sql' \) \
      -exec stat -f '%m %N' {} \; | sort | shasum
  )
}

rebuild_and_restart() {
  cargo build -p stumble-api --release --manifest-path "$ROOT/Cargo.toml"
  STUMBLE_SKIP_BUILD=1 "$ROOT/scripts/install-launchd.sh"
  for _ in {1..20}; do
    if curl -fsS "http://${STUMBLE_BIND:-127.0.0.1:8787}/health" >/dev/null 2>&1; then
      printf '[stumble] rebuilt and restarted %s\n' "$LABEL"
      return 0
    fi
    sleep 0.5
  done
  printf '[stumble] %s restarted, but health check did not pass\n' "$LABEL" >&2
  return 1
}

if [[ "${1:-}" == "--once" ]]; then
  rebuild_and_restart
  exit 0
fi

printf '[stumble] watching %s\n' "$ROOT"
printf '[stumble] restart target: %s\n' "$LABEL"
printf '[stumble] press Ctrl-C to stop\n'

last="$(fingerprint)"
while true; do
  sleep "$INTERVAL_SECONDS"
  current="$(fingerprint)"
  if [[ "$current" != "$last" ]]; then
    last="$current"
    if ! rebuild_and_restart; then
      printf '[stumble] rebuild or restart failed; continuing to watch\n' >&2
    fi
  fi
done
