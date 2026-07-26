#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABEL="${STUMBLE_DISCOVERY_LAUNCHD_LABEL:-io.stumble.discovery}"
INTERVAL="${STUMBLE_DISCOVERY_INTERVAL_SECONDS:-300}"
PLIST_DIR="$HOME/Library/LaunchAgents"
PLIST="$PLIST_DIR/$LABEL.plist"
LOG_DIR="${STUMBLE_LOG_DIR:-$HOME/.stumble/logs}"
WAKE_BIN="${STUMBLE_DISCOVERY_WAKE_BIN:-$HOME/.local/libexec/stumble-wake-discovery}"

xml_escape() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g' -e "s/'/\&apos;/g"
}

escape_value() {
  printf '%s' "$1" | xml_escape
}

if [[ -z "${STUMBLE_DISCOVERY_TOKEN:-}" ]]; then
  printf 'STUMBLE_DISCOVERY_TOKEN is required\n' >&2
  exit 2
fi
if [[ ! "$INTERVAL" =~ ^[1-9][0-9]*$ ]]; then
  printf 'STUMBLE_DISCOVERY_INTERVAL_SECONDS must be a positive integer\n' >&2
  exit 2
fi

mkdir -p "$PLIST_DIR" "$LOG_DIR" "$(dirname "$WAKE_BIN")"
cp -Xf "$ROOT/scripts/wake-discovery.sh" "$WAKE_BIN"
chmod 700 "$WAKE_BIN"
LABEL_XML="$(escape_value "$LABEL")"
WAKE_XML="$(escape_value "$WAKE_BIN")"
DATA_DIR_XML="$(escape_value "${STUMBLE_DATA_DIR:-$HOME/.stumble/nodes/home}")"
STUMBLE_XML="$(escape_value "${STUMBLE_CLI:-$ROOT/target/release/stumble}")"
TOKEN_XML="$(escape_value "$STUMBLE_DISCOVERY_TOKEN")"
HARNESS_XML="$(escape_value "${STUMBLE_DISCOVERY_HARNESS_COMMAND:-}")"
EVENT_PATH_XML="$(escape_value "${STUMBLE_DISCOVERY_EVENT_PATH:-${STUMBLE_DATA_DIR:-$HOME/.stumble/nodes/home}/discovery-ready.json}")"
OUT_XML="$(escape_value "$LOG_DIR/stumble-discovery.out.log")"
ERR_XML="$(escape_value "$LOG_DIR/stumble-discovery.err.log")"
cat >"$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL_XML</string>
  <key>ProgramArguments</key><array><string>$WAKE_XML</string></array>
  <key>StartInterval</key><integer>$INTERVAL</integer>
  <key>StandardOutPath</key><string>$OUT_XML</string>
  <key>StandardErrorPath</key><string>$ERR_XML</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>STUMBLE_DATA_DIR</key><string>$DATA_DIR_XML</string>
    <key>STUMBLE_CLI</key><string>$STUMBLE_XML</string>
    <key>STUMBLE_DISCOVERY_TOKEN</key><string>$TOKEN_XML</string>
    <key>STUMBLE_DISCOVERY_HARNESS_COMMAND</key><string>$HARNESS_XML</string>
    <key>STUMBLE_DISCOVERY_EVENT_PATH</key><string>$EVENT_PATH_XML</string>
  </dict>
</dict>
</plist>
PLIST
chmod 600 "$PLIST"

plutil -lint "$PLIST"
launchctl bootout "gui/$(id -u)" "$PLIST" >/dev/null 2>&1 || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
printf 'Installed %s at %s\n' "$LABEL" "$PLIST"
