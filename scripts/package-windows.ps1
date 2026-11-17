# Build the Windows installer and a portable .zip into dist\.
#
#   scripts\package-windows.ps1             # release build, then package
#   scripts\package-windows.ps1 -SkipBuild
param([switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
# CurseForge needs an API key at build time; CI passes a secret, and
# locally it's read from .curseforge-key. It's never in the source.
if (-not $env:CALCITE_CURSEFORGE_KEY -and (Test-Path .curseforge-key)) {
    $env:CALCITE_CURSEFORGE_KEY = (Get-Content .curseforge-key -Raw).Trim()
}
if (-not $SkipBuild) {
    # The app and its sign-in helper (Microsoft's page in a web view).
    cargo build --release -p calcite -p calcite-login
    if ($LASTEXITCODE) { exit $LASTEXITCODE }
}
$bin = (Resolve-Path (Join-Path $target 'release\calcite.exe')).Path
$login = (Resolve-Path (Join-Path $target 'release\calcite-login.exe')).Path
New-Item -ItemType Directory -Force dist | Out-Null

# Portable: portable.txt beside the exe keeps all data in .\data.
$stage = Join-Path ([IO.Path]::GetTempPath()) 'calcite-portable'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null
Copy-Item $bin (Join-Path $stage 'calcite.exe')
Copy-Item $login (Join-Path $stage 'calcite-login.exe')
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')
Set-Content (Join-Path $stage 'portable.txt') 'Calcite keeps its data in the data folder next to calcite.exe while this file is here.'
$zip = "dist\Calcite-$version-windows-x64-portable.zip"
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip -Force
Remove-Item -Recurse -Force $stage

# Installer
$iscc = Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'
if (-not (Test-Path $iscc)) {
    choco install innosetup -y --no-progress | Out-Null
}
& $iscc /Q "/DAppVersion=$version" "/DBinary=$bin" "/DLoginBinary=$login" packaging\windows\calcite.iss
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Get-ChildItem dist | ForEach-Object { $_.FullName }
