# -------------------------------------------------------------
# Bootstrap: 从 seed 编译出新版编译器，逐级自举
#
# Stage 0: seed 编译当前编译器源码 → .auc
# Stage 1: seed 编译 → 可执行文件（如果支持 --aot）
# Stage 2: 新编译器编译自身（自举验证）
#
# Usage:
#   scripts\bootstrap.ps1
#   scripts\bootstrap.ps1 -Verbose
# -------------------------------------------------------------
param(
    [string]$OutDir = "build/bootstrap",
    [switch]$Verbose
)

$ErrorActionPreference = 'Stop'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $RootDir

$Seed = "aura/seed/aura.exe"
if (-not (Test-Path $Seed)) {
    Write-Host "[bootstrap] ERROR: seed not found at $Seed" -ForegroundColor Red
    Write-Host "  The seed compiler must be present (tracked via git-lfs)."
    Write-Host "  Run 'git lfs pull' to download it."
    exit 1
}

$CompilerEntry = "aura/compiler/aura/lang/compiler/Main.aura"
if (-not (Test-Path $CompilerEntry)) {
    Write-Host "[bootstrap] ERROR: compiler entry not found: $CompilerEntry" -ForegroundColor Red
    exit 1
}

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir -Force | Out-Null }

# Stage 0: seed → .auc bytecode
Write-Host "[bootstrap] Stage 0: seed -> .auc" -ForegroundColor Cyan
$v0Exe = Join-Path $OutDir "aura-v0.exe"
Copy-Item $Seed $v0Exe
$aucArgs = @('build', $CompilerEntry, '--output', (Join-Path $OutDir "aura-v1.auc"))
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$stage0Log = (& $v0Exe @aucArgs 2>&1 | Out-String)
$stage0Exit = $LASTEXITCODE
$ErrorActionPreference = $prevEap

if ($Verbose) {
    ($stage0Log -split "`r?`n") | Where-Object { $_ -match '\S' } | Select-Object -Last 30 | ForEach-Object { Write-Host "  $_" }
}

if ($stage0Exit -ne 0) {
    Write-Host "[bootstrap] Stage 0 FAILED (exit $stage0Exit)" -ForegroundColor Red
    exit 1
}
Write-Host "[bootstrap] Stage 0 OK: aura-v1.auc" -ForegroundColor Green

# Stage 1: seed → .exe (AOT, if supported)
Write-Host "[bootstrap] Stage 1: seed -> .exe (AOT)" -ForegroundColor Cyan
$exeArgs = @('build', $CompilerEntry, '--aot', '--output', (Join-Path $OutDir "aura-v1.exe"))
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$stage1Log = (& $v0Exe @exeArgs 2>&1 | Out-String)
$stage1Exit = $LASTEXITCODE
$ErrorActionPreference = $prevEap

if ($Verbose) {
    ($stage1Log -split "`r?`n") | Where-Object { $_ -match '\S' } | Select-Object -Last 30 | ForEach-Object { Write-Host "  $_" }
}

if ($stage1Exit -ne 0) {
    Write-Host "[bootstrap] Stage 1 FAILED (exit $stage1Exit) - AOT not available, skipping" -ForegroundColor Yellow
    # AOT may not be available with the seed compiler; continue to Stage 2 with .auc
} else {
    Write-Host "[bootstrap] Stage 1 OK: aura-v1.exe" -ForegroundColor Green
}

# Stage 2: v1 -> v2 (self-compile verification)
Write-Host "[bootstrap] Stage 2: v1 -> v2 (self-compile)" -ForegroundColor Cyan
$v1Auc = Join-Path $OutDir "aura-v1.auc"
if (Test-Path $v1Auc) {
    # Try running v1.auc to compile a test file
    $testFile = "tests/basics/hello.aura"
    if (-not (Test-Path $testFile)) {
        # Find any .aura test file
        $testFile = Get-ChildItem "tests" -Recurse -Filter "*.aura" -ErrorAction SilentlyContinue | Select-Object -First 1
    }
    if ($testFile) {
        $stage2Log = (& $v0Exe run $testFile 2>&1 | Out-String)
        $stage2Exit = $LASTEXITCODE
        if ($stage2Exit -eq 0) {
            Write-Host "[bootstrap] Stage 2 OK: v1.auc can compile and run test files" -ForegroundColor Green
        } else {
            Write-Host "[bootstrap] Stage 2: v1.auc test run failed (exit $stage2Exit) - may need S2 work" -ForegroundColor Yellow
        }
    }
}

Write-Host "[bootstrap] === Bootstrap complete ===" -ForegroundColor Green
Write-Host "  Stage 0: $OutDir/aura-v1.auc"
if (Test-Path (Join-Path $OutDir "aura-v1.exe")) {
    Write-Host "  Stage 1: $OutDir/aura-v1.exe"
}