# -------------------------------------------------------------
# P4.2 Full differential regression: Rust VM vs Aura VM (D6/R14)
#
# For every runnable sample under tests/ + examples/, execute the
# SAME source on both VM backends and compare normalized stdout.
# A sample passes when both backends agree byte-for-byte, regardless
# of whether the output itself is "correct" (pre-existing capability
# gaps are tracked by baseline-test.ps1, not here).
#
# Sample sets: tests/p2_diff, tests/language-test, tests/classes,
#              tests/basics, examples/compiler
#
# Usage: powershell -File scripts\diff-test-full.ps1 [-TimeoutSec 120]
# ASCII-only on purpose (Windows PowerShell 5.1 code page safety).
# -------------------------------------------------------------

param([int]$TimeoutSec = 120)

$ErrorActionPreference = 'Continue'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$exe = $null
foreach ($c in @('seed\target\release\aura.exe', 'seed\target\debug\aura.exe', 'build\bin\aura.exe', 'aura\seed\aura.exe')) {
    if (Test-Path $c) { $exe = (Resolve-Path $c).Path; break }
}
if (-not $exe) {
    Write-Host '[difffull] ERROR: no seed compiler found' -ForegroundColor Red
    exit 1
}

$Dirs = @('tests\p2_diff', 'tests\language-test', 'tests\classes', 'tests\basics', 'examples\compiler')

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
        $out += $t
    }
    return $out
}

function RunWithTimeout([string]$cmd, [string[]]$argList, [int]$timeoutMs) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $cmd
    # PS 5.1 (.NET Framework) has no ProcessStartInfo.ArgumentList; quote manually
    $quoted = $argList | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }
    $psi.Arguments = ($quoted -join ' ')
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEndAsync()
    $se = $p.StandardError.ReadToEndAsync()
    if (-not $p.WaitForExit($timeoutMs)) {
        try { $p.Kill() } catch {}
        return @{ Code = -999; Out = @('<TIMEOUT>') }
    }
    $all = ($so.Result + $se.Result) -split "`r?`n"
    return @{ Code = $p.ExitCode; Out = $all }
}

$passed = 0; $failed = 0; $skipped = 0; $total = 0
$failedList = @()

foreach ($d in $Dirs) {
    if (-not (Test-Path $d)) { continue }
    $samples = Get-ChildItem $d -Filter '*.aura' -File | Sort-Object Name
    foreach ($s in $samples) {
        $total++
        $name = "$d/$($s.BaseName)"

        $rust = RunWithTimeout $exe @('run', $s.FullName) ($TimeoutSec * 1000)
        $aura = RunWithTimeout $exe @('run', $s.FullName, '--vm=aura') ($TimeoutSec * 2000)

        $rustText = (Normalize $rust.Out) -join "`n"
        $auraText = (Normalize $aura.Out) -join "`n"

        # both sides empty -> pre-existing failure on both backends; skip
        if (($rustText -eq '') -and ($auraText -eq '')) {
            $skipped++
            continue
        }
        if ($rustText -eq $auraText) {
            $passed++
        } else {
            $failed++; $failedList += $name
            Write-Host ("DIFFFULL {0}: MISMATCH" -f $name) -ForegroundColor Red
            $rLines = $rustText -split "`n"
            $aLines = $auraText -split "`n"
            for ($i = 0; $i -lt [Math]::Max($rLines.Count, $aLines.Count); $i++) {
                $rv = if ($i -lt $rLines.Count) { $rLines[$i] } else { '<none>' }
                $av = if ($i -lt $aLines.Count) { $aLines[$i] } else { '<none>' }
                if ($rv -ne $av) {
                    Write-Host ("    rust: {0}" -f $rv) -ForegroundColor Yellow
                    Write-Host ("    aura: {0}" -f $av) -ForegroundColor Yellow
                    break
                }
            }
        }
    }
}

Write-Host ("=== [difffull] {0} MATCH / {1} MISMATCH / {2} both-fail-skip (of {3}) ===" -f $passed, $failed, $skipped, $total) -ForegroundColor Cyan
if ($failed -gt 0) {
    $failedList | ForEach-Object { Write-Host ("  mismatch: " + $_) }
    exit 1
}
exit 0
