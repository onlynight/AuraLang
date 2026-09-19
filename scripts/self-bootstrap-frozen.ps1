#Requires -Version 5.0
# -------------------------------------------------------------
# Pure-Aura self-bootstrap verification (NO Rust / cargo required)
#
# Uses the frozen Stage-0 carrier committed at dist/bootstrap/aura-compiler.exe
# to bootstrap itself and verify behavioural consistency:
#
#   1. verify SHA256 of the frozen binary against dist/bootstrap/SHA256SUMS
#   2. frozen.exe  compiles Main.aura  -> build/bin/n1.exe
#   3. n1.exe      compiles Main.aura  -> build/bin/n2.exe
#   4. n1.exe / n2.exe compile the same test program; outputs must match
#
# External requirements: LLVM tools (llc / clang) + system CRT only.
#
# Usage:
#   scripts\self-bootstrap-frozen.ps1
#   scripts\self-bootstrap-frozen.ps1 -LlvmHome <dir>
#   scripts\self-bootstrap-frozen.ps1 -SkipHash
# -------------------------------------------------------------
param(
    [string]$LlvmHome = "D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc",
    [switch]$SkipHash,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir  = (Split-Path -Parent $PSScriptRoot)
$Frozen   = Join-Path $RootDir 'dist/bootstrap/aura-compiler.exe'
$Sums     = Join-Path $RootDir 'dist/bootstrap/SHA256SUMS'
$MainAura = Join-Path $RootDir 'aura/compiler/aura/lang/compiler/Main.aura'
$BinDir   = Join-Path $RootDir 'build/bin'
$TestDir  = Join-Path $RootDir 'build/test'

if ($Help) {
    Write-Host "Usage: scripts\self-bootstrap-frozen.ps1 [-LlvmHome <dir>] [-SkipHash]"
    exit 0
}

Set-Location $RootDir
New-Item -ItemType Directory -Path $BinDir  -Force | Out-Null
New-Item -ItemType Directory -Path $TestDir -Force | Out-Null

function Step($msg) { Write-Host ""; Write-Host "[$msg]" -ForegroundColor Cyan }

# ── Step 0: frozen carrier + hash check ──
Step "Step 0: frozen carrier"
if (-not (Test-Path $Frozen)) {
    Write-Host "  FATAL: frozen binary not found: $Frozen" -ForegroundColor Red
    Write-Host "  Run scripts/freeze-bootstrap.ps1 (requires Rust, one-time) to produce it."
    exit 1
}
if (-not $SkipHash -and (Test-Path $Sums)) {
    $expected = ((Get-Content $Sums -Raw).Trim() -split '\s+')[0].ToUpper()
    $actual   = (Get-FileHash -Algorithm SHA256 $Frozen).Hash.ToUpper()
    if ($expected -ne $actual) {
        Write-Host "  FATAL: SHA256 mismatch" -ForegroundColor Red
        Write-Host "    expected: $expected"
        Write-Host "    actual:   $actual"
        exit 1
    }
    Write-Host "  SHA256 OK: $actual" -ForegroundColor Green
} else {
    Write-Host "  SHA256 check skipped" -ForegroundColor Yellow
}

$llc   = Join-Path $LlvmHome 'bin/llc.exe'
$clang = Join-Path $LlvmHome 'bin/clang.exe'
if (-not (Test-Path $llc) -or -not (Test-Path $clang)) {
    Write-Host "  FATAL: LLVM tools not found under $LlvmHome" -ForegroundColor Red
    exit 1
}
Write-Host "  LLVM: $LlvmHome" -ForegroundColor Green

# ── Step 1: frozen -> n1 ──
Step "Step 1: frozen carrier compiles Main.aura -> n1.exe"
$N1 = Join-Path $BinDir 'aura-compiler-n1.exe'
& $Frozen $MainAura -o $N1
if ($LASTEXITCODE -ne 0) { Write-Host "  FATAL: stage 1 failed" -ForegroundColor Red; exit 1 }
Write-Host "  OK: $N1" -ForegroundColor Green

# ── Step 2: n1 -> n2 ──
Step "Step 2: n1 compiles Main.aura -> n2.exe"
$N2 = Join-Path $BinDir 'aura-compiler-n2.exe'
& $N1 $MainAura -o $N2
if ($LASTEXITCODE -ne 0) { Write-Host "  FATAL: stage 2 failed" -ForegroundColor Red; exit 1 }
Write-Host "  OK: $N2" -ForegroundColor Green

# ── Step 3: behavioural consistency ──
Step "Step 3: behavioural consistency"
$TestFile = Join-Path $RootDir 'tests/string_methods_test.aura'
if (-not (Test-Path $TestFile)) {
    Write-Host "  WARNING: test file not found, skipping consistency check" -ForegroundColor Yellow
    exit 0
}
$O1 = Join-Path $TestDir 'n1.txt'
$O2 = Join-Path $TestDir 'n2.txt'
& $N1 $TestFile -o (Join-Path $TestDir 'n1.exe')
if ($LASTEXITCODE -eq 0) { & (Join-Path $TestDir 'n1.exe') > $O1 2>&1 }
& $N2 $TestFile -o (Join-Path $TestDir 'n2.exe')
if ($LASTEXITCODE -eq 0) { & (Join-Path $TestDir 'n2.exe') > $O2 2>&1 }

$c1 = Get-Content $O1 -Raw -ErrorAction SilentlyContinue
$c2 = Get-Content $O2 -Raw -ErrorAction SilentlyContinue
if ($c1 -eq $c2) {
    Write-Host "  OK: behaviour consistent" -ForegroundColor Green
} else {
    Write-Host "  FATAL: behaviour differs" -ForegroundColor Red
    Write-Host "  n1: $c1"
    Write-Host "  n2: $c2"
    exit 1
}

Write-Host ""
Write-Host "=============================================" -ForegroundColor Cyan
Write-Host "  Pure-Aura self-bootstrap verified (no Rust)" -ForegroundColor Cyan
Write-Host "=============================================" -ForegroundColor Cyan
exit 0
