$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$CargoBin = "$env:USERPROFILE\.cargo\bin"
$W64Bin = "$env:USERPROFILE\.local\w64devkit\bin"
$env:PATH = "$CargoBin;$W64Bin;$env:SystemRoot\system32;$env:SystemRoot;$env:SystemRoot\System32\Wbem;$env:PATH"

Write-Host "Building frontend (ui/dist)..."
Push-Location (Join-Path $RepoRoot "ui")
npm run build
Pop-Location

Write-Host "Building release binaries (renamer-cli + renamer-gui)..."
Push-Location $RepoRoot
cargo build --release --workspace
Pop-Location

$StageDir = Join-Path $RepoRoot "dist\RenamerAnime-v2.0.0-windows-x64"
$ZipPath = Join-Path $RepoRoot "dist\RenamerAnime-v2.0.0-windows-x64.zip"

if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
New-Item -ItemType Directory -Force -Path (Join-Path $StageDir "rules") | Out-Null

Copy-Item (Join-Path $RepoRoot "target\release\renamer-gui.exe") $StageDir
Copy-Item (Join-Path $RepoRoot "target\release\renamer-cli.exe") $StageDir
Copy-Item (Join-Path $RepoRoot "anime.sh") $StageDir
Copy-Item (Join-Path $RepoRoot "README.md") $StageDir
Copy-Item (Join-Path $RepoRoot "rules\default_rules.json") (Join-Path $StageDir "rules\")

Compress-Archive -Path "$StageDir\*" -DestinationPath $ZipPath -Force
Write-Host "Packaged Windows release bundle at: $ZipPath"
Get-Item $ZipPath | Select-Object FullName, Length, LastWriteTime
