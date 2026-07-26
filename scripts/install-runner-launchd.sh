#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LABEL="${STUMBLE_RUNNER_LABEL:-io.stumble.runner}"
CONFIG="${STUMBLE_RUNNER_CONFIG:-$HOME/.config/stumble/runner.yaml}"
BIN="${STUMBLE_RUNNER_BIN:-$HOME/.cargo/bin/stumble-runner}"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
LOG_DIR="${STUMBLE_LOG_DIR:-$HOME/.stumble/logs}"

if [[ ! -f "$CONFIG" ]]; then
  printf 'Runner config does not exist: %s\n' "$CONFIG" >&2
  exit 2
fi

# Prove the replacement builds before changing a working installation.
cargo build --release -p stumble-cli --bin stumble-runner --manifest-path "$ROOT/Cargo.toml"

# Migrate the former API + three-job scheduler installation before installing
# the unified daemon. Targets are exact and belong only to the legacy setup.
for legacy_label in io.stumble.node io.stumble.discovery io.stumble.pod-discovery io.stumble.pod-curation; do
  launchctl bootout "gui/$(id -u)/$legacy_label" >/dev/null 2>&1 || true
  rm -f "$HOME/Library/LaunchAgents/$legacy_label.plist"
done
rm -f \
  "$HOME/.local/libexec/stumble-codex-mcp" \
  "$HOME/.local/libexec/stumble-codex-personal-worker-mcp" \
  "$HOME/.local/libexec/stumble-codex-pod-worker-mcp" \
  "$HOME/.local/libexec/stumble-personal-discovery-codex" \
  "$HOME/.local/libexec/stumble-pod-discovery-codex" \
  "$HOME/.local/libexec/stumble-pod-curator" \
  "$HOME/.local/libexec/stumble-wake-discovery" \
  "$HOME/.cargo/bin/stumble-mcp" \
  "$HOME/.stumble/bin/stumble-api"

mkdir -p "$(dirname "$BIN")" "$(dirname "$PLIST")" "$LOG_DIR"
install -m 700 "$ROOT/target/release/stumble-runner" "$BIN"

escape_xml() {
  sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' -e 's/"/\&quot;/g'
}

BIN_XML="$(printf '%s' "$BIN" | escape_xml)"
CONFIG_XML="$(printf '%s' "$CONFIG" | escape_xml)"
OUT_XML="$(printf '%s' "$LOG_DIR/stumble-runner.out.log" | escape_xml)"
ERR_XML="$(printf '%s' "$LOG_DIR/stumble-runner.err.log" | escape_xml)"

cat >"$PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array>
    <string>$BIN_XML</string><string>--config</string><string>$CONFIG_XML</string><string>serve</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$OUT_XML</string>
  <key>StandardErrorPath</key><string>$ERR_XML</string>
</dict></plist>
PLIST
chmod 600 "$PLIST"
plutil -lint "$PLIST"
launchctl bootout "gui/$(id -u)/$LABEL" >/dev/null 2>&1 || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
