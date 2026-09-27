# -------------------------------------------------------------
# Build the Aura core standard library (Aura → .auc bytecode)
#
# Compiles all .aura files under aura/core/ into .auc bytecode,
# preserving the directory structure so each class lands in its
# own .auc file (its own package):
#
#   aura/core/aura/lang/String.aura
#       → build/aura_core_auc/aura/lang/String.auc
#   aura/core/aura/lang/std/Math.aura
#       → build/aura_core_auc/aura/lang/std/Math.auc
#   aura/core/aura/lang/concurrent/Mutex.aura
#       → build/aura_core_auc/aura/lang/concurrent/Mutex.auc
#
# Output: build/aura_core_auc/  (directory mirrors aura/core/)
#
# Usage:
#   scripts\build-aura-core.ps1
#   scripts\build-aura-core.ps1 -Help
#
# NOTE: this script is ASCII-only on purpose, so it runs correctly
#       under Windows PowerShell 5.1 regardless of the active code page.
# -------------------------------------------------------------
param(
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$CoreDir   = 'aura/core'
$OutDir    = 'build/aura_core_auc'

if ($Help) {
    Write-Host "Usage: scripts\build-aura-core.ps1 [-Help]"
    Write-Host ""
    Write-Host "Pre-compile the Aura core standard library (aura/core/**/*.aura) into .auc"
    Write-Host "bytecode files under build/aura_core_auc/, mirroring the source directory"
    Write-Host "structure so each class gets its own .auc file (package)."
    Write-Host ""
    Write-Host "Bootstrap compiler resolution:"
    Write-Host "  1. rust/target/{release,debug}/aura.exe"
    Write-Host "  2. build/bin/aura.exe"
    Write-Host "  3. aura/seed/aura.exe"
    Write-Host ""
    Write-Host "Outputs:"
    Write-Host "  build/aura_core_auc/aura/lang/**/*.auc   (mirrors aura/core/)"
    exit 0
}

# ---- locate bootstrap compiler -------------------------------------------
$SeedPath = ''
foreach ($c in @('rust/target/release/aura.exe', 'rust/target/debug/aura.exe', 'build/bin/aura.exe', 'aura/seed/aura.exe')) {
    if (Test-Path $c) { $SeedPath = $c; break }
}
if ($SeedPath -eq '') {
    Write-Host "[build-aura-core] ERROR: no bootstrap compiler found" -ForegroundColor Red
    Write-Host "  Build it with:  cd rust; cargo build -p cli --features llvm --release"
    exit 1
}
Write-Host "[build-aura-core] compiler: $SeedPath"
Write-Host "[build-aura-core] source:   $CoreDir"
Write-Host "[build-aura-core] output:   $OutDir"

if (-not (Test-Path $CoreDir)) {
    Write-Host "[build-aura-core] ERROR: source directory not found: $CoreDir" -ForegroundColor Red
    exit 1
}

# ---- run stdlib-compile ---------------------------------------------------
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$BuildLog  = (& $SeedPath 'stdlib-compile' $CoreDir --output $OutDir 2>&1 | Out-String)
$BuildExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap

# Echo informative lines only
($BuildLog -split "`r?`n") |
    Where-Object { $_ -match '\S' -and $_ -notmatch 'compiler_pkg_root' } |
    Select-Object -Last 20 |
    ForEach-Object { Write-Host $_ }

if ($BuildExit -ne 0) {
    Write-Host "[build-aura-core] ERROR: compilation failed (exit $BuildExit)" -ForegroundColor Red
    exit 1
}

# ---- artifact summary -----------------------------------------------------
$AucFiles = Get-ChildItem $OutDir -Recurse -Filter "*.auc" -ErrorAction SilentlyContinue
$AucCount = if ($AucFiles) { $AucFiles.Count } else { 0 }
$totalSize = if ($AucFiles) { ($AucFiles | Measure-Object -Property Length -Sum).Sum } else { 0 }

if ($AucCount -eq 0) {
    Write-Host "[build-aura-core] WARNING: no .auc files produced" -ForegroundColor Yellow
    exit 1
}

Write-Host "[build-aura-core] OK: $AucCount .auc files, $([math]::Round($totalSize / 1MB, 2)) MB total" -ForegroundColor Green
Write-Host "[build-aura-core] output: $OutDir"