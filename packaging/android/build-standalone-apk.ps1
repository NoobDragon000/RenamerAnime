$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$AndroidDir = $PSScriptRoot
$ApkToolsDir = "D:\apk\tools"
$BuildToolsDir = "$ApkToolsDir\build-tools"
$Javac = "$ApkToolsDir\jdk\bin\javac.exe"
$Java = "$ApkToolsDir\java.cmd"
$Aapt = "$BuildToolsDir\aapt.exe"
$D8Jar = "$BuildToolsDir\lib\d8.jar"
$Zipalign = "$ApkToolsDir\zipalign.cmd"
$Apksigner = "$ApkToolsDir\apksigner.cmd"
$FrameworkApk = "$env:LOCALAPPDATA\apktool\framework\1.apk"
$Keystore = "D:\apk\keys\debug.keystore"

foreach ($req in @($Javac, $Aapt, $D8Jar, $FrameworkApk, $Keystore)) {
    if (-not (Test-Path $req)) {
        throw "Required Android packaging dependency missing: $req"
    }
}

Write-Host "==> [1/6] Building frontend bundle (ui/dist)..."
Push-Location (Join-Path $RepoRoot "ui")
npm run build
Pop-Location

$WorkDir = Join-Path $RepoRoot "target\android-standalone"
$StubsClasses = Join-Path $WorkDir "stubs-classes"
$AppClasses = Join-Path $WorkDir "app-classes"
$AssetsDir = Join-Path $WorkDir "assets"
$ResDir = Join-Path $WorkDir "res"
$DexDir = Join-Path $WorkDir "dex"

if (Test-Path $WorkDir) { Remove-Item -Recurse -Force $WorkDir }
New-Item -ItemType Directory -Force -Path $StubsClasses, $AppClasses, "$AssetsDir\www", "$ResDir\drawable", $DexDir | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $RepoRoot "dist") | Out-Null

# Stage assets (web UI + rules config) and launcher icon resource
Copy-Item (Join-Path $RepoRoot "ui\dist\*") "$AssetsDir\www\" -Recurse -Force
Copy-Item (Join-Path $RepoRoot "rules\default_rules.json") "$AssetsDir\default_rules.json" -Force
Copy-Item (Join-Path $RepoRoot "crates\renamer-gui\icons\icon.png") "$ResDir\drawable\ic_launcher.png" -Force

Write-Host "==> [2/6] Compiling Android compile-time stubs & app Java sources..."
$StubFiles = Get-ChildItem (Join-Path $AndroidDir "stubs") -Recurse -Filter "*.java" | ForEach-Object { $_.FullName }
& $Javac --release 8 -d $StubsClasses @StubFiles
if ($LASTEXITCODE -ne 0) { throw "Stub javac compilation failed" }

$AppFiles = Get-ChildItem (Join-Path $AndroidDir "src") -Recurse -Filter "*.java" | ForEach-Object { $_.FullName }
& $Javac --release 8 -cp $StubsClasses -d $AppClasses @AppFiles
if ($LASTEXITCODE -ne 0) { throw "App javac compilation failed" }

Write-Host "==> [3/6] Compiling Java bytecode to Dalvik classes.dex with D8..."
$ClassFiles = Get-ChildItem $AppClasses -Recurse -Filter "*.class" | ForEach-Object { $_.FullName }
& $Java -cp $D8Jar com.android.tools.r8.D8 --min-api 24 --output $DexDir @ClassFiles
if ($LASTEXITCODE -ne 0) { throw "d8 dex compilation failed" }

Write-Host "==> [4/6] Packaging APK resources & assets with aapt..."
$UnsignedApk = Join-Path $WorkDir "unsigned.apk"
& $Aapt package -f `
    -M (Join-Path $AndroidDir "AndroidManifest.xml") `
    -S $ResDir `
    -A $AssetsDir `
    -I $FrameworkApk `
    -F $UnsignedApk
if ($LASTEXITCODE -ne 0) { throw "aapt package failed" }

# Store classes.dex inside UnsignedApk using JDK jar
& "$ApkToolsDir\jdk\bin\jar.exe" uf $UnsignedApk -C $DexDir classes.dex
if ($LASTEXITCODE -ne 0) { throw "jar uf classes.dex failed" }

Write-Host "==> [5/6] Aligning APK with zipalign..."
$AlignedApk = Join-Path $WorkDir "aligned.apk"
& $Zipalign -p -f 4 $UnsignedApk $AlignedApk
if ($LASTEXITCODE -ne 0) { throw "zipalign failed" }

Write-Host "==> [6/6] Signing & verifying APK with apksigner..."
$FinalApk = Join-Path $RepoRoot "dist\RenamerAnime-v2.0.0-android.apk"
if (Test-Path $FinalApk) { Remove-Item -Force $FinalApk }
& $Apksigner sign `
    --ks $Keystore `
    --ks-key-alias androiddebugkey `
    --ks-pass pass:android `
    --key-pass pass:android `
    --out $FinalApk `
    $AlignedApk
if ($LASTEXITCODE -ne 0) { throw "apksigner sign failed" }

& $Apksigner verify -v $FinalApk
if ($LASTEXITCODE -ne 0) { throw "apksigner verify failed" }

Write-Host "==> Standalone Android APK ready: $FinalApk"
Get-Item $FinalApk | Select-Object FullName, Length, LastWriteTime
