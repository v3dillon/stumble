#!/usr/bin/env bash
# Deploys a Stumble Bootstrap + Index + Relay node on an Ubuntu/Debian VPS.
#
#   sudo ./scripts/deploy-bootstrap-vps.sh bootstrap.example.com
#   flags: --no-index, --no-relay
#
# Idempotent: re-run after `git pull` to upgrade in place. Installs Rust and
# Caddy if missing, builds release binaries, initializes a Home Node under a
# dedicated system user, and serves HTTPS via Caddy's automatic certificates.
# Prerequisite: a DNS A/AAAA record for the domain pointing at this machine.
set -euo pipefail

DOMAIN="${1:?usage: deploy-bootstrap-vps.sh <domain> [--no-index] [--no-relay]}"
shift
INDEX_FLAG="--index"
RELAY_FLAG="--relay"
for flag in "$@"; do
  case "$flag" in
    --no-index) INDEX_FLAG="" ;;
    --no-relay) RELAY_FLAG="" ;;
    *)
      echo "unknown flag: $flag (expected --no-index or --no-relay)" >&2
      exit 1
      ;;
  esac
done
if [[ "$(id -u)" -ne 0 ]]; then
  echo "run with sudo (installs packages, writes systemd units)" >&2
  exit 1
fi

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
STUMBLE_HOME=/var/lib/stumble
DATA_DIR="$STUMBLE_HOME/node"
CREDENTIAL_DIR="$STUMBLE_HOME/credentials"
BIND=127.0.0.1:8787

echo "==> Installing system packages"
export DEBIAN_FRONTEND=noninteractive
apt-get update -q
apt-get install -qy curl git build-essential ca-certificates debian-keyring \
  debian-archive-keyring apt-transport-https

if ! command -v caddy >/dev/null; then
  echo "==> Installing Caddy"
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' |
    gpg --batch --yes --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    >/etc/apt/sources.list.d/caddy-stable.list
  apt-get update -q
  apt-get install -qy caddy
fi

if ! command -v cargo >/dev/null; then
  echo "==> Installing Rust"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
fi
export PATH="$HOME/.cargo/bin:$PATH"

echo "==> Building release binaries"
# One package ships both product binaries (CLI + HTTP server).
cargo build --release --manifest-path "$REPO_DIR/Cargo.toml" -p stumble-cli
install -m 0755 "$REPO_DIR/target/release/stumble" /usr/local/bin/stumble
install -m 0755 "$REPO_DIR/target/release/stumble-api" /usr/local/bin/stumble-api

echo "==> Preparing the stumble service user and Home Node"
id -u stumble >/dev/null 2>&1 || useradd --system --home "$STUMBLE_HOME" --shell /usr/sbin/nologin stumble
mkdir -p "$DATA_DIR" "$CREDENTIAL_DIR"
chown -R stumble:stumble "$STUMBLE_HOME"
if ! sudo -u stumble STUMBLE_DATA_DIR="$DATA_DIR" \
  STUMBLE_CREDENTIAL_STORE_DIR="$CREDENTIAL_DIR" /usr/local/bin/stumble node show >/dev/null 2>&1; then
  sudo -u stumble STUMBLE_DATA_DIR="$DATA_DIR" \
    STUMBLE_CREDENTIAL_STORE_DIR="$CREDENTIAL_DIR" /usr/local/bin/stumble node init >/dev/null
  echo "    initialized a fresh Home Node at $DATA_DIR"
fi

echo "==> Writing systemd unit"
cat >/etc/systemd/system/stumble-bootstrap.service <<UNIT
[Unit]
Description=Stumble Bootstrap/Index/Relay node
After=network-online.target
Wants=network-online.target

[Service]
User=stumble
Environment=STUMBLE_DATA_DIR=$DATA_DIR
Environment=STUMBLE_CREDENTIAL_STORE_DIR=$CREDENTIAL_DIR
ExecStart=/usr/local/bin/stumble-api --bootstrap $INDEX_FLAG $RELAY_FLAG --bind $BIND --base-url https://$DOMAIN
Restart=always
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=$STUMBLE_HOME
ProtectHome=true

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now stumble-bootstrap
systemctl restart stumble-bootstrap

echo "==> Configuring Caddy for https://$DOMAIN"
cat >/etc/caddy/Caddyfile <<CADDY
$DOMAIN {
	reverse_proxy $BIND
}
CADDY
systemctl reload caddy || systemctl restart caddy

echo "==> Waiting for the node to answer"
for _ in $(seq 1 30); do
  if curl -fsS "http://$BIND/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
curl -fsS "http://$BIND/.well-known/stumble-node" >/dev/null

cat <<DONE

Deployed. Verify from anywhere once DNS + certificates settle (~1 minute):

    curl https://$DOMAIN/.well-known/stumble-node

Point Home Nodes at it:

    stumble sync bootstrap add --label $DOMAIN --base-url https://$DOMAIN
    stumble sync discovery index add --label $DOMAIN --base-url https://$DOMAIN

Upgrade later: git pull, then re-run this script.
DONE
