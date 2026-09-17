param([switch]$AzureSigning)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $projectRoot
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
if ($AzureSigning) {
    foreach ($name in @('AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET')) {
        if (-not [Environment]::GetEnvironmentVariable($name)) { throw "Missing required signing credential: $name" }
    }
    if (-not (Get-Command artifact-signing-cli -ErrorAction SilentlyContinue)) { throw 'Install artifact-signing-cli 0.11.0 before an Azure-signed build.' }
}
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $signingFile = Join-Path $env:LOCALAPPDATA 'PDFWorkstation\signing\updater.key'
    if (-not (Test-Path -LiteralPath $signingFile)) { throw 'Set TAURI_SIGNING_PRIVATE_KEY to the release signing key path.' }
    $env:TAURI_SIGNING_PRIVATE_KEY = $signingFile
}
if (-not (Test-Path 'src-tauri/resources/pdfium/bin/pdfium.dll')) { & "$PSScriptRoot/setup-pdfium.ps1" }
cargo fetch --locked --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw 'Could not fetch locked dependencies for notice verification.' }
node scripts/mpl-source-archives.mjs --check
if ($LASTEXITCODE -ne 0) { throw 'Exact MPL source archives are missing or stale. Regenerate and review them before building an installer.' }
node scripts/dependency-notices.mjs --check
if ($LASTEXITCODE -ne 0) { throw 'Dependency notices are missing or stale. Regenerate and review them before building an installer.' }
if ($AzureSigning) {
    npm.cmd run tauri -- build --ci --bundles nsis --config src-tauri/tauri.azure.conf.json
} else {
    npm.cmd run tauri -- build --ci --bundles nsis
}
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed.' }
if ($AzureSigning) {
    $version = (Get-Content -LiteralPath 'src-tauri/tauri.conf.json' -Raw -Encoding UTF8 | ConvertFrom-Json).version
    $installer = "src-tauri/target/release/bundle/nsis/PDF Workstation_${version}_x64-setup.exe"
    & "$PSScriptRoot/verify-windows-signatures.ps1" -Paths @($installer)
    $extractor = Get-Command 7z -ErrorAction Stop
    $verificationDirectory = Join-Path $projectRoot ("src-tauri/target/signature-checks/" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $verificationDirectory -Force | Out-Null
    & $extractor.Source e $installer "-o$verificationDirectory" -r -y 'pdf-workstation.exe'
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract the packaged application for signature verification.' }
    $packagedApplication = Join-Path $verificationDirectory 'pdf-workstation.exe'
    if (-not (Test-Path -LiteralPath $packagedApplication -PathType Leaf)) { throw 'The installer does not contain the expected application executable.' }
    & "$PSScriptRoot/verify-windows-signatures.ps1" -Paths @($packagedApplication)
}
node scripts/release-manifest.mjs
if ($LASTEXITCODE -ne 0) { throw 'Update manifest generation failed.' }
