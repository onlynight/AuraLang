# -------------------------------------------------------------
# Build the Aura compiler (Self-Bootstrap) - Windows PowerShell
#
# Produces:
#   build/bin/aura.exe                 <- Bootstrap compiler (from seed or Rust)
#   build/auc/compiler/aura-compiler.auc   <- Aura-written compiler bytecode (default)
#   build/auc/compiler/aura-compiler.exe   <- same, AOT-compiled native exe (-Aot)
#
# Usage:
#   scripts\build-aura-compiler.ps1            # bytecode .auc (default)
#   scripts\build-aura-compiler.ps1 -Aot       # native executable (needs LLVM)
#   scripts\build-aura-compiler.ps1 -NoBootstrap   # skip copying aura.exe
#   scripts\build-aura-compiler.ps1 -RebuildSeed   # rebuild seed from Rust source
#   scripts\build-aura-compiler.ps1 -Help
#
# Bootstrap resolution order:
#   1. aura/seed/aura.exe          <- pre-built seed (preferred)
#   2. target/release/aura.exe     <- local Rust build
#   3. cargo build (if neither exists)
#
# NOTE: this script is ASCII-only on purpose, so it runs correctly under
#       Windows PowerShell 5.1 regardless of the active code page.
# -------------------------------------------------------------
param(
    [switch]$Aot,
    [switch]$NoBootstrap,
    [switch]$RebuildSeed,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$Entry  = 'aura/compiler/aura/lang/compiler/Main.aura'
$BinDir = 'build/bin'
$AucDir = 'build/auc/compiler'

if ($Help) {
    Write-Host "Usage: scripts\build-aura-compiler.ps1 [-Aot] [-NoBootstrap] [-RebuildSeed]"
    Write-Host "  -Aot           produce a native executable via LLVM (default: .auc bytecode)"
    Write-Host "  -NoBootstrap   do not copy the bootstrap aura.exe into build/bin"
    Write-Host "  -RebuildSeed   rebuild aura/seed/aura.exe from Rust source"
    Write-Host ""
    Write-Host "Bootstrap resolution order:"
    Write-Host "  1. aura/seed/aura.exe          (pre-built seed, preferred)"
    Write-Host "  2. target/release/aura.exe     (local Rust build)"
    Write-Host "  3. cargo build                 (if neither exists)"
    Write-Host ""
    Write-Host "Outputs:"
    Write-Host "  build/bin/aura.exe                        Bootstrap compiler"
    Write-Host "  build/auc/compiler/aura-compiler.(auc|exe)   Aura-written compiler"
    exit 0
}

if (-not (Test-Path $Entry)) {
    Write-Host "[build-aura-compiler] ERROR: compiler entry not found: $Entry" -ForegroundColor Red
    exit 1
}

function Find-Aura {
    # Prefer the pre-built seed file (no Rust toolchain needed)
    if (Test-Path 'aura/seed/aura.exe') {
        return 'aura/seed/aura.exe'
    }
    foreach ($candidate in @(
        'target/release/aura.exe', 'target/release/aura',
        'target/debug/aura.exe',   'target/debug/aura'
    )) {
        if (Test-Path $candidate) { return $candidate }
    }
    return $null
}

# AOT 需要 LLVM 后端：bootstrap 必须以 `--features llvm` 构建

# Rebuild seed from Rust source (optional)
if ($RebuildSeed) {
    Write-Host "[build-aura-compiler] rebuilding seed from Rust source..."
    if (-not (Test-Path 'compiler/Cargo.toml')) {
        Write-Host "[build-aura-compiler] ERROR: compiler/Cargo.toml not found, cannot rebuild seed" -ForegroundColor Red
        exit 1
    }
    cargo build --release -p cli --features llvm
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[build-aura-compiler] ERROR: cargo build failed" -ForegroundColor Red
        exit 1
    }
    $SeedDir = 'aura/seed'
    if (-not (Test-Path $SeedDir)) { New-Item -ItemType Directory -Path $SeedDir -Force | Out-Null }
    Copy-Item 'target/release/aura.exe' (Join-Path $SeedDir 'aura.exe') -Force
    Write-Host "[build-aura-compiler] seed rebuilt -> aura/seed/aura.exe" -ForegroundColor Green
    exit 0
}

$Aura = Find-Aura
if ($Aot) {
    # AOT needs LLVM backend: ensure the seed has it, or rebuild
    if ($Aura -eq 'aura/seed/aura.exe') {
        Write-Host "[build-aura-compiler] AOT mode: seed may lack LLVM support, rebuilding..."
        if (-not (Test-Path 'compiler/Cargo.toml')) {
            Write-Host "[build-aura-compiler] ERROR: compiler/Cargo.toml not found, cannot rebuild for AOT" -ForegroundColor Red
            exit 1
        }
        cargo build --release -p cli --features llvm
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[build-aura-compiler] ERROR: cargo build failed" -ForegroundColor Red
            exit 1
        }
        $Aura = 'target/release/aura.exe'
    }
} elseif (-not $Aura) {
    Write-Host "[build-aura-compiler] no bootstrap found, attempting cargo build..."
    if (-not (Test-Path 'compiler/Cargo.toml')) {
        Write-Host "[build-aura-compiler] ERROR: compiler/Cargo.toml not found" -ForegroundColor Red
        exit 1
    }
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
Write-Host "[build-aura-compiler] bin dir:        $BinDir"
Write-Host "[build-aura-compiler] auc dir:        $AucDir"

if (-not (Test-Path $BinDir)) { New-Item -ItemType Directory -Path $BinDir -Force | Out-Null }
if (-not (Test-Path $AucDir)) { New-Item -ItemType Directory -Path $AucDir -Force | Out-Null }

# 1) 最小 bootstrap 编译器 → build/bin/aura.exe
if (-not $NoBootstrap) {
    $BootstrapOut = Join-Path $BinDir 'aura.exe'
    Copy-Item $Aura $BootstrapOut -Force
    Write-Host "[build-aura-compiler] bootstrap -> $BootstrapOut" -ForegroundColor Green
}

# 2) Aura 编写的编译器 → build/auc/compiler/aura-compiler.(auc|exe)
if ($Aot) {
    $Out = Join-Path $AucDir 'aura-compiler.exe'
    Write-Host "[build-aura-compiler] mode: AOT (LLVM) -> $Out"
    & $Aura build $Entry --aot --output $Out
} else {
    $Out = Join-Path $AucDir 'aura-compiler.auc'
    Write-Host "[build-aura-compiler] mode: bytecode -> $Out"
    & $Aura build $Entry --output $Out
}

if ($LASTEXITCODE -ne 0) {
    Write-Host "[build-aura-compiler] ERROR: compilation failed" -ForegroundColor Red
    exit 1
}

Write-Host "[build-aura-compiler] OK: $Out" -ForegroundColor Green
