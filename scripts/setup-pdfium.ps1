$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$resourceRoot = Join-Path $projectRoot 'src-tauri\resources'
$archivePath = Join-Path $resourceRoot 'pdfium-win-x64.tgz'
$pdfiumRoot = Join-Path $resourceRoot 'pdfium'
New-Item -ItemType Directory -Path $pdfiumRoot -Force | Out-Null
Invoke-WebRequest -Uri 'https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F7881/pdfium-win-x64.tgz' -OutFile $archivePath
tar -xzf $archivePath -C $pdfiumRoot
if ($LASTEXITCODE -ne 0) { throw 'PDFium extraction failed.' }
Write-Output 'PDFium Chromium 7881 extracted with its license notices.'
