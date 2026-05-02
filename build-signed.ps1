# Butterlog Signed Build Script
# This script sets the necessary environment variables and runs the production build.

# Load the signing key
$keyPath = "C:\Users\arnau\butterlog.key"
if (Test-Path $keyPath) {
    Write-Host "Loading signing key from $keyPath..." -ForegroundColor Cyan
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $keyPath -Raw).Trim()
} else {
    Write-Error "Signing key not found at $keyPath. Build will proceed without signing (updater will not work)."
}

# Run the build
Write-Host "Starting Tauri production build..." -ForegroundColor Green
npm run tauri build
