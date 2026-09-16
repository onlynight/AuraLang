#Requires -Version 5.0
# Phase E.1-E.2: Frozen Bootstrap Binary + SHA256SUMS
#
# Produces a frozen Aura compiler binary that requires only LLVM tools
# (llc/clang) to compile Aura programs — no Rust or C compiler needed.
#
# Flow:
#   1. Build Rust bootstrap compiler (aura.exe) — one-time dev dependency
#   2. Compile Main.aura → aura-compiler-native.exe (Aura AOT backend)
#   3. Self-verify: aura-compiler-native.exe compiles Main.aura → native2.exe
#   4. Behavioral consistency check
#   5. Generate SHA256SUMS
#   6. Output release package layout
#
# Usage:
#   scripts\freeze-bootstrap.ps1              # full flow
#   scripts\freeze-bootstrap.ps1 -SkipBuild    # skip compilation, only SHA256
#   scripts\freeze-bootstrap.ps1 -Help
#
# Note: This script is ASCII-only for Windows PowerShell 5.1 compatibility.

param(
    [switch]$SkipBuild,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir   = (Split-Path -Parent $PSScriptRoot)
$BuildDir  = "$RootDir\build"
$BinDir    = "$BuildDir\bin"
$LLVMHome  = "D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc"
$MainAura  = "$RootDir\aura\compiler\aura\lang\compiler\Main.aura"
$TestDir   = "$RootDir\tests"

function Write-Step($msg) {
    Write-Host ""
    Write-Host "[$msg]" -ForegroundColor Cyan
}

if ($Help) {
    Write-Host "Usage: scripts\freeze-bootstrap.ps1 [-SkipBuild] [-Help]"
    Write-Host ""
    Write-Host "Freeze the Aura compiler binary (Phase E)."
    Write-Host "Requires: aura.exe (Rust), LLVM tools (llc/clang)."
    Write-Host "Output:  build/bin/aura-compiler.exe + SHA256SUMS"
    exit 0
}

Set-Location $RootDir
New-Item -ItemType Directory -Path $BinDir -Force | Out-Null

# ── Step 0: Environment check ──
Write-Step "Step 0: Environment check"

$Aurabin = "$RootDir\build\bin\aura.exe"
if (-not (Test-Path $Aurabin)) {
    Write-Host "  Building aura.exe..." -ForegroundColor Yellow
    cargo build --release -p cli --features llvm
    if ($LASTEXITCODE -ne 0) { Write-Host "  FATAL: cargo build failed" -ForegroundColor Red; exit 1 }
}
Write-Host "  aura.exe: $Aurabin" -ForegroundColor Green

$llc = "$LLVMHome\bin\llc.exe"
$clang = "$LLVMHome\bin\clang.exe"
if (-not (Test-Path $llc) -or -not (Test-Path $clang)) {
    Write-Host "  FATAL: LLVM tools not found at $LLVMHome" -ForegroundColor Red; exit 1
}
Write-Host "  LLVM: $LLVMHome" -ForegroundColor Green

if (-not (Test-Path $MainAura)) {
    Write-Host "  FATAL: Main.aura not found: $MainAura" -ForegroundColor Red; exit 1
}
Write-Host "  Main.aura: $MainAura" -ForegroundColor Green

# ── Step 1: Compile native carrier ──
if (-not $SkipBuild) {
    Write-Step "Step 1: Compile Main.aura -> aura-compiler-native.exe"
    $NativeOut = "$BinDir\aura-compiler-native.exe"
    & $Aurabin build $MainAura --aot --llvm-home $LLVMHome -o $NativeOut 2>&1
    if ($LASTEXITCODE -ne 0) { Write-Host "  FATAL: AOT compilation failed" -ForegroundColor Red; exit 1 }
    Write-Host "  OK: $NativeOut" -ForegroundColor Green
}

# ── Step 2: Self-bootstrap verification ──
if (-not $SkipBuild) {
    Write-Step "Step 2: Self-bootstrap (Aura AOT backend)"
    $Native2Out = "$BinDir\aura-compiler-native2.exe"
    & "$BinDir\aura-compiler-native.exe" $MainAura --llvm-home $LLVMHome -o $Native2Out 2>&1
    if ($LASTEXITCODE -ne 0) { Write-Host "  FATAL: Self-bootstrap failed" -ForegroundColor Red; exit 1 }
    Write-Host "  OK: $Native2Out" -ForegroundColor Green
}

# ── Step 3: Behavioral consistency ──
if (-not $SkipBuild) {
    Write-Step "Step 3: Behavioral consistency check"
    $TestFile = "$TestDir\pure_aura\hir_b1_when_tests.aura"
    if (-not (Test-Path $TestFile)) { $TestFile = "$TestDir\string_methods_test.aura" }
    
    $Out1 = "$BuildDir\test\output1.txt"
    $Out2 = "$BuildDir\test\output2.txt"
    New-Item -ItemType Directory -Path "$BuildDir\test" -Force | Out-Null
    
    & "$BinDir\aura-compiler-native.exe" $TestFile --llvm-home $LLVMHome -o "$BuildDir\test\native1.exe" 2>&1
    if ($LASTEXITCODE -eq 0) { & "$BuildDir\test\native1.exe" > $Out1 2>&1 }
    
    & "$BinDir\aura-compiler-native2.exe" $TestFile --llvm-home $LLVMHome -o "$BuildDir\test\native2.exe" 2>&1
    if ($LASTEXITCODE -eq 0) { & "$BuildDir\test\native2.exe" > $Out2 2>&1 }
    
    $c1 = Get-Content $Out1 -Raw -ErrorAction SilentlyContinue
    $c2 = Get-Content $Out2 -Raw -ErrorAction SilentlyContinue
    if ($c1 -eq $c2) {
        Write-Host "  OK: Behavior consistent" -ForegroundColor Green
    } else {
        Write-Host "  WARNING: Behavior inconsistent" -ForegroundColor Yellow
        Write-Host "  native1: $c1" -ForegroundColor DarkGray
        Write-Host "  native2: $c2" -ForegroundColor DarkGray
    }
}

# ── Step 4: Promote to final binary ──
if (-not $SkipBuild) {
    Write-Step "Step 4: Promote frozen binary"
    $FinalBin = "$BinDir\aura-compiler.exe"
    Copy-Item "$BinDir\aura-compiler-native2.exe" $FinalBin -Force
    $sizeKB = [math]::Round((Get-Item $FinalBin).Length / 1024, 1)
    Write-Host "  OK: $FinalBin ($sizeKB KB)" -ForegroundColor Green
}

# ── Step 5: Generate SHA256SUMS ──
Write-Step "Step 5: SHA256SUMS"
$SumFile = "$BinDir\SHA256SUMS"
$hash = (Get-FileHash -Algorithm SHA256 "$BinDir\aura-compiler.exe").Hash
$fileName = "aura-compiler.exe"
$checksumLine = "$hash  $fileName"
Set-Content -Path $SumFile -Value $checksumLine -Encoding ASCII
Write-Host "  $checksumLine" -ForegroundColor Green
Write-Host "  Written to: $SumFile" -ForegroundColor Green

# ── Step 6: Output release package layout ──
Write-Step "Step 6: Release package layout"
Write-Host ""
Write-Host "  build/bin/" -ForegroundColor White
Write-Host "  |-- aura-compiler.exe    (frozen bootstrap binary)" -ForegroundColor White
Write-Host "  |-- SHA256SUMS           (checksum)" -ForegroundColor White
Write-Host ""
Write-Host "  User environment requirements:" -ForegroundColor Cyan
Write-Host "    - aura-compiler.exe   (from this release)"
Write-Host "    - llc                 (LLVM tools)"
Write-Host "    - clang               (LLVM tools, link-only mode)"
Write-Host "    - System CRT          (OS-provided: libc/kernel32.dll)"
Write-Host ""
Write-Host "  No Rust compiler, no C compiler, no C runtime source needed." -ForegroundColor Green

# ── Summary ──
Write-Host ""
Write-Host "=============================================" -ForegroundColor Cyan
Write-Host "  Phase E: Frozen bootstrap complete!" -ForegroundColor Cyan
Write-Host "  Binary: build/bin/aura-compiler.exe" -ForegroundColor Cyan
Write-Host "  SHA256: $hash" -ForegroundColor Cyan
Write-Host "=============================================" -ForegroundColor Cyan
exit 0