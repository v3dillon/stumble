#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="${STUMBLE_DATA_DIR:-$HOME/.stumble/nodes/default}"
PODCTL="${STUMBLE_PODCTL:-$ROOT/target/release/podctl}"
TOKEN="${STUMBLE_DISCOVERY_TOKEN:-}"
HARNESS_COMMAND="${STUMBLE_DISCOVERY_HARNESS_COMMAND:-}"
API_URL="${STUMBLE_API_URL:-}"
EVENT_PATH="${STUMBLE_DISCOVERY_EVENT_PATH:-$DATA_DIR/discovery-ready.json}"

if [[ -z "$TOKEN" ]]; then
  printf 'STUMBLE_DISCOVERY_TOKEN is required\n' >&2
  exit 2
fi

if [[ -n "$API_URL" ]]; then
  curl -fsS -X POST "$API_URL/discovery-tasks" -H "authorization: Bearer $TOKEN" >/dev/null
  tasks="$(curl -fsS "$API_URL/discovery-tasks/ready" -H "authorization: Bearer $TOKEN")"
else
  "$PODCTL" --data-dir "$DATA_DIR" --token "$TOKEN" materialize-discovery-tasks >/dev/null
  tasks="$("$PODCTL" --data-dir "$DATA_DIR" --token "$TOKEN" list-ready-discovery-tasks)"
fi
compact="${tasks//$'\n'/}"
compact="${compact// /}"
emit_event() {
  printf '{"type":"discovery_ready","tasks":%s}\n' "$tasks"
}

mkdir -p "$(dirname "$EVENT_PATH")"
umask 077
event_tmp="$EVENT_PATH.tmp.$$"
if [[ "$compact" == "[]" ]]; then
  printf '{"type":"discovery_idle","tasks":[]}\n' >"$event_tmp"
  mv -f "$event_tmp" "$EVENT_PATH"
  exit 0
fi
emit_event >"$event_tmp"
mv -f "$event_tmp" "$EVENT_PATH"

if [[ -n "$HARNESS_COMMAND" ]]; then
  if [[ ! -x "$HARNESS_COMMAND" ]]; then
    printf 'configured harness command is not executable: %s\n' "$HARNESS_COMMAND" >&2
    exit 2
  fi
  "$HARNESS_COMMAND" <"$EVENT_PATH"
else
  cat "$EVENT_PATH"
fi
