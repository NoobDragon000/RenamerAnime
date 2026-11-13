$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")

if (-not $env:ANDROID_HOME -and -not $env:ANDROID_SDK_ROOT) {
    throw "ANDROID_HOME or ANDROID_SDK_ROOT is not set. Install Android SDK & NDK (API 34+) or run via GitHub Actions workflow (.github/workflows/release.yml)."
}

if (-not $env:NDK_HOME) {
    $SdkRoot = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { $env:ANDROID_SDK_ROOT }
    $NdkCandidates = Get-ChildItem (Join-Path $SdkRoot "ndk") -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending
    if ($NdkCandidates.Count -gt 0) {
        $env:NDK_HOME = $NdkCandidates[0].FullName
    } else {
        throw "Android NDK not found under $SdkRoot\ndk. Set NDK_HOME explicitly."
    }
}

Write-Host "Building frontend (ui/dist)..."
Push-Location (Join-Path $RepoRoot "ui")
npm run build
Pop-Location

$JniDir = Join-Path $RepoRoot "crates\renamer-gui\gen\android\app\src\main\jniLibs"
New-Item -ItemType Directory -Force -Path $JniDir | Out-Null

Write-Host "Cross-compiling Rust core + Tauri lib via cargo-ndk..."
Push-Location (Join-Path $RepoRoot "crates\renamer-gui")
cargo ndk -t arm64-v8a -t x86_64 -o $JniDir rustc --release --lib -- --crate-type cdylib
Pop-Location

Write-Host "Assembling APK via Tauri Android CLI..."
Push-Location $RepoRoot
cargo tauri android build --apk
Pop-Location
