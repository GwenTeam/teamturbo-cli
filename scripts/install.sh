#!/bin/bash
# Teamturbo teamturbo-cli Installation Script for Linux/macOS
# This script downloads and installs the Teamturbo teamturbo-cli tool

set -e

echo "Installing Teamturbo teamturbo-cli..."

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux*)
        case "$ARCH" in
            x86_64)
                DOWNLOAD_URL="https://gwen.teamturbo.io/teamturbo-cli/download/teamturbo-linux-x86_64.gz"
                ;;
            aarch64|arm64)
                DOWNLOAD_URL="https://gwen.teamturbo.io/teamturbo-cli/download/teamturbo-linux-arm64.gz"
                ;;
            *)
                echo "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        INSTALL_DIR="$HOME/.local/bin"
        ;;
    Darwin*)
        case "$ARCH" in
            x86_64)
                DOWNLOAD_URL="https://gwen.teamturbo.io/teamturbo-cli/download/teamturbo-macos-x86_64.gz"
                ;;
            arm64)
                DOWNLOAD_URL="https://gwen.teamturbo.io/teamturbo-cli/download/teamturbo-macos-aarch64.gz"
                ;;
            *)
                echo "Unsupported architecture: $ARCH"
                exit 1
                ;;
        esac
        INSTALL_DIR="$HOME/.local/bin"
        ;;
    *)
        echo "Unsupported operating system: $OS"
        exit 1
        ;;
esac

echo "Detected OS: $OS $ARCH"
echo "Download URL: $DOWNLOAD_URL"

# Create installation directory
if [ ! -d "$INSTALL_DIR" ]; then
    echo "Creating installation directory: $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
fi

# Download teamturbo-cli
echo "Downloading Teamturbo teamturbo-cli..."
TMP_FILE="/tmp/teamturbo.gz"
if command -v curl &> /dev/null; then
    curl -fSL "$DOWNLOAD_URL" -o "$TMP_FILE"
elif command -v wget &> /dev/null; then
    wget -q "$DOWNLOAD_URL" -O "$TMP_FILE"
else
    echo "Error: Neither curl nor wget is available. Please install one of them."
    exit 1
fi

echo "Download completed"

# Verify download
if [ ! -f "$TMP_FILE" ]; then
    echo "Error: Download failed - file not found"
    exit 1
fi

# Check file size
FILE_SIZE=$(stat -f%z "$TMP_FILE" 2>/dev/null || stat -c%s "$TMP_FILE" 2>/dev/null)
if [ "$FILE_SIZE" -lt 1000 ]; then
    echo "Error: Downloaded file is too small (${FILE_SIZE} bytes), might be an error page"
    cat "$TMP_FILE"
    rm "$TMP_FILE"
    exit 1
fi

# Extract gzip file
echo "Extracting files..."
gunzip -c "$TMP_FILE" > "$INSTALL_DIR/teamturbo"
rm "$TMP_FILE"
echo "Extraction completed"

# Make executable
CLI_PATH="$INSTALL_DIR/teamturbo"
mkdir -p $INSTALL_DIR
chmod +x "$CLI_PATH"

# Add to PATH if not already present
# Detect shell type from $SHELL environment variable or $0
SHELL_RC=""

# First try $SHELL environment variable (most reliable)
if [ -n "$SHELL" ]; then
    CURRENT_SHELL="$(basename "$SHELL")"
else
    # Fallback to $0, strip leading dash if present
    CURRENT_SHELL="$(echo "$0" | sed 's/^-//')"
fi

echo "Detected shell: $CURRENT_SHELL"

case "$CURRENT_SHELL" in
    *bash*)
        SHELL_RC="$HOME/.bashrc"
        ;;
    *zsh*)
        SHELL_RC="$HOME/.zshrc"
        ;;
    *sh*)
        # Generic sh shell, try .profile
        SHELL_RC="$HOME/.profile"
        ;;
    *)
        # If we still can't detect, try environment variables
        if [ -n "$BASH_VERSION" ]; then
            SHELL_RC="$HOME/.bashrc"
        elif [ -n "$ZSH_VERSION" ]; then
            SHELL_RC="$HOME/.zshrc"
        else
            # Fallback to .profile
            SHELL_RC="$HOME/.profile"
        fi
        ;;
esac

echo "Using shell config file: $SHELL_RC"

if [ -n "$SHELL_RC" ]; then
    # Check if RC file exists, create if it doesn't
    if [ ! -f "$SHELL_RC" ]; then
        echo "Creating shell config file: $SHELL_RC"
        touch "$SHELL_RC"
    fi

    # Add to PATH if not already present
    if ! grep -q "$INSTALL_DIR" "$SHELL_RC"; then
        echo "Adding Teamturbo teamturbo-cli to PATH in $SHELL_RC..."
        echo "" >> "$SHELL_RC"
        echo "# Teamturbo teamturbo-cli" >> "$SHELL_RC"
        echo "export PATH=\"\$PATH:$INSTALL_DIR\"" >> "$SHELL_RC"
        echo "PATH updated. Please run: source $SHELL_RC"
    else
        echo "Teamturbo teamturbo-cli is already in PATH"
    fi

    # Check if 'tt' command exists, if not, create symlink
    TT_PATH="$INSTALL_DIR/tt"
    if ! command -v tt >/dev/null 2>&1; then
        if [ ! -e "$TT_PATH" ]; then
            echo "Creating 'tt' symlink for teamturbo..."
            ln -s "$CLI_PATH" "$TT_PATH"
            echo "Symlink 'tt' created for convenience"
        else
            echo "'tt' symlink already exists at $TT_PATH"
        fi
    else
        echo "'tt' command already exists in system, skipping symlink creation"
    fi
fi

# Save installation metadata for upgrade功能
METADATA_DIR="$HOME/.teamturbo-cli"
METADATA_FILE="$METADATA_DIR/install.json"

mkdir -p "$METADATA_DIR"

# Extract base URL from download URL
BASE_URL=$(echo "$DOWNLOAD_URL" | sed -E 's|(https?://[^/]+).*|\1|')

cat > "$METADATA_FILE" << EOF
{
  "base_url": "$BASE_URL",
  "download_url": "$DOWNLOAD_URL",
  "install_dir": "$INSTALL_DIR",
  "install_path": "$CLI_PATH",
  "os": "$OS",
  "arch": "$ARCH",
  "installed_at": "$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
}
EOF

echo "Installation metadata saved to: $METADATA_FILE"

# Verify installation
if [ -f "$CLI_PATH" ]; then
    echo ""
    echo "Installation completed successfully!"
    echo "teamturbo-cli installed at: $CLI_PATH"
    echo ""
    echo "To get started:"
    echo "  1. Reload your shell: source $SHELL_RC"
    echo "     Or restart your terminal"
    echo "  2. Run: teamturbo --version"
    echo "  3. Run: teamturbo login"
    echo "  4. Run: teamturbo upgrade (to check for updates)"
else
    echo "Installation failed: teamturbo-cli executable not found"
    exit 1
fi
