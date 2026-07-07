# said — one-line installer for Windows (PowerShell).
#
#   irm https://github.com/Qonsult1001/said-build/releases/latest/download/install.ps1 | iex
#
# Downloads said.exe + said-mcp.exe for Windows x64 from the latest GitHub Release, installs them to
# %LOCALAPPDATA%\Programs\said, and adds that dir to the user PATH. No build, no admin needed.
# Override the bundle with $env:SAID_BUNDLE = 'coding' (default: brain, the free tier).

$ErrorActionPreference = 'Stop'

$repo   = 'Qonsult1001/said-build'
$bundle = if ($env:SAID_BUNDLE) { $env:SAID_BUNDLE } else { 'brain' }
$asset  = "said-$bundle-windows-x64.zip"
$url    = "https://github.com/$repo/releases/latest/download/$asset"

$dest = Join-Path $env:LOCALAPPDATA 'Programs\said'
New-Item -ItemType Directory -Force -Path $dest | Out-Null

$tmp = Join-Path $env:TEMP ("said-" + [guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
  Write-Host "said: downloading $asset ..."
  $zip = Join-Path $tmp 'said.zip'
  Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing

  Write-Host "said: extracting ..."
  Expand-Archive -Path $zip -DestinationPath $tmp -Force

  foreach ($b in @('said.exe', 'said-mcp.exe')) {
    $src = Join-Path $tmp $b
    if (Test-Path $src) { Copy-Item $src (Join-Path $dest $b) -Force }
  }
  Write-Host "said: installed to $dest"

  # Add to the USER PATH (persistent) if not already there.
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if ($userPath -notlike "*$dest*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$dest", 'User')
    Write-Host "said: added $dest to your PATH (reopen your terminal to use `said`)"
  }

  & (Join-Path $dest 'said.exe') --version
  Write-Host ""
  Write-Host 'Done. Try:  said create my-brain.said ; said --path my-brain.said add "my first note"'
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
