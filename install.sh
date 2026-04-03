#!/bin/sh
set -e

REPO="sennadevos/kablam"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  linux)  TARGET_OS="unknown-linux-gnu" ;;
  darwin) TARGET_OS="apple-darwin" ;;
  *)      echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  TARGET_ARCH="x86_64" ;;
  aarch64|arm64) TARGET_ARCH="aarch64" ;;
  *)             echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

TARGET="${TARGET_ARCH}-${TARGET_OS}"

# Get latest release tag
LATEST=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
if [ -z "$LATEST" ]; then
  echo "Failed to fetch latest release"
  exit 1
fi

URL="https://github.com/$REPO/releases/download/$LATEST/kablam-${TARGET}.tar.gz"

echo "Installing kablam $LATEST for $TARGET..."
echo "  From: $URL"
echo "  To:   $INSTALL_DIR/kablam"

# Download and extract
mkdir -p "$INSTALL_DIR"
curl -sL "$URL" | tar xz -C "$INSTALL_DIR"
chmod +x "$INSTALL_DIR/kablam"

echo ""
echo "kablam installed to $INSTALL_DIR/kablam"

# Check if install dir is in PATH
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Note: $INSTALL_DIR is not in your PATH. Add it with:"
     echo "  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
