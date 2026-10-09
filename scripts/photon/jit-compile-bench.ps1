# -------------------------------------------------------------
# jit-compile-bench.ps1 — JIT 编译延迟基准（Phase 4.3.2）
#
# 测量种子 VM 下 compileEncodeOnly 对同一 2 函数源的每编译延迟
# （p19_compile_speed_test 的 C 段基准循环，10 次取均值）。
#
# 用法:
#   scripts\photon\jit-compile-bench.ps1 [-Runs 5]
# -------------------------------------------------------------
param(
    [int]$Runs = 5
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

$Aura = Join-Path $Root 'rust\target\release\aura.exe'
if (-not (Test-Path $Aura)) { Write-Host "aura.exe not found" -ForegroundColor Red; exit 1 }

Write-Host "=== JIT 编译延迟基准（seed VM, 2-func source, p19 C 段循环）===" -ForegroundColor Cyan

$samples = New-Object System.Collections.Generic.List[double]
for ($i = 1; $i -le $Runs; $i++) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $null = cmd /c "`"$Aura`" run tests/photon/P1/p19_compile_speed_test.aura 2>&1 > NUL"
    $sw.Stop()
    if ($LASTEXITCODE -ne 0) { Write-Host "run $i failed (exit=$LASTEXITCODE)" -ForegroundColor Red; exit 1 }
    $ms = $sw.Elapsed.TotalMilliseconds
    [void]$samples.Add($ms)
    Write-Host ("  run {0}: {1:N0} ms (whole test incl. VM boot + 10 compiles)" -f $i, $ms)
}

$sorted = $samples | Sort-Object
$med = $sorted[[int][math]::Floor($Runs / 2)]
$min = $sorted[0]
$max = $sorted[$Runs - 1]
Write-Host ""
Write-Host ("  min={0:N0} ms  med={1:N0} ms  max={2:N0} ms  (n={3})" -f $min, $med, $max, $Runs)
Write-Host "  注：含 VM 启动与 stdlib 合并（~1775 函数）固定开销； JIT 编译延迟趋势"
Write-Host "  以中位数跨提交对比为准。原生路径延迟待 P5-① 后由 jit_vmhost 自检测量。"
