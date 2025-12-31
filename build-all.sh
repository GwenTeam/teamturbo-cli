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

# Linux x86_64 (GNU)
echo "[1/6] Building for Linux x86_64 (GNU)..."
cargo build --release --target x86_64-unknown-linux-gnu
cp target/x86_64-unknown-linux-gnu/release/teamturbo $BUILD_DIR/teamturbo-linux-x86_64
strip $BUILD_DIR/teamturbo-linux-x86_64
gzip -c $BUILD_DIR/teamturbo-linux-x86_64 > $BUILD_DIR/teamturbo-linux-x86_64.gz
echo "   ✓ Linux x86_64 (GNU) complete"

# Linux x86_64 (musl) - Static linking for better compatibility
echo "[2/6] Building for Linux x86_64 (musl)..."
if rustup target list | grep -q "x86_64-unknown-linux-musl (installed)"; then
    cargo build --release --target x86_64-unknown-linux-musl
    cp target/x86_64-unknown-linux-musl/release/teamturbo $BUILD_DIR/teamturbo-linux-x86_64-musl
    strip $BUILD_DIR/teamturbo-linux-x86_64-musl
    gzip -c $BUILD_DIR/teamturbo-linux-x86_64-musl > $BUILD_DIR/teamturbo-linux-x86_64-musl.gz
    echo "   ✓ Linux x86_64 (musl) complete"
else
    echo "   ⚠ Skipping Linux musl build (target not installed)"
    echo "     Run: rustup target add x86_64-unknown-linux-musl"
fi

# macOS x86_64 (Intel)
echo "[3/6] Building for macOS x86_64..."
if rustup target list | grep -q "x86_64-apple-darwin (installed)"; then
    cargo build --release --target x86_64-apple-darwin
    cp target/x86_64-apple-darwin/release/teamturbo $BUILD_DIR/teamturbo-macos-x86_64
    strip $BUILD_DIR/teamturbo-macos-x86_64
    gzip -c $BUILD_DIR/teamturbo-macos-x86_64 > $BUILD_DIR/teamturbo-macos-x86_64.gz
    echo "   ✓ macOS x86_64 complete"
else
    echo "   ⚠ Skipping macOS x86_64 build (target not installed)"
    echo "     Run: rustup target add x86_64-apple-darwin"
fi

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

# Windows x86_64 (GNU)
echo "[5/6] Building for Windows x86_64 (GNU)..."
if rustup target list | grep -q "x86_64-pc-windows-gnu (installed)"; then
    cargo build --release --target x86_64-pc-windows-gnu
    cp target/x86_64-pc-windows-gnu/release/teamturbo.exe $BUILD_DIR/teamturbo-windows-x86_64-gnu.exe
    strip $BUILD_DIR/teamturbo-windows-x86_64-gnu.exe || true
    cd $BUILD_DIR && zip -q teamturbo-windows-x86_64-gnu.zip teamturbo-windows-x86_64-gnu.exe && cd -
    echo "   ✓ Windows x86_64 (GNU) complete"
else
    echo "   ⚠ Skipping Windows GNU build (target not installed)"
    echo "     Run: rustup target add x86_64-pc-windows-gnu"
fi

# Windows x86_64 (MSVC) - Requires cargo-xwin or Windows environment
echo "[6/6] Building for Windows x86_64 (MSVC)..."
if command -v cargo-xwin &> /dev/null; then
    cargo xwin build --release --target x86_64-pc-windows-msvc
    cp target/x86_64-pc-windows-msvc/release/teamturbo.exe $BUILD_DIR/teamturbo-windows-x86_64.exe
    cd $BUILD_DIR && zip -q teamturbo-windows-x86_64.zip teamturbo-windows-x86_64.exe && cd -
    echo "   ✓ Windows x86_64 (MSVC) complete"
else
    echo "   ⚠ Skipping Windows MSVC build (cargo-xwin not found)"
    echo "     Install: cargo install cargo-xwin"
    echo "     Or build on Windows with MSVC toolchain"
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

scp target/release-builds/*.gz raisethink@10.99.100.9:/home/raisethink/teamturbo-cli/download/

