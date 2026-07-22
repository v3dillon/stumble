#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABEL="${STUMBLE_LAUNCHD_LABEL:-io.stumble.node}"
BIND="${STUMBLE_BIND:-127.0.0.1:8787}"
DATA_DIR="${STUMBLE_DATA_DIR:-$HOME/.stumble/nodes/default}"
LOG_DIR="${STUMBLE_LOG_DIR:-$HOME/.stumble/logs}"
PLIST_DIR="$HOME/Library/LaunchAgents"
PLIST="$PLIST_DIR/$LABEL.plist"
BUILT_BIN="$ROOT/target/release/stumble-api"
BIN="${STUMBLE_BIN:-$HOME/.stumble/bin/stumble-api}"

xml_escape() {
  sed \
    -e 's/&/\&amp;/g' \
    -e 's/</\&lt;/g' \
    -e 's/>/\&gt;/g' \
    -e 's/"/\&quot;/g' \
    -e "s/'/\&apos;/g"
}

escape_value() {
  printf '%s' "$1" | xml_escape
}

if [[ "${STUMBLE_SKIP_BUILD:-}" != "1" ]]; then
  cargo build -p stumble-api --release --manifest-path "$ROOT/Cargo.toml"
fi

mkdir -p "$DATA_DIR" "$LOG_DIR" "$PLIST_DIR" "$(dirname "$BIN")"
cp -Xf "$BUILT_BIN" "$BIN"
chmod +x "$BIN"

BIN_XML="$(escape_value "$BIN")"
ROOT_XML="$(escape_value "$ROOT")"
BIND_XML="$(escape_value "$BIND")"
DATA_DIR_XML="$(escape_value "$DATA_DIR")"
OUT_LOG_XML="$(escape_value "$LOG_DIR/stumble-node.out.log")"
ERR_LOG_XML="$(escape_value "$LOG_DIR/stumble-node.err.log")"
LABEL_XML="$(escape_value "$LABEL")"

cat > "$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL_XML</string>

  <key>ProgramArguments</key>
  <array>
    <string>$BIN_XML</string>
    <string>--mode</string>
    <string>local</string>
    <string>--bind</string>
    <string>$BIND_XML</string>
    <string>--data-dir</string>
    <string>$DATA_DIR_XML</string>
  </array>

  <key>WorkingDirectory</key>
  <string>$ROOT_XML</string>

  <key>RunAtLoad</key>
  <true/>

  <key>KeepAlive</key>
  <true/>

  <key>StandardOutPath</key>
  <string>$OUT_LOG_XML</string>

  <key>StandardErrorPath</key>
  <string>$ERR_LOG_XML</string>
</dict>
</plist>
PLIST

plutil -lint "$PLIST"

launchctl bootout "gui/$(id -u)" "$PLIST" >/dev/null 2>&1 || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
for _ in {1..20}; do
  if launchctl print "gui/$(id -u)/$LABEL" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
launchctl kickstart -k "gui/$(id -u)/$LABEL" >/dev/null 2>&1 || true

printf 'Installed and started %s\n' "$LABEL"
printf 'Plist: %s\n' "$PLIST"
printf 'Binary: %s\n' "$BIN"
printf 'API: http://%s\n' "$BIND"
