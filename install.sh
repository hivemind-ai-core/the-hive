#!/bin/sh
set -eu

REPO="hivemind-ai/the-hive"
INSTALL_DIR="${HIVE_HOME:-$HOME/.hive}"
VERSION="${1:-}"  # optional version arg, defaults to latest

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  os="unknown-linux-gnu" ;;
  Darwin) os="apple-darwin" ;;
  *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
  x86_64)        arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${arch}-${os}"

# Fetch latest version if not specified
if [ -z "$VERSION" ]; then
  echo "Fetching latest version..."
  VERSION=$(curl -sf "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"v\?\([^"]*\)".*/\1/')
  if [ -z "$VERSION" ]; then
    echo "Failed to fetch latest version" >&2
    exit 1
  fi
fi

echo "Installing hive v$VERSION for $TARGET..."

# Download tarball
TARBALL="hive-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/$REPO/releases/download/v${VERSION}/$TARBALL"
TMP=$(mktemp -d)

echo "Downloading $URL..."
curl -fSL "$URL" -o "$TMP/$TARBALL"

# Extract
tar -xzf "$TMP/$TARBALL" -C "$TMP"
EXTRACTED="$TMP/hive-${VERSION}-${TARGET}"

# Install
mkdir -p "$INSTALL_DIR/bin" "$INSTALL_DIR/docker"

cp "$EXTRACTED/hive"         "$INSTALL_DIR/bin/hive"
cp "$EXTRACTED/hive-server"  "$INSTALL_DIR/bin/hive-server"
cp "$EXTRACTED/hive-agent"   "$INSTALL_DIR/bin/hive-agent"
cp "$EXTRACTED/app-daemon"   "$INSTALL_DIR/bin/app-daemon"
cp "$EXTRACTED/docker/"*     "$INSTALL_DIR/docker/"
cp "$EXTRACTED/version"      "$INSTALL_DIR/version"

chmod +x "$INSTALL_DIR/bin/"*

# Cleanup
rm -rf "$TMP"

echo "Installed hive v$VERSION to $INSTALL_DIR"
echo ""

# PATH instructions
case ":$PATH:" in
  *":$INSTALL_DIR/bin:"*) ;;
  *)
    echo "Add hive to your PATH by adding this to your shell config:"
    echo "  export PATH=\"\$HOME/.hive/bin:\$PATH\""
    echo ""
    SHELL_RC=""
    case "${SHELL:-}" in
      */zsh)  SHELL_RC="~/.zshrc" ;;
      */bash) SHELL_RC="~/.bashrc" ;;
    esac
    if [ -n "$SHELL_RC" ]; then
      echo "Or run:"
      echo "  echo 'export PATH=\"\$HOME/.hive/bin:\$PATH\"' >> $SHELL_RC"
    fi
    ;;
esac

echo "Done! Run 'hive --help' to get started."
