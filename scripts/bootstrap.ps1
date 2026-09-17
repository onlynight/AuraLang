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
        Write-Host "✗ File not found: $Path" -ForegroundColor Red
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
Write-Host "=== Phase 0: Environment check ===" -ForegroundColor Cyan
Test-FileExists "compiler\Cargo.toml" "Please complete the compiler code first"

# ── 阶段 1: 编译最小 aura.exe（Rust） ─────────────────────────────
Write-Host "=== Phase 1: Compile minimal aura.exe ===" -ForegroundColor Cyan
Write-Host "  Running: cargo build --release --manifest-path compiler/Cargo.toml"
cargo build --release --manifest-path compiler/Cargo.toml
if ($LASTEXITCODE -ne 0) {
    Write-Host "✗ cargo build failed" -ForegroundColor Red
    exit 1
}

# ── 阶段 2: 用最小 aura.exe 编译 core/aura/lang/std/ ───────────────
Write-Host "=== Phase 2: Compile vm.aura / gc.aura / memory.aura with minimal aura.exe ===" -ForegroundColor Cyan
Test-FileExists "core\aura\lang\std\vm\vm.aura"     "Please write core/aura/lang/std/vm/vm.aura first"
Test-FileExists "core\aura\lang\std\gc\gc.aura"     "Please write core/aura/lang/std/gc/gc.aura first"
Test-FileExists "core\aura\lang\std\memory\memory.aura" "Please write core/aura/lang/std/memory/memory.aura first"

Write-Host "  Compiling vm.aura → vm.exe..."
& $Aurabin build "core\aura\lang\std\vm\vm.aura" --aot --output $VmOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ Failed to compile vm.aura" -ForegroundColor Red; exit 1 }

Write-Host "  Compiling gc.aura → gc.exe..."
& $Aurabin build "core\aura\lang\std\gc\gc.aura" --aot --output $GcOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ Failed to compile gc.aura" -ForegroundColor Red; exit 1 }

Write-Host "  Compiling memory.aura → memory.exe..."
& $Aurabin build "core\aura\lang\std\memory\memory.aura" --aot --output $MemOut
if ($LASTEXITCODE -ne 0) { Write-Host "✗ Failed to compile memory.aura" -ForegroundColor Red; exit 1 }

# ── 阶段 3: 用 vm.exe 重新编译 vm.aura（自举验证） ────────────────
Write-Host "=== Phase 3: Recompile vm.aura with vm.exe (bootstrap verification) ===" -ForegroundColor Cyan
Test-FileExists $VmOut

Write-Host "  Compiling vm.aura with vm.exe → vm2.exe..."
& $VmOut build "core\aura\lang\std\vm\vm.aura" --aot --output $Vm2Out
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm.exe failed to compile vm.aura" -ForegroundColor Red; exit 1 }

Write-Host "  Compiling gc.aura with vm2.exe → gc2.exe..."
& $Vm2Out build "core\aura\lang\std\gc\gc.aura" --aot --output $Gc2Out
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm2.exe failed to compile gc.aura" -ForegroundColor Red; exit 1 }

# ── 阶段 4: 验证行为一致性 ────────────────────────────────────────
Write-Host "=== Phase 4: Verify behavioral consistency ===" -ForegroundColor Cyan
Test-FileExists "$TestDir\vm_test.aura"

Write-Host "  vm.exe run vm_test.aura → output1.txt..."
& $VmOut run "$TestDir\vm_test.aura" > $Out1File
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm.exe failed to run vm_test.aura" -ForegroundColor Red; exit 1 }

Write-Host "  vm2.exe run vm_test.aura → output2.txt..."
& $Vm2Out run "$TestDir\vm_test.aura" > $Out2File
if ($LASTEXITCODE -ne 0) { Write-Host "✗ vm2.exe failed to run vm_test.aura" -ForegroundColor Red; exit 1 }

$Same = Compare-Outputs $Out1File $Out2File
if ($Same) {
    Write-Host "  ✓ Behavior consistent: vm.exe and vm2.exe output identical" -ForegroundColor Green
    Write-Host "  ✓ Bootstrap verification passed" -ForegroundColor Green
} else {
    Write-Host "  ✗ Behavior inconsistent:" -ForegroundColor Red
    Write-Host "--- vm.exe output (output1.txt) ---"
    Get-Content $Out1File
    Write-Host "--- vm2.exe output (output2.txt) ---"
    Get-Content $Out2File
    Write-Host "  ✗ Bootstrap verification failed" -ForegroundColor Red
    exit 1
}

# ── 阶段 5: 验证性能 ──────────────────────────────────────────────
Write-Host "=== Phase 5: Verify performance ===" -ForegroundColor Cyan
Test-FileExists "$TestDir\performance_test.aura"

Write-Host "  Running performance_test.aura (vm.exe)..."
$Stopwatch1 = New-Object System.Diagnostics.Stopwatch
$Stopwatch1.Start()
& $VmOut run "$TestDir\performance_test.aura" > $null
$Stopwatch1.Stop()
$Time1Ms = $Stopwatch1.ElapsedMilliseconds

Write-Host "  Running performance_test.aura (vm2.exe)..."
$Stopwatch2 = New-Object System.Diagnostics.Stopwatch
$Stopwatch2.Start()
& $Vm2Out run "$TestDir\performance_test.aura" > $null
$Stopwatch2.Stop()
$Time2Ms = $Stopwatch2.ElapsedMilliseconds

Write-Host "  vm.exe elapsed:  ${Time1Ms}ms"
Write-Host "  vm2.exe elapsed: ${Time2Ms}ms"

# 性能差异检查（5% 容差）
if ($Time1Ms -gt 0) {
    $PerfDiff = [math]::Abs($Time1Ms - $Time2Ms) / $Time1Ms * 100
} else {
    $PerfDiff = 0
}
$PerfDiffStr = "{0:F2}" -f $PerfDiff

Write-Host "  Performance difference: ${PerfDiffStr}%"

if ($PerfDiff -lt 5.0) {
    Write-Host "  ✓ Performance consistent (difference < 5%)" -ForegroundColor Green
} else {
    Write-Host "  ⚠ Performance difference exceeds 5%, please check" -ForegroundColor Yellow
    Write-Host "  Note: Performance difference check is a reference metric and does not affect verification results"
}

# ── 阶段 6: 验证总结 ──────────────────────────────────────────────
Write-Host "=== Phase 6: Verification summary ===" -ForegroundColor Cyan
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
    Write-Host "  Backed up original aura.exe → $BackupPath"
}

# 替换（仅当设置 AUTO_REPLACE=true 时执行）
if (Get-Command $env:AUTO_REPLACE -ErrorAction SilentlyContinue) {
    if ($env:AUTO_REPLACE -eq "true") {
        Write-Host "  AUTO_REPLACE=true, replacing..."
        Copy-Item $VmOut  $Aurabin  -Force
        Copy-Item $GcOut  "$RootDir\gc.exe" -Force
        Copy-Item $MemOut "$RootDir\memory.exe" -Force
        Write-Host "  ✓ Replacement complete" -ForegroundColor Green
    } else {
        Write-Host "  AUTO_REPLACE=true not set, skipping auto-replacement"
        Write-Host "  To replace manually, run:"
        Write-Host "    Copy-Item vm.exe target\release\aura.exe -Force"
        Write-Host "    Copy-Item gc.exe target\release\gc.exe -Force"
        Write-Host "    Copy-Item memory.exe target\release\memory.exe -Force"
    }
}

Write-Host ""
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan
Write-Host "  Bootstrap verification complete!" -ForegroundColor Cyan
Write-Host "  Behavior consistency: ✓" -ForegroundColor Green
Write-Host "  Performance consistency: ${PerfDiffStr}% difference"
Write-Host "═══════════════════════════════════════════════════════" -ForegroundColor Cyan

# 清理临时文件
Remove-Item $Out1File -ErrorAction SilentlyContinue
Remove-Item $Out2File -ErrorAction SilentlyContinue
Write-Host "  Deleted temporary output files"

Write-Host ""
Write-Host "Verification completed at: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
