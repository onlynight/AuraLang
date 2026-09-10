# -------------------------------------------------------------
# Source snapshot harness (Full-Aura migration - Phase 0) - Windows PowerShell
#
# Captures / compares the Rust compiler's lexer & parser output for the
# snapshot cases under tests/snapshots/cases/, storing baselines under
# tests/snapshots/baseline/.
#
# Phase 0 : the baseline is produced by the RUST compiler (reference impl)
#           and the check verifies the harness itself is stable.
# Phase 1+: pass -Compiler <path to Aura compiler> to compare the Aura
#           implementation against the same baseline.
#
# Usage:
#   scripts\snapshot.ps1 -Update                 # regenerate baselines
#   scripts\snapshot.ps1                          # check against baselines
#   scripts\snapshot.ps1 -Compiler .\build\aura-compiler.exe
#
# NOTE: ASCII-only on purpose (Windows PowerShell 5.1 code page safety).
# -------------------------------------------------------------
param(
    [switch]$Update,
    [string]$Compiler = ''
)

$ErrorActionPreference = 'Stop'

$RootDir   = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$CasesDir  = 'tests/snapshots/cases'
$BaseDir   = 'tests/snapshots/baseline'
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)

function Find-Aura {
    foreach ($candidate in @(
        'target/release/aura.exe', 'target/release/aura',
        'target/debug/aura.exe',   'target/debug/aura'
    )) {
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

if ($Compiler -ne '') {
    $Aura = $Compiler
} else {
    $Aura = Find-Aura
}
if (-not $Aura -or -not (Test-Path $Aura)) {
    Write-Host "[snapshot] ERROR: compiler binary not found (use -Compiler <path>)" -ForegroundColor Red
    exit 1
}

function Normalize([string]$s) {
    if ($null -eq $s) { return '' }
    $t = $s -replace "`r`n", "`n"
    $t = $t -replace "`r", "`n"
    return $t.TrimEnd("`n")
}

function Run-Aura([string]$kind, [string]$file) {
    $stdout = & $Aura $kind $file 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    return (Normalize (($stdout | Out-String)))
}

if (-not (Test-Path $CasesDir)) {
    Write-Host "[snapshot] ERROR: cases dir not found: $CasesDir" -ForegroundColor Red
    exit 1
}

if ($Update -and -not (Test-Path $BaseDir)) {
    New-Item -ItemType Directory -Path $BaseDir -Force | Out-Null
}

$kinds = @('tokens', 'ast')
$cases = Get-ChildItem -Path $CasesDir -Filter *.aura | Sort-Object Name
if ($cases.Count -eq 0) {
    Write-Host "[snapshot] ERROR: no snapshot cases in $CasesDir" -ForegroundColor Red
    exit 1
}

Write-Host "[snapshot] compiler: $Aura"
Write-Host "[snapshot] mode:     $(if ($Update) { 'UPDATE' } else { 'CHECK' })"
Write-Host ""

$failures = 0
$checked  = 0

foreach ($case in $cases) {
    $name = [System.IO.Path]::GetFileNameWithoutExtension($case.Name)
    foreach ($kind in $kinds) {
        $rel      = "$BaseDir/$name.$kind.txt"
        $baseline = Join-Path $RootDir ($rel -replace '/', '\')
        $actual   = Run-Aura $kind $case.FullName

        if ($null -eq $actual) {
            Write-Host ("[snapshot] FAIL  {0,-40} {1} (compiler error)" -f $rel, $kind) -ForegroundColor Red
            $failures++
            continue
        }

        if ($Update) {
            [System.IO.File]::WriteAllText($baseline, $actual + "`n", $Utf8NoBom)
            Write-Host ("[snapshot] WROTE {0}" -f $rel) -ForegroundColor Green
            continue
        }

        if (-not (Test-Path $baseline)) {
            Write-Host ("[snapshot] MISS  {0} (run with -Update)" -f $rel) -ForegroundColor Red
            $failures++
            continue
        }

        $checked++
        $expected = Normalize ([System.IO.File]::ReadAllText($baseline))
        if ($expected -eq $actual) {
            Write-Host ("[snapshot] ok    {0}" -f $rel)
        } else {
            Write-Host ("[snapshot] DIFF  {0}" -f $rel) -ForegroundColor Red
            $e = $expected -split "`n"
            $a = $actual   -split "`n"
            $max = [Math]::Max($e.Count, $a.Count)
            $shown = 0
            for ($i = 0; $i -lt $max -and $shown -lt 5; $i++) {
                $ev = if ($i -lt $e.Count) { $e[$i] } else { '<missing>' }
                $av = if ($i -lt $a.Count) { $a[$i] } else { '<missing>' }
                if ($ev -ne $av) {
                    Write-Host ("           line {0}: expected '{1}' / actual '{2}'" -f ($i + 1), $ev, $av) -ForegroundColor DarkYellow
                    $shown++
                }
            }
            $failures++
        }
    }
}

Write-Host ""
if ($Update) {
    Write-Host "[snapshot] baselines updated" -ForegroundColor Green
    exit 0
}

if ($failures -eq 0) {
    Write-Host "[snapshot] OK: $checked snapshots match" -ForegroundColor Green
    exit 0
}

Write-Host "[snapshot] FAILED: $failures snapshot(s) differ or missing" -ForegroundColor Red
exit 1
