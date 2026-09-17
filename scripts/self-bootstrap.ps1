#Requires -Version 5.0
# Phase C.4: Aura 编译器自举验证脚本 (Windows PowerShell)
# 验证流程：Rust AOT → aura-compiler-native.exe → 自举编译 → 行为一致性验证
$ErrorActionPreference = 'Stop'

$RootDir   = (Split-Path -Parent $PSScriptRoot)
$Aurabin   = "$RootDir\target\release\aura.exe"
$MainAura  = "$RootDir\aura\compiler\aura\lang\compiler\Main.aura"
$NativeOut = "$RootDir\build\bin\aura-compiler-native.exe"
$Native2Out= "$RootDir\build\bin\aura-compiler-native2.exe"
$TestDir   = "$RootDir\tests"
$BuildDir  = "$RootDir\build\test"
$Out1      = "$BuildDir\output1.txt"
$Out2      = "$BuildDir\output2.txt"

Set-Location $RootDir
New-Item -ItemType Directory -Path "$RootDir\build\bin" -Force | Out-Null
New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null

# ── 阶段 0: 环境检查 ──
Write-Host "=== Phase 0: Environment check ===" -ForegroundColor Cyan
if (-not (Test-Path $Aurabin)) {
    Write-Host "✗ aura.exe not found. Building..." -ForegroundColor Yellow
    cargo build --release -p cli --features llvm
}
if (-not (Test-Path $MainAura)) {
    Write-Host "✗ Main.aura not found: $MainAura" -ForegroundColor Red; exit 1
}

# ── 阶段 1: 编译原生载体（Rust AOT 后端） ──
Write-Host "=== Phase 1: Compile native carrier (Rust AOT) ===" -ForegroundColor Cyan
Write-Host "  $Aurabin build $MainAura --aot -o $NativeOut"
& $Aurabin build $MainAura --aot -o $NativeOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ Failed to compile Main.aura" -ForegroundColor Red; exit 1 }
Write-Host "  ✓ $NativeOut generated" -ForegroundColor Green

# ── 阶段 2: 用原生载体重新编译自身（Aura 侧 AOT 后端） ──
Write-Host "=== Phase 2: Self-bootstrap (Aura AOT) ===" -ForegroundColor Cyan
Write-Host "  $NativeOut $MainAura -o $Native2Out"
& $NativeOut $MainAura -o $Native2Out
if ($LASTEXITCODE -ne 0) { Write-Host "✗ Self-bootstrap failed" -ForegroundColor Red; exit 1 }
Write-Host "  ✓ $Native2Out generated" -ForegroundColor Green

# ── 阶段 3: 行为一致性验证 ──
Write-Host "=== Phase 3: Behavioral consistency ===" -ForegroundColor Cyan
$TestFile = "$TestDir\pure_aura\hir_b1_when_tests.aura"
if (-not (Test-Path $TestFile)) {
    $TestFile = "$TestDir\string_methods_test.aura"
}

Write-Host "  Compiling test with native1..."
& $NativeOut $TestFile -o "$BuildDir\native1.exe"
& "$BuildDir\native1.exe" > $Out1

Write-Host "  Compiling test with native2..."
& $Native2Out $TestFile -o "$BuildDir\native2.exe"
& "$BuildDir\native2.exe" > $Out2

$content1 = Get-Content $Out1 -Raw
$content2 = Get-Content $Out2 -Raw
if ($content1 -eq $content2) {
    Write-Host "  ✓ Behavior consistent" -ForegroundColor Green
} else {
    Write-Host "  ✗ Behavior inconsistent:" -ForegroundColor Red
    Write-Host "--- native1 output ---"
    Get-Content $Out1
    Write-Host "--- native2 output ---"
    Get-Content $Out2
    exit 1
}

# ── 阶段 4: 性能对比 ──
Write-Host "=== Phase 4: Performance comparison ===" -ForegroundColor Cyan
$sw1 = New-Object System.Diagnostics.Stopwatch
$sw1.Start()
& $NativeOut $MainAura -o $null
$sw1.Stop()
$t1 = $sw1.ElapsedMilliseconds

$sw2 = New-Object System.Diagnostics.Stopwatch
$sw2.Start()
& $Native2Out $MainAura -o $null
$sw2.Stop()
$t2 = $sw2.ElapsedMilliseconds

$diff = if ($t1 -gt 0) { [math]::Abs($t1 - $t2) / $t1 * 100 } else { 0 }
Write-Host "  native1: ${t1}ms"
Write-Host "  native2: ${t2}ms"
Write-Host "  diff:    $([math]::Round($diff, 2))%"

# ── 总结 ──
Write-Host ""
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Self-bootstrap verification complete!" -ForegroundColor Cyan
Write-Host "  Behavior consistency: ✓" -ForegroundColor Green
Write-Host "  Performance diff: $([math]::Round($diff, 2))%"
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan