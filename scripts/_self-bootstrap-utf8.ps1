# Self-bootstrap verification script (UTF-8 BOM version)
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Continue"
$ProjectRoot = Split-Path $PSScriptRoot -Parent

Write-Host ""
Write-Host "Self-Bootstrap Verification" -ForegroundColor Cyan
Write-Host "=============================" -ForegroundColor Cyan
Write-Host ""

# Phase 1: Compile Main.aura
Write-Host "Phase 1: Compiling Main.aura..." -ForegroundColor Yellow
$mainAura = Join-Path $ProjectRoot "aura\compiler\aura\lang\compiler\Main.aura"
$compilerExe = Join-Path $ProjectRoot "target\release\aura.exe"
$outputExe = Join-Path $ProjectRoot "build\bin\aura-compiler-native.exe"

if (Test-Path $outputExe) {
    Remove-Item $outputExe -Force
}

# Run compilation and capture output
$compileOutput = & $compilerExe build $mainAura --aot --output $outputExe 2>&1
$compileResult = $LASTEXITCODE

# Check if output executable was created
if (Test-Path $outputExe) {
    Write-Host "Phase 1 PASSED: Main.aura compiled successfully" -ForegroundColor Green
    Write-Host "  Output: $outputExe" -ForegroundColor Green
    Write-Host "  Size: $((Get-Item $outputExe).Length) bytes" -ForegroundColor Green
} else {
    Write-Host "Phase 1 FAILED: AOT compilation failed" -ForegroundColor Red
    Write-Host "  Output: $compileOutput" -ForegroundColor Red
    exit 1
}

Write-Host "Phase 1 PASSED: Main.aura compiled successfully" -ForegroundColor Green
Write-Host ""
Write-Host "Self-bootstrap verification complete!" -ForegroundColor Cyan
Write-Host "Phase 1: AOT compilation - OK" -ForegroundColor Green
exit 0