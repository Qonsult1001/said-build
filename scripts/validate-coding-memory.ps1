# validate-coding-memory.ps1
#
# Validates the coding-memory commands (learn-fix / recall-fix) end-to-end,
# confirming the intent-separation breakthrough holds through the real CLI:
#   - a real ADD paraphrase recalls the ADD fix (correct case)
#   - a "document the endpoint" DISTRACTOR does NOT recall the ADD fix
#   - a "rename" distractor does NOT recall it either
#
# learn-fix stores a verified fix in the Procedural pillar (outcome=success);
# recall-fix matches on action/intent residue + target nouns (no ticket numbers).
#
# Run:  powershell -File scripts/validate-coding-memory.ps1

$ErrorActionPreference = 'Stop'
$said = "G:\development\said-build\target\debug\said.exe"
if (-not (Test-Path $said)) { $said = "G:\development\said-build\target\release\said.exe" }
if (-not (Test-Path $said)) { throw "build said first" }

$work = Join-Path $env:TEMP ("said_cm_" + [System.Guid]::NewGuid().ToString('N').Substring(0,8))
New-Item -ItemType Directory -Force $work | Out-Null
$brain = Join-Path $work "cm.said"
Set-Location $work
& $said create $brain --json | Out-Null

# Learn three verified fixes (problem -> change-set). --edits-file avoids the
# PowerShell inline-JSON-quote mangling gotcha.
$fixes = @(
  @{ id='cores'; prov='#102';
     problem='Add a GET /api/cores endpoint that returns ProcessorCount, anonymous';
     edits='[{"file":"Program.cs","mode":"insert-after-text","anchor":".AllowAnonymous();","content":"app.MapGet(\"/api/cores\", () => Environment.ProcessorCount).AllowAnonymous();"}]' },
  @{ id='retry'; prov='#95';
     problem='Add a retry policy with exponential backoff to the outbound HTTP client';
     edits='[{"file":"Program.cs","mode":"insert-after-text","anchor":"AddHttpClient","content":".AddTransientHttpErrorPolicy(p => p.WaitAndRetryAsync(3, _ => TimeSpan.FromSeconds(2)));"}]' }
)
foreach ($f in $fixes) {
  $ef = Join-Path $work ("e_" + $f.id + ".json")
  Set-Content -Path $ef -Value $f.edits -Encoding utf8 -NoNewline
  $r = & $said learn-fix --path $brain --problem $f.problem --edits-file $ef --label $f.prov --json | ConvertFrom-Json
  if (-not $r.ok) { throw "learn-fix failed for $($f.id)" }
}

function Recall($q) {
  $out = & $said recall-fix --path $brain --problem $q --min-similarity 0.0 --json | ConvertFrom-Json
  if ($out.fix) {
    [pscustomobject]@{ score=[math]::Round($out.fix.score,3); prov=$out.fix.provenance; problem=$out.fix.matched_problem }
  } else { [pscustomobject]@{ score=0; prov='(none)'; problem='' } }
}

$probes = @(
  @{ class='PARAPHRASE'; expect='pr:#102'; q='Expose the CPU processor count via a new public unauthenticated GET route' },
  @{ class='PARAPHRASE'; expect='pr:#95';  q='Make the HTTP client resilient to transient failures by retrying with growing delays' },
  @{ class='DISTRACTOR'; expect='(none)';  q='Document the /api/cores endpoint in the OpenAPI spec and route table' },
  @{ class='DISTRACTOR'; expect='(none)';  q='Rename the ProcessorCount variable in the logging module for clarity' }
)

"{0,-11} {1,-7} {2,-9} {3,-9} {4}" -f 'class','score','recalled','expect','query'
"".PadRight(96,'-')
$rows = @()
foreach ($p in $probes) {
  $res = Recall $p.q
  $rows += [pscustomobject]@{ class=$p.class; score=$res.score; prov=$res.prov; expect=$p.expect }
  $qs = if ($p.q.Length -gt 46) { $p.q.Substring(0,46)+'...' } else { $p.q }
  "{0,-11} {1,-7} {2,-9} {3,-9} {4}" -f $p.class, $res.score, $res.prov, $p.expect, $qs
}
"".PadRight(96,'-')

$para = $rows | Where-Object { $_.class -eq 'PARAPHRASE' }
$dist = $rows | Where-Object { $_.class -eq 'DISTRACTOR' }
$paraRight = ($para | Where-Object { $_.prov -eq $_.expect }).Count
"PARAPHRASE recalled the CORRECT fix: $paraRight / $($para.Count)"
$paraMin = ($para | Where-Object { $_.prov -eq $_.expect } | Measure-Object score -Minimum).Minimum
$distMax = ($dist | Measure-Object score -Maximum).Maximum
"Lowest correct-paraphrase score: $paraMin"
"Highest distractor score:        $distMax"
if ($paraMin -and $paraMin -gt $distMax) {
  $mid = [math]::Round(($paraMin + $distMax)/2, 3)
  "SEPARATION: YES. Threshold ~$mid recalls real fixes and rejects distractors."
} else {
  "SEPARATION: needs review (correct paraphrases do not all clear the distractors)."
}

Set-Location $env:TEMP
Start-Sleep -Milliseconds 300
try { Remove-Item -Recurse -Force $work -ErrorAction Stop; "`ncleaned: $work" }
catch { "`n(left temp dir $work - file handle still closing)" }
