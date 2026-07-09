# said — one-line installer for Windows (PowerShell).
#
#   irm https://github.com/Qonsult1001/said-build/releases/latest/download/install.ps1 | iex
#
# Downloads said.exe + said-mcp.exe for Windows x64 from the latest GitHub Release, installs them to
# %LOCALAPPDATA%\Programs\said, and adds that dir to the user PATH. No build, no admin needed.
# Override the bundle with $env:SAID_BUNDLE = 'coding' (default: brain, the free tier).

$ErrorActionPreference = 'Stop'

# Copy one binary over a possibly-locked destination. Returns $true on success, $false if the
# target is still in use (so the caller can retry). Kept as a function so the copy loop stays
# free of inline try/catch (which the here-string-heavy body below doesn't parse cleanly around).
function Copy-Said-Binary($src, $dst) {
  try { Copy-Item $src $dst -Force -ErrorAction Stop; return $true }
  catch { return $false }
}

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

  # An agent (Cursor / Claude Desktop / Copilot) may have said-mcp.exe running as its MCP server,
  # which LOCKS the file on Windows and makes the copy fail with "being used by another process".
  # Stop any running said / said-mcp processes first so the update can overwrite them — this only
  # stops the SERVER PROCESS, never touches any .said brain file. Agents relaunch it on reload.
  $running = @(Get-Process said, said-mcp -ErrorAction SilentlyContinue)
  if ($running.Count -gt 0) {
    Write-Host "said: stopping $($running.Count) running said process(es) so they can be updated (your data is untouched)..."
    $running | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 800   # let the OS release the file handles
    $wasRunning = $true
  } else { $wasRunning = $false }

  foreach ($b in @('said.exe', 'said-mcp.exe')) {
    $src = Join-Path $tmp $b
    if (-not (Test-Path $src)) { continue }
    $dst = Join-Path $dest $b
    # Retry the copy a few times in case a file handle is still releasing after Stop-Process.
    # On a persistent lock (an agent that refused to stop), give a clear, actionable message
    # instead of a raw IOException.
    $copied = $false
    $n = 0
    while (-not $copied -and $n -lt 4) {
      $n++
      if (Copy-Said-Binary $src $dst) { $copied = $true } else { Start-Sleep -Milliseconds 700 }
    }
    if (-not $copied) {
      Write-Host "said: could not update $b -- it is still in use."
      Write-Host "      Fully close your AI agents (Cursor, Claude Desktop, VS Code/Copilot) and re-run this installer."
      throw "said: $b is locked by a running process; close your agents and retry."
    }
  }
  Write-Host "said: installed to $dest"
  if ($wasRunning) {
    Write-Host "said: (reload / restart your AI agents so they pick up the updated server)"
  }

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
    # Use the ABSOLUTE exe path, not the bare name: agents launch MCP servers WITHOUT the user's
    # freshly-updated PATH (and already-running agents have the old PATH), so a bare "said-mcp"
    # fails to spawn until every agent is restarted. The absolute path always resolves.
    $mcpRef = Join-Path $dest 'said-mcp.exe'
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

    # 1) Claude Code — write its user config (~/.claude.json) DIRECTLY with a proper command/args
    #    split. We do NOT use `claude mcp add`: its `-- <cmd> --path X` form rejects `--path` as an
    #    unknown option, and its single-string form ("said-mcp --path X") stuffs the WHOLE string
    #    into `command` with empty `args`, so Claude Code can't spawn it → "Failed to connect".
    #    We also do NOT use the PS ConvertFrom-Json merge here: ~/.claude.json is large and can hold
    #    duplicate project keys that make PS's parser THROW, which would wipe the whole file. node
    #    tolerates dup keys (last wins) and only touches mcpServers — so merge via node, and if node
    #    is missing, SKIP (never risk clobbering the user's Claude config).
    $cc = Join-Path $env:USERPROFILE '.claude.json'
    if ((Test-Path $cc) -or (Get-Command claude -ErrorAction SilentlyContinue)) {
      if (Get-Command node -ErrorAction SilentlyContinue) {
        Copy-Item $cc "$cc.said-bak" -Force -ErrorAction SilentlyContinue
        $node = @"
const fs=require('fs'); const f=process.env.SAID_CC;
let j={}; try{ j=JSON.parse(fs.readFileSync(f,'utf8')); }catch(e){ j={}; }
j.mcpServers=j.mcpServers||{};
j.mcpServers['said-brain']={command:process.env.SAID_MCP,args:['--path',process.env.SAID_BRAINP]};
fs.writeFileSync(f, JSON.stringify(j,null,2));
"@
        $env:SAID_CC = $cc; $env:SAID_MCP = $mcpRef; $env:SAID_BRAINP = $brain
        & node -e $node 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) { $registered += 'claude-code' }
      }
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
