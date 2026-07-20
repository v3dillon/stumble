#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_DIR="${STUMBLE_DATA_DIR:-$HOME/.stumble/nodes/home}"
STUMBLE="${STUMBLE_CLI:-$ROOT/target/release/stumble}"
TOKEN="${STUMBLE_DISCOVERY_TOKEN:-}"
HARNESS_COMMAND="${STUMBLE_DISCOVERY_HARNESS_COMMAND:-}"
EVENT_PATH="${STUMBLE_DISCOVERY_EVENT_PATH:-$DATA_DIR/discovery-ready.json}"

if [[ -z "$TOKEN" ]]; then
  printf 'STUMBLE_DISCOVERY_TOKEN is required\n' >&2
  exit 2
fi

response="$(STUMBLE_HARNESS_CREDENTIAL="$TOKEN" "$STUMBLE" \
  --data-dir "$DATA_DIR" discover task list --state ready --limit 100)"
tasks="$(printf '%s' "$response" | sed -E \
  's/^\{"version":2,"data":\{"items":(\[.*\]),"next_cursor":(null|"[^"]*")\}\}$/\1/')"
if [[ "$tasks" == "$response" ]]; then
  printf 'stumble returned an unexpected Discovery Task response\n' >&2
  exit 1
fi

# Inspectable schedule backpressure for Personal Discovery schedules (same identities as list/claim).
schedules_response="$(STUMBLE_HARNESS_CREDENTIAL="$TOKEN" "$STUMBLE" \
  --data-dir "$DATA_DIR" discover personal schedule list 2>/dev/null || true)"
if [[ -n "$schedules_response" ]]; then
  schedule_backpressure="$(printf '%s' "$schedules_response" | sed -E \
    's/^\{"version":2,"data":(\[.*\])\}$/\1/')"
  if [[ "$schedule_backpressure" == "$schedules_response" ]]; then
    schedule_backpressure='[]'
  fi
else
  schedule_backpressure='[]'
fi

compact="${tasks//$'\n'/}"
compact="${compact// /}"
emit_event() {
  printf '{"type":"discovery_ready","tasks":%s,"schedule_backpressure":%s}\n' \
    "$tasks" "$schedule_backpressure"
}

mkdir -p "$(dirname "$EVENT_PATH")"
umask 077
event_tmp="$EVENT_PATH.tmp.$$"
if [[ "$compact" == "[]" ]]; then
  printf '{"type":"discovery_idle","tasks":[],"schedule_backpressure":%s}\n' \
    "$schedule_backpressure" >"$event_tmp"
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
