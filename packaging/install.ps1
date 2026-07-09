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

  # ── AUTO-REGISTER said-mcp into any AI agents found (Claude Code / Desktop / Cursor / Copilot).
  #    On by default; opt out with $env:SAID_NO_CONNECT = '1'. MERGE into existing config (never
  #    clobber other servers), idempotent. Default brain: %USERPROFILE%\.said\brain.said (override
  #    with $env:SAID_BRAIN).
  if ($env:SAID_NO_CONNECT -ne '1') {
    $brain = if ($env:SAID_BRAIN) { $env:SAID_BRAIN } else { Join-Path $env:USERPROFILE '.said\brain.said' }
    New-Item -ItemType Directory -Force -Path (Split-Path $brain) | Out-Null
    $mcpRef = 'said-mcp'   # on PATH now; agents can call it by name
    $registered = @()

    # Merge {parent}.{topKey}.said-brain = {command,args} into a JSON config, preserving everything else.
    function Register-Json($cfg, $topKey, $parent) {
      New-Item -ItemType Directory -Force -Path (Split-Path $cfg) | Out-Null
      $obj = $null
      if (Test-Path $cfg) {
        Copy-Item $cfg "$cfg.said-bak" -Force -ErrorAction SilentlyContinue
        try { $obj = Get-Content $cfg -Raw | ConvertFrom-Json } catch { $obj = $null }
      }
      if ($null -eq $obj) { $obj = [pscustomobject]@{} }
      $host2 = $obj
      if ($parent) {
        if (-not $obj.PSObject.Properties[$parent]) { $obj | Add-Member -NotePropertyName $parent -NotePropertyValue ([pscustomobject]@{}) -Force }
        $host2 = $obj.$parent
      }
      if (-not $host2.PSObject.Properties[$topKey]) { $host2 | Add-Member -NotePropertyName $topKey -NotePropertyValue ([pscustomobject]@{}) -Force }
      $entry = [pscustomobject]@{ command = $mcpRef; args = @('--path', $brain) }
      if ($host2.$topKey.PSObject.Properties['said-brain']) { $host2.$topKey.'said-brain' = $entry }
      else { $host2.$topKey | Add-Member -NotePropertyName 'said-brain' -NotePropertyValue $entry -Force }
      $obj | ConvertTo-Json -Depth 20 | Set-Content -Path $cfg -Encoding utf8
      return $true
    }

    # 1) Claude Code — clean CLI, user (global) scope. The CLI's `--` passthrough is unreliable
    #    (commander parses a leading-dash arg like --path as its own option); passing the whole
    #    command+args as ONE commandOrUrl string is parsed correctly. Native exit code, not throw,
    #    signals success — so gate on $LASTEXITCODE (a failed `mcp add` does NOT raise in PowerShell).
    #    `mcp add` ERRORS if the name already exists (it won't update), so remove-then-add makes
    #    re-running the installer idempotent + quiet. If it already exists, count it as connected.
    if (Get-Command claude -ErrorAction SilentlyContinue) {
      & claude mcp remove said-brain -s user 2>$null | Out-Null   # ignore result (may not exist)
      & claude mcp add said-brain -s user "$mcpRef --path $brain" 2>$null | Out-Null
      if ($LASTEXITCODE -eq 0) { $registered += 'claude-code' }
    }
    # 2) Claude Desktop — %APPDATA%\Claude\claude_desktop_config.json, mcpServers.
    $cdesk = Join-Path $env:APPDATA 'Claude\claude_desktop_config.json'
    if ((Test-Path (Split-Path $cdesk)) -or (Get-Command claude -ErrorAction SilentlyContinue)) {
      try { Register-Json $cdesk 'mcpServers' $null | Out-Null; $registered += 'claude-desktop' } catch {}
    }
    # 3) Cursor — %USERPROFILE%\.cursor\mcp.json (global), mcpServers.
    $cur = Join-Path $env:USERPROFILE '.cursor\mcp.json'
    if ((Test-Path (Join-Path $env:USERPROFILE '.cursor')) -or (Get-Command cursor -ErrorAction SilentlyContinue)) {
      try { Register-Json $cur 'mcpServers' $null | Out-Null; $registered += 'cursor' } catch {}
    }
    # 4) GitHub Copilot (VS Code) — %APPDATA%\Code\User\settings.json, mcp.servers.
    $vs = Join-Path $env:APPDATA 'Code\User\settings.json'
    if (Test-Path $vs) {
      try { Register-Json $vs 'servers' 'mcp' | Out-Null; $registered += 'copilot(vscode)' } catch {}
    }

    Write-Host ""
    if ($registered.Count -gt 0) {
      Write-Host ("said: connected the brain to your agent(s): " + ($registered -join ', '))
      Write-Host "  brain file: $brain   (restart / reload each agent to pick it up)"
    } else {
      Write-Host "said: no AI agent auto-detected. To connect one, use the MCP config:"
      Write-Host "  command `"$mcpRef`"  args [`"--path`", `"$brain`"]"
    }
  }

  Write-Host ""
  Write-Host 'Done. Your agent can now remember things — just tell it "remember ..." and ask later.'
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
