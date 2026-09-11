# -------------------------------------------------------------
# Build the Aura compiler (Full-Aura migration) - Windows PowerShell
#
# Produces, under build/bin/:
#   aura.exe            <- Rust bootstrap compiler (minimal bootstrap layer)
#   aura-compiler.auc   <- "Aura compiler written in Aura", bytecode (default)
#   aura-compiler.exe   <- same, AOT-compiled native executable (-Aot, needs LLVM)
#
# Usage:
#   scripts\build-aura-compiler.ps1            # bytecode .auc (default)
#   scripts\build-aura-compiler.ps1 -Aot       # native executable (needs LLVM)
#   scripts\build-aura-compiler.ps1 -NoBootstrap   # skip copying aura.exe
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
    [switch]$NoBootstrap,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$Entry  = 'aura/compiler/aura/lang/compiler/Main.aura'
$OutDir = 'build/bin'

if ($Help) {
    Write-Host "Usage: scripts\build-aura-compiler.ps1 [-Aot] [-NoBootstrap]"
    Write-Host "  -Aot           produce a native executable via LLVM (default: .auc bytecode)"
    Write-Host "  -NoBootstrap   do not copy the bootstrap aura.exe into build/bin"
    Write-Host ""
    Write-Host "Outputs (build/bin): aura.exe, aura-compiler.(auc|exe)"
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

# AOT 需要 LLVM 后端：bootstrap 必须以 `--features llvm` 构建，否则
# `aura build --aot` 会直接报 "llvm feature 未启用"。
function Build-Bootstrap {
    Write-Host "[build-aura-compiler] building Rust bootstrap: cargo build --release -p cli --features llvm"
    cargo build --release -p cli --features llvm
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[build-aura-compiler] ERROR: cargo build failed" -ForegroundColor Red
        exit 1
    }
}

$Aura = Find-Aura
if ($Aot) {
    # 始终重建（cargo 命中缓存时很快），确保 llvm 后端可用
    Build-Bootstrap
    $Aura = Find-Aura
} elseif (-not $Aura) {
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

Write-Host "[build-aura-compiler] rust bootstrap: $Aura"
Write-Host "[build-aura-compiler] entry source:   $Entry"
Write-Host "[build-aura-compiler] output dir:     $OutDir"

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

# 1) 最小 bootstrap 编译器 → build/bin/aura.exe
if (-not $NoBootstrap) {
    $BootstrapOut = Join-Path $OutDir 'aura.exe'
    Copy-Item $Aura $BootstrapOut -Force
    Write-Host "[build-aura-compiler] bootstrap -> $BootstrapOut" -ForegroundColor Green
}

# 2) Aura 编写的编译器 → build/bin/aura-compiler.(auc|exe)
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
