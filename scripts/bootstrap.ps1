#Requires -Version 5.0
# 完全 Aura 化自举验证脚本 (Windows PowerShell)
# 验证流程：最小 aura.exe → 编译 vm.aura → 用 vm.exe 重新编译 → 验证行为一致性
$ErrorActionPreference = 'Stop'

# ── 配置 ──────────────────────────────────────────────────────────
$RootDir    = (Split-Path -Parent $PSScriptRoot)
$Aurabin    = "$RootDir\target\release\aura.exe"
$AurabinDbg = "$RootDir\target\debug\aura.exe"
$VmOut      = "$RootDir\vm.exe"
$Vm2Out     = "$RootDir\vm2.exe"
$GcOut      = "$RootDir\gc.exe"
$Gc2Out     = "$RootDir\gc2.exe"
$MemOut     = "$RootDir\memory.exe"
$TestDir    = "$RootDir\tests\self_bootstrap"
$Out1File   = "$RootDir\output1.txt"
$Out2File   = "$RootDir\output2.txt"

Set-Location $RootDir

# ── 辅助函数 ──────────────────────────────────────────────────────
function Find-AuraExe {
    if (Test-Path $Aurabin) {
        return $Aurabin
    } elseif (Test-Path $AurabinDbg) {
        return $AurabinDbg
    } else {
        return $null
    }
}

function Test-FileExists {
    param([string]$Path, [string]$Message = "")
    if (-not (Test-Path $Path)) {
        Write-Host "✗ 文件不存在: $Path" -ForegroundColor Red
        if ($Message) { Write-Host "  $Message" -ForegroundColor Yellow }
        exit 1
    }
}

function Compare-Outputs {
    param([string]$File1, [string]$File2)
    $Content1 = Get-Content $File1 -Raw
    $Content2 = Get-Content $File2 -Raw
    if ($Content1 -eq $Content2) {
        return $true
    } else {
        return $false
    }
}

# ── 阶段 0: 环境检查 ──────────────────────────────────────────────
Write-Host "=== 阶段 0: 环境检查 ===" -ForegroundColor Cyan
Test-FileExists "compiler\Cargo.toml" "请先完成编译器代码"

# ── 阶段 1: 编译最小 aura.exe（Rust） ─────────────────────────────
Write-Host "=== 阶段 1: 编译最小 aura.exe ===" -ForegroundColor Cyan
Write-Host "  运行: cargo build --release --manifest-path compiler/Cargo.toml"
cargo build --release --manifest-path compiler/Cargo.toml
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ cargo build 失败" -ForegroundColor Red
    exit 1
}

# ── 阶段 2: 用最小 aura.exe 编译 core/aura/lang/std/ ───────────────
Write-Host "=== 阶段 2: 用最小 aura.exe 编译 vm.aura / gc.aura / memory.aura ===" -ForegroundColor Cyan
Test-FileExists "core\aura\lang\std\vm\vm.aura"     "请先编写 core/aura/lang/std/vm/vm.aura"
Test-FileExists "core\aura\lang\std\gc\gc.aura"     "请先编写 core/aura/lang/std/gc/gc.aura"
Test-FileExists "core\aura\lang\std\memory\memory.aura" "请先编写 core/aura/lang/std/memory/memory.aura"

Write-Host "  编译 vm.aura → vm.exe..."
& $Aurabin build "core\aura\lang\std\vm\vm.aura" --aot --output $VmOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ 编译 vm.aura 失败" -ForegroundColor Red; exit 1 }

Write-Host "  编译 gc.aura → gc.exe..."
& $Aurabin build "core\aura\lang\std\gc\gc.aura" --aot --output $GcOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ 编译 gc.aura 失败" -ForegroundColor Red; exit 1 }

Write-Host "  编译 memory.aura → memory.exe..."
& $Aurabin build "core\aura\lang\std\memory\memory.aura" --aot --output $MemOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ 编译 memory.aura 失败" -ForegroundColor Red; exit 1 }

# ── 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证） ────────────────
Write-Host "=== 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证）===" -ForegroundColor Cyan
Test-FileExists $VmOut

Write-Host "  用 vm.exe 编译 vm.aura → vm2.exe..."
& $VmOut build "core\aura\lang\std\vm\vm.aura" --aot --output $Vm2Out
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm.exe 编译 vm.aura 失败" -ForegroundColor Red; exit 1 }

Write-Host "  用 vm2.exe 编译 gc.aura → gc2.exe..."
& $Vm2Out build "core\aura\lang\std\gc\gc.aura" --aot --output $Gc2Out
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm2.exe 编译 gc.aura 失败" -ForegroundColor Red; exit 1 }

# ── 阶段 4: 验证行为一致性 ────────────────────────────────────────
Write-Host "=== 阶段 4: 验证行为一致性 ===" -ForegroundColor Cyan
Test-FileExists "$TestDir\vm_test.aura"

Write-Host "  vm.exe run vm_test.aura → output1.txt..."
& $VmOut run "$TestDir\vm_test.aura" > $Out1File
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm.exe 运行 vm_test.aura 失败" -ForegroundColor Red; exit 1 }

Write-Host "  vm2.exe run vm_test.aura → output2.txt..."
& $Vm2Out run "$TestDir\vm_test.aura" > $Out2File
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm2.exe 运行 vm_test.aura 失败" -ForegroundColor Red; exit 1 }

$Same = Compare-Outputs $Out1File $Out2File
if ($Same) {
    Write-Host "  ✓ 行为一致: vm.exe 与 vm2.exe 输出相同" -ForegroundColor Green
    Write-Host "  ✓ 自举验证通过" -ForegroundColor Green
} else {
    Write-Host "  ✗ 行为不一致:" -ForegroundColor Red
    Write-Host "--- vm.exe 输出 (output1.txt) ---"
    Get-Content $Out1File
    Write-Host "--- vm2.exe 输出 (output2.txt) ---"
    Get-Content $Out2File
    Write-Host "  ✗ 自举验证失败" -ForegroundColor Red
    exit 1
}

# ── 阶段 5: 验证性能 ──────────────────────────────────────────────
Write-Host "=== 阶段 5: 验证性能 ===" -ForegroundColor Cyan
Test-FileExists "$TestDir\performance_test.aura"

Write-Host "  运行 performance_test.aura (vm.exe)..."
$Stopwatch1 = New-Object System.Diagnostics.Stopwatch
$Stopwatch1.Start()
& $VmOut run "$TestDir\performance_test.aura" > $null
$Stopwatch1.Stop()
$Time1Ms = $Stopwatch1.ElapsedMilliseconds

Write-Host "  运行 performance_test.aura (vm2.exe)..."
$Stopwatch2 = New-Object System.Diagnostics.Stopwatch
$Stopwatch2.Start()
& $Vm2Out run "$TestDir\performance_test.aura" > $null
$Stopwatch2.Stop()
$Time2Ms = $Stopwatch2.ElapsedMilliseconds

Write-Host "  vm.exe 耗时:  ${Time1Ms}ms"
Write-Host "  vm2.exe 耗时: ${Time2Ms}ms"

# 性能差异检查（5% 容差）
if ($Time1Ms -gt 0) {
    $PerfDiff = [math]::Abs($Time1Ms - $Time2Ms) / $Time1Ms * 100
} else {
    $PerfDiff = 0
}
$PerfDiffStr = "{0:F2}" -f $PerfDiff

Write-Host "  性能差异: ${PerfDiffStr}%"

if ($PerfDiff -lt 5.0) {
    Write-Host "  ✓ 性能一致（差异 < 5%）" -ForegroundColor Green
} else {
    Write-Host "  ⚠ 性能差异超过 5%，请检查" -ForegroundColor Yellow
    Write-Host "  注意: 性能差异检查为参考指标，不影响验证结果"
}

# ── 阶段 6: 验证总结 ──────────────────────────────────────────────
Write-Host "=== 阶段 6: 验证总结 ===" -ForegroundColor Cyan
Write-Host "  vm.exe     →  $VmOut"
Write-Host "  vm2.exe    →  $Vm2Out"
Write-Host "  gc.exe     →  $GcOut"
Write-Host "  gc2.exe    →  $Gc2Out"
Write-Host "  memory.exe →  $MemOut"
Write-Host ""

# 备份原始 aura.exe
if (Test-Path $Aurabin) {
    $BackupPath = "$Aurabin.bak"
    Copy-Item $Aurabin $BackupPath -Force
    Write-Host "  已备份原始 aura.exe → $BackupPath"
}

# 替换（仅当设置 AUTO_REPLACE=true 时执行）
if (Get-Command $env:AUTO_REPLACE -ErrorAction SilentlyContinue) {
    if ($env:AUTO_REPLACE -eq "true") {
        Write-Host "  AUTO_REPLACE=true，正在替换..."
        Copy-Item $VmOut  $Aurabin  -Force
        Copy-Item $GcOut  "$RootDir\gc.exe" -Force
        Copy-Item $MemOut "$RootDir\memory.exe" -Force
        Write-Host "  ✓ 替换完成" -ForegroundColor Green
    } else {
        Write-Host "  未设置 AUTO_REPLACE=true，跳过自动替换"
        Write-Host "  如需替换，请手动执行:"
        Write-Host "    Copy-Item vm.exe target\release\aura.exe -Force"
        Write-Host "    Copy-Item gc.exe target\release\gc.exe -Force"
        Write-Host "    Copy-Item memory.exe target\release\memory.exe -Force"
    }
}

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  自举验证完成！" -ForegroundColor Cyan
Write-Host "  行为一致性: ✓" -ForegroundColor Green
Write-Host "  性能一致性: ${PerfDiffStr}% 差异"
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan

# 清理临时文件
Remove-Item $Out1File -ErrorAction SilentlyContinue
Remove-Item $Out2File -ErrorAction SilentlyContinue
Write-Host "  已删除临时输出文件"

Write-Host ""
Write-Host "验证完成时间: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
