#!/usr/bin/env sh
# Usage: curl -fsSL https://evot.ai/install | sh
#
# POSIX sh compatible — do NOT use bash-specific syntax (e.g. [[ ]], pipefail,
# arrays, process substitution). This script is piped to 'sh' which may be
# dash on Ubuntu/WSL.
set -e

REPO="evotai/evot"
BINARY="evot"
INSTALL_DIR="${EVOT_INSTALL_DIR:-$HOME/.evotai/bin}"

# --- Colors & helpers ---

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

info()  { printf "${BLUE}%s${NC}\n" "$*"; }
ok()    { printf "${GREEN}%s${NC}\n" "$*"; }
warn()  { printf "${YELLOW}%s${NC}\n" "$*"; }
error() { printf "${RED}%s${NC}\n" "$*" >&2; exit 1; }

# --- Download abstraction (curl with wget fallback) ---

DOWNLOADER=""
if command -v curl > /dev/null 2>&1; then
  DOWNLOADER="curl"
elif command -v wget > /dev/null 2>&1; then
  DOWNLOADER="wget"
else
  error "Either curl or wget is required but neither is installed"
fi

download() {
  _url="$1"; _output="$2"
  if [ "$DOWNLOADER" = "curl" ]; then
    curl -fsSL -o "$_output" "$_url"
  else
    wget -q -O "$_output" "$_url"
  fi
}

fetch() {
  _url="$1"
  if [ "$DOWNLOADER" = "curl" ]; then
    curl -fsSL "$_url"
  else
    wget -qO- "$_url"
  fi
}

# --- Platform detection ---

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) os="darwin" ;;
  Linux)  os="linux" ;;
  *)      error "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
  x86_64|amd64)  arch="x86_64" ;;
  aarch64|arm64)  arch="aarch64" ;;
  *)              error "Unsupported architecture: $ARCH" ;;
esac

case "${os}-${arch}" in
  linux-x86_64)   TARGET="x86_64-unknown-linux-gnu" ;;
  linux-aarch64)  TARGET="aarch64-unknown-linux-gnu" ;;
  darwin-x86_64)  TARGET="x86_64-apple-darwin" ;;
  darwin-aarch64) TARGET="aarch64-apple-darwin" ;;
esac

# --- Version resolution ---

if [ -n "${EVOT_INSTALL_VERSION:-}" ]; then
  TAG="$EVOT_INSTALL_VERSION"
else
  TAG="$(fetch "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')"
fi

[ -z "$TAG" ] && error "Failed to determine latest version. GitHub API rate limit?"
VERSION="${TAG#v}"

ASSET="${BINARY}-v${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"
SHA_URL="${URL}.sha256"

# --- Download & verify ---

info "Installing ${BINARY} v${VERSION} for ${TARGET}..."

TMP="$(mktemp -d)"
BINARY_STAGE=""
LIB_STAGE=""
cleanup() {
  rm -rf "$TMP"
  [ -z "$BINARY_STAGE" ] || rm -f "$BINARY_STAGE"
  [ -z "$LIB_STAGE" ] || rm -rf "$LIB_STAGE"
}
trap cleanup 0
trap 'exit 1' HUP INT TERM

download "$URL" "$TMP/$ASSET"

# SHA256 verification (best-effort: skip if .sha256 file not published)
EXPECTED_SHA="$(fetch "$SHA_URL" 2>/dev/null || true)"
if [ -n "$EXPECTED_SHA" ]; then
  EXPECTED_SHA="$(echo "$EXPECTED_SHA" | awk '{print $1}')"
  if command -v sha256sum > /dev/null 2>&1; then
    ACTUAL_SHA="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
  elif command -v shasum > /dev/null 2>&1; then
    ACTUAL_SHA="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
  else
    ACTUAL_SHA=""
  fi
  if [ -n "$ACTUAL_SHA" ] && [ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]; then
    error "Checksum verification failed (expected $EXPECTED_SHA, got $ACTUAL_SHA)"
  fi
  info "Checksum verified"
fi

# --- Validate package ---

tar -xzf "$TMP/$ASSET" -C "$TMP"

[ -f "$TMP/bin/$BINARY" ] || error "Release archive does not contain bin/$BINARY"
chmod +x "$TMP/bin/$BINARY"

# Preserve the signatures produced by the release workflow. Clearing download
# attributes is safe, but re-signing here can make macOS reject a reused inode.
if [ "$os" = "darwin" ]; then
  xattr -cr "$TMP/bin/$BINARY" 2>/dev/null || true
  if [ -d "$TMP/lib" ]; then
    xattr -cr "$TMP/lib" 2>/dev/null || true
  fi
fi

CANDIDATE_VERSION="$(EVOT_HOME="$TMP" "$TMP/bin/$BINARY" --version 2>&1)" \
  || error "Downloaded evot failed to start: $CANDIDATE_VERSION"
[ "$CANDIDATE_VERSION" = "evot v$VERSION" ] \
  || error "Downloaded version mismatch (expected evot v$VERSION, got $CANDIDATE_VERSION)"

# --- Install ---

mkdir -p "$INSTALL_DIR"
case "$(basename "$INSTALL_DIR")" in
  bin) EVOT_HOME_DIR="$(dirname "$INSTALL_DIR")" ;;
  *)   EVOT_HOME_DIR="$INSTALL_DIR" ;;
esac
LIB_DIR="$EVOT_HOME_DIR/lib"
mkdir -p "$LIB_DIR"

# Stage on the destination filesystems, then rename into place. This keeps the
# old executable usable while /update runs and always gives macOS a fresh inode.
BINARY_STAGE="$INSTALL_DIR/.evot.new.$$"
LIB_STAGE="$LIB_DIR/.evot-install.$$"
mkdir -p "$LIB_STAGE"
cp "$TMP/bin/$BINARY" "$BINARY_STAGE"
chmod +x "$BINARY_STAGE"

if [ -d "$TMP/lib" ]; then
  for f in "$TMP"/lib/*; do
    [ -f "$f" ] || continue
    cp "$f" "$LIB_STAGE/"
  done
fi

if [ "$os" = "darwin" ]; then
  xattr -cr "$BINARY_STAGE" 2>/dev/null || true
  xattr -cr "$LIB_STAGE" 2>/dev/null || true
  codesign --verify --strict "$BINARY_STAGE" >/dev/null 2>&1 \
    || error "Downloaded evot has an invalid macOS signature"
  for f in "$LIB_STAGE"/*.node; do
    [ -f "$f" ] || continue
    codesign --verify --strict "$f" >/dev/null 2>&1 \
      || error "Downloaded $(basename "$f") has an invalid macOS signature"
  done
fi

# Install bindings first and the executable last. mv performs an atomic rename
# within each destination directory instead of overwriting an existing inode.
for f in "$LIB_STAGE"/*; do
  [ -f "$f" ] || continue
  mv -f "$f" "$LIB_DIR/$(basename "$f")"
done
rmdir "$LIB_STAGE"
LIB_STAGE=""
mv -f "$BINARY_STAGE" "$INSTALL_DIR/$BINARY"
BINARY_STAGE=""

INSTALLED_VERSION="$(EVOT_HOME="$EVOT_HOME_DIR" "$INSTALL_DIR/$BINARY" --version 2>&1)" \
  || error "Installed evot failed to start: $INSTALLED_VERSION"
[ "$INSTALLED_VERSION" = "evot v$VERSION" ] \
  || error "Installed version mismatch (expected evot v$VERSION, got $INSTALLED_VERSION)"

ok "  ✓ Installed ${BINARY} to ${INSTALL_DIR}/${BINARY}"

# --- PATH guidance ---

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"
    case "$SHELL_NAME" in
      zsh)  RC="$HOME/.zshrc" ;;
      bash) RC="$HOME/.bashrc" ;;
      fish) RC="$HOME/.config/fish/config.fish" ;;
      *)    RC="$HOME/.profile" ;;
    esac

    warn "$INSTALL_DIR is not in your PATH. Run:"
    echo ""
    if [ "$SHELL_NAME" = "fish" ]; then
      echo "  set -Ux fish_user_paths $INSTALL_DIR \$fish_user_paths"
    else
      echo "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> $RC"
      echo "  source $RC"
    fi
    echo ""
    ;;
esac
