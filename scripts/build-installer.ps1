$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $signingFile = Join-Path $env:LOCALAPPDATA 'PDFWorkstation\signing\updater.key'
    if (-not (Test-Path -LiteralPath $signingFile)) { throw 'Set TAURI_SIGNING_PRIVATE_KEY to the release signing key path.' }
    $env:TAURI_SIGNING_PRIVATE_KEY = $signingFile
}
if (-not (Test-Path 'src-tauri/resources/pdfium/bin/pdfium.dll')) { & "$PSScriptRoot/setup-pdfium.ps1" }
npm.cmd run tauri -- build --ci --bundles nsis
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
node scripts/release-manifest.mjs
if ($LASTEXITCODE -ne 0) { throw 'Update manifest generation failed.' }
