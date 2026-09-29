#!/bin/sh
set -e

# MacDebounce installer for macOS

if [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
  echo "MacDebounce installer for macOS"
  echo ""
  echo "Usage:"
  echo "  curl -fsSL https://raw.githubusercontent.com/jjangsangy/MacDebounce/main/scripts/install.sh | sh"
  echo ""
  echo "Environment variables:"
  echo "  INSTALL_DIR          Installation directory (default: /usr/local/bin)"
  echo "  MACDEBOUNCE_VERSION  Release version to install (default: latest)"
  exit 0
fi

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Error: MacDebounce is only supported on macOS." >&2
  exit 1
fi

INSTALL_DIR="${INSTALL_DIR:-${1:-/usr/local/bin}}"
REPO="jjangsangy/MacDebounce"
TAG="${MACDEBOUNCE_VERSION:-latest}"

if [ "$TAG" = "latest" ]; then
  URL="https://github.com/${REPO}/releases/latest/download/macdebounce-darwin-universal.tar.gz"
else
  case "$TAG" in
    v*) ;;
    *) TAG="v$TAG" ;;
  esac
  URL="https://github.com/${REPO}/releases/download/${TAG}/macdebounce-darwin-universal.tar.gz"
fi

TMP_DIR="$(mktemp -d -t macdebounce)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading MacDebounce (${TAG})..."
if ! curl -fsSL "$URL" -o "$TMP_DIR/macdebounce.tar.gz"; then
  echo "Error: Failed to download MacDebounce from $URL" >&2
  echo "Please verify that the release exists at https://github.com/${REPO}/releases" >&2
  exit 1
fi

tar -xzf "$TMP_DIR/macdebounce.tar.gz" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/macdebounce" "$INSTALL_DIR/macdebounce"
chmod +x "$INSTALL_DIR/macdebounce"

echo "Successfully installed macdebounce to $INSTALL_DIR/macdebounce"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "Notice: $INSTALL_DIR is not currently in your PATH."
    echo "Add it to your shell configuration (e.g. ~/.zshrc):"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
