# -------------------------------------------------------------
# Build the Aura compiler (Self-Bootstrap) - Windows PowerShell
#
# Produces:
#   build/bin/aura.exe                 <- Bootstrap compiler (from seed)
#   build/auc/compiler/aura-compiler.auc   <- Aura-written compiler bytecode (default)
#   build/auc/compiler/aura-compiler.exe   <- same, AOT-compiled native exe (-Aot)
#
# Usage:
#   scripts\build-aura-compiler.ps1            # bytecode .auc (default)
#   scripts\build-aura-compiler.ps1 -Aot       # native executable (needs LLVM + photon)
#   scripts\build-aura-compiler.ps1 -NoBootstrap   # skip copying aura.exe
#   scripts\build-aura-compiler.ps1 -Help
#
# NOTE: this script is ASCII-only on purpose, so it runs correctly under
#       Windows PowerShell 5.1 regardless of the active code page.
# -------------------------------------------------------------
param(
    [switch]$Aot,
    [switch]$NoBootstrap,
    [switch]$FrozenSeed,
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$Entry  = 'aura/compiler/aura/lang/compiler/Main.aura'
$BinDir = 'build/bin'
$AucDir = 'build/auc/compiler'

if ($Help) {
    Write-Host "Usage: scripts\build-aura-compiler.ps1 [-Aot] [-NoBootstrap] [-FrozenSeed]"
    Write-Host "  -Aot           produce a native executable via LLVM (default: .auc bytecode)"
    Write-Host "  -NoBootstrap   do not copy the bootstrap aura.exe into build/bin"
    Write-Host "  -FrozenSeed    force the git-lfs seed (aura/seed/aura.exe) instead of rust/target"
    Write-Host ""
    Write-Host "Bootstrap: rust/target/{release,debug}/aura.exe (cargo-built), else aura/seed/aura.exe"
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

# 种子选择：优先用 cargo 从 `rust/` 构建的种子（路径正确、前端完整），
# 只有显式 -FrozenSeed 或两者都不存在时，才回退到 git-lfs 跟踪的冻结种子。
# 冻结种子把 AOT 运行库路径写死为旧目录名 `AuraLangWithRust\...`，已不再支持。
$SeedPath = ''
if (-not $FrozenSeed) {
    foreach ($c in @('rust/target/release/aura.exe', 'rust/target/debug/aura.exe')) {
        if (Test-Path $c) { $SeedPath = $c; break }
    }
}
if ($SeedPath -eq '') { $SeedPath = 'aura/seed/aura.exe' }
if (-not (Test-Path $SeedPath)) {
    Write-Host "[build-aura-compiler] ERROR: seed not found at $SeedPath" -ForegroundColor Red
    Write-Host "  Build it with:  cd rust; cargo build -p cli --features llvm --release"
    Write-Host "  (or run 'git lfs pull' to download the frozen seed at aura/seed/aura.exe)."
    exit 1
}

Write-Host "[build-aura-compiler] seed: $SeedPath"
Write-Host "[build-aura-compiler] entry: $Entry"
Write-Host "[build-aura-compiler] bin:   $BinDir"
Write-Host "[build-aura-compiler] auc:   $AucDir"

if (-not (Test-Path $BinDir)) { New-Item -ItemType Directory -Path $BinDir -Force | Out-Null }
if (-not (Test-Path $AucDir)) { New-Item -ItemType Directory -Path $AucDir -Force | Out-Null }

# ---- 不再需要旧目录名兼容层 ------------------------------------------
# 旧树 `AuraLangWithRust/` 已改名为 `rust/`（唯一目录，不再建联接）。
# 早期冻结的种子二进制把 AOT 的 C 运行库路径写死成 `AuraLangWithRust\...`，
# 为此曾建立 `AuraLangWithRust -> rust` 目录联接；现改为**优先使用 cargo
# 从 `rust/` 重新构建的种子**（其路径按 CARGO_MANIFEST_DIR 解析，天然指向
# `rust/compiler/...` 与 `aura/runtime/cffi/...`），因此无需任何兼容联接。
# 仅当显式指定 -FrozenSeed 时才回退到 `aura/seed/aura.exe`。

# 1) Bootstrap compiler -> build/bin/aura.exe
if (-not $NoBootstrap) {
    $BootstrapOut = Join-Path $BinDir 'aura.exe'
    Copy-Item $SeedPath $BootstrapOut -Force
    Write-Host "[build-aura-compiler] bootstrap -> $BootstrapOut" -ForegroundColor Green
}

$Aura = $SeedPath

# 2) Aura-written compiler -> build/auc/compiler/aura-compiler.(auc|exe)
if ($Aot) {
    $Out = Join-Path $AucDir 'aura-compiler.exe'
    Write-Host "[build-aura-compiler] mode: AOT (LLVM) -> $Out"
    $AuraArgs = @('build', $Entry, '--aot', '--output', $Out)
} else {
    $Out = Join-Path $AucDir 'aura-compiler.auc'
    Write-Host "[build-aura-compiler] mode: bytecode -> $Out"
    $AuraArgs = @('build', $Entry, '--output', $Out)
}

# The seed writes progress/errors to stderr; with $ErrorActionPreference='Stop'
# a single stderr line aborts the script, so relax it for this call only.
$prevEap   = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$BuildLog  = (& $Aura @AuraArgs 2>&1 | Out-String)
$BuildExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap

# Echo the informative lines only (skip the per-import [debug] noise)
($BuildLog -split "`r?`n") |
    Where-Object { $_ -match '\S' -and $_ -notmatch 'compiler_pkg_root|resolving compiler pkg import' } |
    Select-Object -Last 20 |
    ForEach-Object { Write-Host $_ }

if ($BuildExit -ne 0) {
    Write-Host "[build-aura-compiler] ERROR: compilation failed (exit $BuildExit)" -ForegroundColor Red
    exit 1
}

# ---- artifact integrity check -------------------------------------------
# The seed compiler degrades SILENTLY: it exits 0 while package imports are
# unresolved and many calls are unresolved, so the .auc only contains the entry
# file itself (not the imported compiler modules).
$PkgRootMisses = ([regex]::Matches($BuildLog, 'compiler_pkg_root is None!')).Count
# Pattern = "error: " + U+672A U+89E3 U+6790 ("unresolved"), spelled as char codes.
$Unresolved    = ([regex]::Matches($BuildLog, 'error: ' + [char]0x672A + [char]0x89E3 + [char]0x6790)).Count
if ($PkgRootMisses -gt 0 -or $Unresolved -gt 0) {
    Write-Host "[build-aura-compiler] ERROR: incomplete artifact - exit=0 but unresolved symbols remain" -ForegroundColor Red
    Write-Host "  compiler_pkg_root is None! : $PkgRootMisses (aura.lang.compiler.* package imports unresolved)"
    Write-Host "  unresolved function calls  : $Unresolved"
    Write-Host "  artifact: $Out"
    Write-Host "  It covers only $Entry and cannot be used as a compiler."
    exit 1
}

Write-Host "[build-aura-compiler] OK: $Out ($((Get-Item $Out).Length) bytes)" -ForegroundColor Green