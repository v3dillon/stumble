#!/usr/bin/env bash
# Install Stumble product binaries from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/v3dillon/stumble/main/scripts/install.sh | bash
#   STUMBLE_VERSION=v0.1.0 bash install.sh        # pin a release
#   STUMBLE_INSTALL_DIR=~/bin bash install.sh     # custom destination
#
# Installs: stumble, stumble-api, stumble-runner
set -euo pipefail

REPO="${STUMBLE_REPO:-v3dillon/stumble}"
INSTALL_DIR="${STUMBLE_INSTALL_DIR:-${HOME}/.local/bin}"
VERSION="${STUMBLE_VERSION:-latest}"
TMPDIR="${TMPDIR:-/tmp}"
WORKDIR="$(mktemp -d "${TMPDIR%/}/stumble-install.XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

detect_asset() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m)"

  case "$os" in
    darwin) os="macos" ;;
    linux) os="linux" ;;
    *) die "unsupported OS: $(uname -s) (supported: macOS, Linux)" ;;
  esac

  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="arm64" ;;
    *) die "unsupported architecture: $arch (supported: x86_64, arm64)" ;;
  esac

  printf '%s-%s' "$os" "$arch"
}

download() {
  local url="$1" dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 --retry-delay 1 -o "$dest" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$dest" "$url"
  else
    die "need curl or wget to download releases"
  fi
}

need_cmd uname
need_cmd tar
need_cmd mktemp
need_cmd install

ASSET="$(detect_asset)"
ARCHIVE="stumble-${ASSET}.tar.gz"

if [[ "$VERSION" == "latest" ]]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  # Accept both v0.1.0 and 0.1.0
  case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
  esac
  BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

URL="${BASE_URL}/${ARCHIVE}"
log "Downloading ${ARCHIVE} from ${REPO} (${VERSION})"
if ! download "$URL" "${WORKDIR}/${ARCHIVE}"; then
  die "download failed: ${URL}
Is there a published release for this platform? See https://github.com/${REPO}/releases"
fi

log "Extracting"
tar -xzf "${WORKDIR}/${ARCHIVE}" -C "$WORKDIR"

# Tarball contains a single top-level directory with the three binaries.
SRC_DIR="$(find "$WORKDIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
[[ -n "$SRC_DIR" ]] || die "archive had no top-level directory"

mkdir -p "$INSTALL_DIR"
for bin in stumble stumble-api stumble-runner; do
  [[ -f "${SRC_DIR}/${bin}" ]] || die "archive missing binary: ${bin}"
  install -m 0755 "${SRC_DIR}/${bin}" "${INSTALL_DIR}/${bin}"
  log "Installed ${INSTALL_DIR}/${bin}"
done

# PATH hint when the install dir is not already searchable.
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    printf '\n'
    log "Add ${INSTALL_DIR} to your PATH, then open a new shell:"
    printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
    if [[ "$(uname -s)" == "Darwin" ]]; then
      printf '    # e.g. add that line to ~/.zprofile or ~/.zshrc\n'
    else
      printf '    # e.g. add that line to ~/.bashrc or ~/.profile\n'
    fi
    ;;
esac

printf '\n'
log "Stumble is installed. Next:"
printf '    stumble node init\n'
printf '    stumble add "https://example.com/something-worth-keeping"\n'
printf '    stumble\n'
printf '\n'
log "Binaries: stumble, stumble-api, stumble-runner"
