# -------------------------------------------------------------
# Build the Aura compiler (Full-Aura migration - Phase 0) - Windows PowerShell
#
# Uses the Rust compiler to compile the "Aura compiler written in Aura":
#   aura/compiler/aura/lang/compiler/Main.aura  ->  build/aura-compiler.(auc|exe)
#
# Usage:
#   scripts\build-aura-compiler.ps1            # bytecode .auc (default)
#   scripts\build-aura-compiler.ps1 -Aot       # native executable (needs LLVM)
#   scripts\build-aura-compiler.ps1 -Help
#
# NOTE: this script is ASCII-only on purpose, so it runs correctly under
#       Windows PowerShell 5.1 regardless of the active code page.
#
# Constraint: the Rust compiler (compiler/) is kept untouched; this script
#             only reads it, never modifies its sources.
# -------------------------------------------------------------
param(
    [switch]$Aot,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$Entry  = 'aura/compiler/aura/lang/compiler/Main.aura'
$OutDir = 'build'

if ($Help) {
    Write-Host "Usage: scripts\build-aura-compiler.ps1 [-Aot]"
    Write-Host "  -Aot   produce a native executable via LLVM (default: .auc bytecode)"
    exit 0
}

if (-not (Test-Path $Entry)) {
    Write-Host "[build-aura-compiler] ERROR: compiler entry not found: $Entry" -ForegroundColor Red
    exit 1
}

function Find-Aura {
    foreach ($candidate in @(
        'target/release/aura.exe', 'target/release/aura',
        'target/debug/aura.exe',   'target/debug/aura'
    )) {
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

$Aura = Find-Aura
if (-not $Aura) {
    Write-Host "[build-aura-compiler] aura binary not found, building the Rust compiler via cargo..."
    cargo build --release --manifest-path compiler/Cargo.toml
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[build-aura-compiler] ERROR: cargo build failed" -ForegroundColor Red
        exit 1
    }
    $Aura = Find-Aura
}

if (-not $Aura) {
    Write-Host "[build-aura-compiler] ERROR: aura binary still not found after build" -ForegroundColor Red
    exit 1
}

Write-Host "[build-aura-compiler] rust compiler: $Aura"
Write-Host "[build-aura-compiler] entry source:  $Entry"

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }

if ($Aot) {
    $Out = Join-Path $OutDir 'aura-compiler.exe'
    Write-Host "[build-aura-compiler] mode: AOT (LLVM) -> $Out"
    & $Aura build $Entry --aot --output $Out
} else {
    $Out = Join-Path $OutDir 'aura-compiler.auc'
    Write-Host "[build-aura-compiler] mode: bytecode -> $Out"
    & $Aura build $Entry --output $Out
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "[build-aura-compiler] ERROR: compilation failed" -ForegroundColor Red
    exit 1
}

Write-Host "[build-aura-compiler] OK: $Out" -ForegroundColor Green
