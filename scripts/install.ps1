# Teamturbo CLI Installation Script for Windows
# This script downloads and installs the Teamturbo CLI tool

$ErrorActionPreference = "Stop"

Write-Host "Installing Teamturbo CLI..." -ForegroundColor Green

# Configuration
$CLI_URL = "https://gwen.teamturbo.io/teamturbo-cli/download/teamturbo-windows-x86_64.zip"
$INSTALL_DIR = "$env:LOCALAPPDATA\Teamturbo"
$CLI_PATH = "$INSTALL_DIR\teamturbo.exe"

# Create installation directory
if (-not (Test-Path $INSTALL_DIR)) {
    Write-Host "Creating installation directory: $INSTALL_DIR"
    New-Item -ItemType Directory -Path $INSTALL_DIR -Force | Out-Null
}

# Download CLI
Write-Host "Downloading Teamturbo CLI..."
$ZIP_PATH = "$env:TEMP\teamturbo.zip"
Invoke-WebRequest -Uri $CLI_URL -OutFile $ZIP_PATH -UseBasicParsing
Write-Host "Download completed" -ForegroundColor Green

# Extract ZIP
Write-Host "Extracting files..."
Expand-Archive -Path $ZIP_PATH -DestinationPath $INSTALL_DIR -Force
Write-Host "Extraction completed" -ForegroundColor Green

# Rename the extracted file to teamturbo.exe
$ExtractedFile = "$INSTALL_DIR\teamturbo-windows-x86_64.exe"
if (Test-Path $ExtractedFile) {
    Move-Item -Path $ExtractedFile -Destination $CLI_PATH -Force
}

Remove-Item $ZIP_PATH

# Add to PATH
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($CurrentPath -notlike "*$INSTALL_DIR*") {
    Write-Host "Adding Teamturbo CLI to PATH..."
    [Environment]::SetEnvironmentVariable(
        "Path",
        "$CurrentPath;$INSTALL_DIR",
        "User"
    )
    Write-Host "PATH updated" -ForegroundColor Green
} else {
    Write-Host "Teamturbo CLI is already in PATH" -ForegroundColor Green
}

# Create 'tt' copy if it doesn't exist
$TT_PATH = "$INSTALL_DIR\tt.exe"
$ttExists = Get-Command tt -ErrorAction SilentlyContinue
if (-not $ttExists) {
    if (-not (Test-Path $TT_PATH)) {
        Copy-Item -Path $CLI_PATH -Destination $TT_PATH -Force
        Write-Host "'tt' shortcut created" -ForegroundColor Green
    }
}

# Save installation metadata for upgrade功能
$METADATA_DIR = "$env:USERPROFILE\.teamturbo-cli"
$METADATA_FILE = "$METADATA_DIR\install.json"

if (-not (Test-Path $METADATA_DIR)) {
    New-Item -ItemType Directory -Path $METADATA_DIR -Force | Out-Null
}

# Extract base URL from download URL
$BASE_URL = ([System.Uri]$CLI_URL).GetLeftPart([System.UriPartial]::Authority)

$metadata = @{
    base_url = $BASE_URL
    download_url = $CLI_URL
    install_dir = $INSTALL_DIR
    install_path = $CLI_PATH
    tt_path = $TT_PATH
    os = "Windows"
    arch = "x86_64"
    installed_at = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
} | ConvertTo-Json

$metadata | Out-File -FilePath $METADATA_FILE -Encoding UTF8
Write-Host "Installation metadata saved to: $METADATA_FILE" -ForegroundColor Green

# Verify installation
if (Test-Path $CLI_PATH) {
    Write-Host "`nInstallation completed successfully!" -ForegroundColor Green
    Write-Host "CLI installed at: $CLI_PATH" -ForegroundColor Cyan
    Write-Host "`nTo get started:" -ForegroundColor Yellow
    Write-Host "  1. Restart your terminal"
    Write-Host "  2. Run: teamturbo --version"
    Write-Host "  3. Run: teamturbo login"
    Write-Host "  4. Run: teamturbo upgrade (to check for updates)"
} else {
    Write-Host "`nInstallation failed: CLI executable not found" -ForegroundColor Red
    exit 1
}
