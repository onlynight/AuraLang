# -------------------------------------------------------------
# P2.4 differential test harness: Rust VM vs Aura VM
#
# For each sample in tests/p2_diff/*.aura:
#   1. compile to .auc  (aura.exe build <sample> --output <auc>)
#   2. run on Rust VM   (aura.exe run <sample>)
#   3. run on Aura VM   (aura.exe run examples/compiler/auc_diff_runner.aura
#                        with build/diff/current.auc = the .auc)
#   4. compare normalized stdout (drop tooling noise lines) + exit codes
#
# NOTE: ASCII-only on purpose (Windows PowerShell 5.1 code page safety).
# Usage: powershell -File scripts\diff-test.ps1
# -------------------------------------------------------------

$ErrorActionPreference = 'Continue'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir
$exe = "rust\target\release\aura.exe"
$diffDir = "build\diff"
New-Item -ItemType Directory -Force -Path $diffDir | Out-Null

function Normalize([string[]]$lines) {
    $out = @()
    foreach ($l in $lines) {
        $t = $l.TrimEnd()
        if ($t -match '^\[ok\]') { continue }
        if ($t -match '^\[vm\]') { continue }
        if ($t -match '^\[bytecode\]') { continue }
        if ($t -match '^semantic warning') { continue }
        if ($t -match '^warning') { continue }
        if ($t -eq '') { continue }
        # EXIT=<code> is runner metadata on the Aura side only
        if ($t -match '^EXIT=') { continue }
        $out += $t
    }
    return $out
}

$samples = Get-ChildItem tests\p2_diff -Filter "*.aura" | Sort-Object Name
$passed = 0
$failed = 0
$failedList = @()

foreach ($s in $samples) {
    $name = $s.BaseName
    $auc = Join-Path $diffDir ($name + ".auc")

    # 1. compile to .auc
    $buildOut = cmd /c "`"$exe`" build `"$($s.FullName)`" --output `"$auc`" 2>&1" | Out-String
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $auc)) {
        Write-Host ("DIFF {0}: BUILD FAIL" -f $name) -ForegroundColor Red
        $failed++; $failedList += $name
        continue
    }

    # 2. Rust VM side
    $rustRaw = cmd /c "`"$exe`" run `"$($s.FullName)`" 2>&1"
    $rustCode = $LASTEXITCODE
    $rustLines = Normalize $rustRaw

    # 3. Aura VM side
    Copy-Item $auc (Join-Path $diffDir "current.auc") -Force
    $auraRaw = cmd /c "`"$exe`" run examples\compiler\auc_diff_runner.aura 2>&1"
    $auraCode = $LASTEXITCODE
    $auraLines = Normalize $auraRaw

    # 4. compare
    $rustText = ($rustLines -join "`n")
    $auraText = ($auraLines -join "`n")
    if ($rustText -eq $auraText) {
        Write-Host ("DIFF {0}: PASS ({1} line(s))" -f $name, $rustLines.Count) -ForegroundColor Green
        $passed++
    } else {
        Write-Host ("DIFF {0}: FAIL" -f $name) -ForegroundColor Red
        Write-Host "  rust side:" -ForegroundColor Yellow
        $rustLines | ForEach-Object { Write-Host ("    " + $_) }
        Write-Host "  aura side:" -ForegroundColor Yellow
        $auraLines | ForEach-Object { Write-Host ("    " + $_) }
        $failed++; $failedList += $name
    }
}

Write-Host ("=== [diff] {0} PASS / {1} FAIL (of {2}) ===" -f $passed, $failed, $samples.Count) -ForegroundColor Cyan
if ($failed -gt 0) {
    $failedList | ForEach-Object { Write-Host ("  failed: " + $_) }
    exit 1
}
exit 0
