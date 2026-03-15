$ErrorActionPreference = "Stop"

$repo = "LordCail1/codex-vision"
$binDir = if ($env:BIN_DIR) { $env:BIN_DIR } else { Join-Path $env:USERPROFILE "AppData\Local\codex-vision\bin" }

if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
  throw "Unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE"
}

$asset = "codex-vision-x86_64-pc-windows-msvc.zip"
$url = "https://github.com/$repo/releases/latest/download/$asset"
$tempDir = Join-Path $env:TEMP ("codex-vision-" + [guid]::NewGuid().ToString())
$zipPath = Join-Path $tempDir $asset

New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

try {
  Write-Host "Downloading $asset..."
  Invoke-WebRequest -Uri $url -OutFile $zipPath
  Expand-Archive -Path $zipPath -DestinationPath $tempDir -Force
  Copy-Item (Join-Path $tempDir "codex-vision.exe") (Join-Path $binDir "codex-vision.exe") -Force
}
finally {
  Remove-Item -Recurse -Force $tempDir
}

Write-Host "Installed codex-vision to $binDir\codex-vision.exe"
Write-Host "Add $binDir to PATH if needed, then run: codex-vision doctor"
