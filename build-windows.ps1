# PowerShell build script for Windows
$VERSION = "1.0.0"
$BUILD_DIR = "target\release-builds"

Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║       Building TeamTurbo CLI for Windows                 ║" -ForegroundColor Cyan
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

# Create build directory
New-Item -ItemType Directory -Force -Path $BUILD_DIR | Out-Null

# Windows x86_64 (MSVC)
Write-Host "[1/2] Building for Windows x86_64 (MSVC)..." -ForegroundColor Green
cargo build --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -eq 0) {
    Copy-Item "target\x86_64-pc-windows-msvc\release\teamturbo.exe" "$BUILD_DIR\teamturbo-windows-x86_64.exe"
    Compress-Archive -Path "$BUILD_DIR\teamturbo-windows-x86_64.exe" -DestinationPath "$BUILD_DIR\teamturbo-windows-x86_64.zip" -Force
    Write-Host "   ✓ Windows x86_64 (MSVC) complete" -ForegroundColor Green
} else {
    Write-Host "   ✗ Windows MSVC build failed" -ForegroundColor Red
}

# Windows x86_64 (GNU)
Write-Host "[2/2] Building for Windows x86_64 (GNU)..." -ForegroundColor Green
$hasGnuTarget = rustup target list | Select-String "x86_64-pc-windows-gnu.*installed"
if ($hasGnuTarget) {
    cargo build --release --target x86_64-pc-windows-gnu
    if ($LASTEXITCODE -eq 0) {
        Copy-Item "target\x86_64-pc-windows-gnu\release\teamturbo.exe" "$BUILD_DIR\teamturbo-windows-x86_64-gnu.exe"
        Compress-Archive -Path "$BUILD_DIR\teamturbo-windows-x86_64-gnu.exe" -DestinationPath "$BUILD_DIR\teamturbo-windows-x86_64-gnu.zip" -Force
        Write-Host "   ✓ Windows x86_64 (GNU) complete" -ForegroundColor Green
    } else {
        Write-Host "   ✗ Windows GNU build failed" -ForegroundColor Red
    }
} else {
    Write-Host "   ⚠ Skipping Windows GNU build (target not installed)" -ForegroundColor Yellow
    Write-Host "     Run: rustup target add x86_64-pc-windows-gnu" -ForegroundColor Gray
}

Write-Host ""
Write-Host "╔═══════════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║                   Build Complete!                         ║" -ForegroundColor Green
Write-Host "╚═══════════════════════════════════════════════════════════╝" -ForegroundColor Green
Write-Host ""

Write-Host "Build artifacts in: $BUILD_DIR" -ForegroundColor Cyan
Get-ChildItem -Path $BUILD_DIR -File

# Generate checksums
Write-Host ""
Write-Host "Generating SHA256 checksums..." -ForegroundColor Cyan
$checksumFile = Join-Path $BUILD_DIR "SHA256SUMS.txt"
Get-ChildItem -Path $BUILD_DIR -File -Filter "teamturbo-*" | ForEach-Object {
    $hash = (Get-FileHash -Path $_.FullName -Algorithm SHA256).Hash
    "$hash  $($_.Name)" | Out-File -FilePath $checksumFile -Append -Encoding utf8
}

Write-Host ""
Write-Host "✓ SHA256 checksums saved to $checksumFile" -ForegroundColor Green
Write-Host ""
Write-Host "Done! Upload these files to your release server or GitHub Releases." -ForegroundColor Green
