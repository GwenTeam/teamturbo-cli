#!/bin/bash
set -e

# Ensure we use the system Rust toolchain (not asdf)
export PATH="$HOME/.cargo/bin:$PATH"

VERSION="1.0.0"
BUILD_DIR="target/release-builds"

echo "╔═══════════════════════════════════════════════════════════╗"
echo "║       Building TeamTurbo CLI for all platforms           ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""

# Create build directory
mkdir -p $BUILD_DIR

# macOS aarch64 (Apple Silicon)
echo "[4/6] Building for macOS aarch64..."
if rustup target list | grep -q "aarch64-apple-darwin (installed)"; then
    cargo build --release --target aarch64-apple-darwin
    cp target/aarch64-apple-darwin/release/teamturbo $BUILD_DIR/teamturbo-macos-aarch64
    strip $BUILD_DIR/teamturbo-macos-aarch64
    gzip -c $BUILD_DIR/teamturbo-macos-aarch64 > $BUILD_DIR/teamturbo-macos-aarch64.gz
    echo "   ✓ macOS aarch64 complete"
else
    echo "   ⚠ Skipping macOS aarch64 build (target not installed)"
    echo "     Run: rustup target add aarch64-apple-darwin"
fi


echo ""
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║                   Build Complete!                         ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo ""
echo "Build artifacts in: $BUILD_DIR"
ls -lh $BUILD_DIR/

# Generate checksums
echo ""
echo "Generating SHA256 checksums..."
cd $BUILD_DIR
sha256sum teamturbo-* 2>/dev/null > SHA256SUMS.txt || shasum -a 256 teamturbo-* > SHA256SUMS.txt
cd -

echo ""
echo "✓ SHA256 checksums saved to $BUILD_DIR/SHA256SUMS.txt"
echo ""
echo "Done! Upload these files to your release server or GitHub Releases."
