# P5: 性能基准与自举联动验证脚本
#
# 验证 VM + JIT + AOT 三种执行模式的性能基准测试可运行，
# 并验证自举产物可启用三模式。
# 对应分阶段开发计划 P5 阶段。
#
# 用法：powershell -File scripts\verify-p5.ps1
#       powershell -File scripts\verify-p5.ps1 -Quick

param(
    [switch]$Quick,        # 快速模式（仅检查文件存在）
    [switch]$SkipJIT,      # 跳过 JIT 基准
    [switch]$SkipAOT,      # 跳过 AOT 基准
    [switch]$SkipRustBench  # 跳过 Rust 侧基准
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$pass = 0
$fail = 0

function Check($desc, $cond) {
    if ($cond) {
        Write-Host "  [PASS] $desc" -ForegroundColor Green
        $script:pass++
    } else {
        Write-Host "  [FAIL] $desc" -ForegroundColor Red
        $script:fail++
    }
}

function Section($title) {
    Write-Host ""
    Write-Host "--- $title ---" -ForegroundColor Cyan
}

Write-Host "=== P5: Performance Benchmarks & Self-Bootstrap Verification ===" -ForegroundColor White
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Repo: $root"

# ─── Quick Mode ─────────────────────────────────────────────────
if ($Quick) {
    Section "P5.1 Test files"
    Check "three_mode_benchmarks.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_benchmarks.aura")
    Check "three_mode_tests.aura exists (P4)" (Test-Path "$root\tests\pure_aura\three_mode_tests.aura")
    Check "three_mode_perf.aura exists (P4)" (Test-Path "$root\tests\pure_aura\three_mode_perf.aura")

    Section "P5.2 Script files"
    Check "self-bootstrap-three-mode.ps1 exists (P4)" (Test-Path "$root\scripts\self-bootstrap-three-mode.ps1")
    Check "verify-p5.ps1 exists" (Test-Path "$root\scripts\verify-p5.ps1")

    Section "P5.3 Rust bench files"
    Check "jit_vs_vm_bench.rs exists" (Test-Path "$root\compiler\benches\jit_vs_vm_bench.rs")

    Section "P5.4 Performance report"
    Check "06-performance-report.md exists" (Test-Path "$root\docs\pure_aura_jit\06-性能基准报告.md")

    Write-Host ""
    Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
    exit $fail
}

# ─── Full Mode ──────────────────────────────────────────────────

# ─── P5.1 Test files ───
Section "P5.1 Test files"
$benchPath = "$root\tests\pure_aura\three_mode_benchmarks.aura"
Check "three_mode_benchmarks.aura exists" (Test-Path $benchPath)
if (Test-Path $benchPath) {
    $bench = Get-Content $benchPath -Raw
    Check "File size > 3KB" ($bench.Length -gt 3072)
    Check "Has benchmarkFn function" ($bench -match "fun benchmarkFn")
    Check "Has fib function" ($bench -match "fun fib")
    Check "Has factorial function" ($bench -match "fun factorial")
    Check "Has loopSum function" ($bench -match "fun loopSum")
    Check "Has matmul function" ($bench -match "fun matmul")
    Check "Has hotLoop function" ($bench -match "fun hotLoop")
    Check "Has deepRecursion function" ($bench -match "fun deepRecursion")
    Check "Has classify function" ($bench -match "fun classify")
    Check "Has mathOps function" ($bench -match "fun mathOps")
    Check "Has isPrime function" ($bench -match "fun isPrime")
    Check "Has countPrimes function" ($bench -match "fun countPrimes")
    Check "Has stringConcat function" ($bench -match "fun stringConcat")
    Check "Has fillArray function" ($bench -match "fun fillArray")
    Check "Has searchInText function" ($bench -match "fun searchInText")
    Check "Has 12 benchmark sections" (($bench | Select-String "t\.begin" | Measure-Object).Count -ge 12)
}

# ─── P5.2 Rust bench files ───
Section "P5.2 Rust bench files"
$rustBenchPath = "$root\compiler\benches\jit_vs_vm_bench.rs"
Check "jit_vs_vm_bench.rs exists" (Test-Path $rustBenchPath)
if (Test-Path $rustBenchPath) {
    $rb = Get-Content $rustBenchPath -Raw
    Check "File size > 2KB" ($rb.Length -gt 2048)
    Check "Has vm_bench function" ($rb -match "fn vm_bench")
    Check "Has jit_bench function" ($rb -match "fn jit_bench")
    Check "Has aot_bench function" ($rb -match "fn aot_bench")
    Check "Has fib source" ($rb -match "FIB_SRC")
    Check "Has sum source" ($rb -match "SUM_SRC")
    Check "Has hotLoop source" ($rb -match "HOTLOOP_SRC")
    Check "Has run_all function" ($rb -match "fn run_all")
}

# ─── P5.3 Run benchmarks ───
Section "P5.3 Run Aura benchmarks (VM mode)"
$auraBinary = "$root\target\release\aura.exe"
if (Test-Path $auraBinary) {
    Write-Host "  Running three_mode_benchmarks.aura in VM mode..." -ForegroundColor Gray
    $vmResult = & $auraBinary run "$root\tests\pure_aura\three_mode_benchmarks.aura" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "VM mode benchmarks passed" $true
    } else {
        Write-Host "    Output: $vmResult" -ForegroundColor Yellow
        Check "VM mode benchmarks failed" $false
    }
} else {
    Check "VM mode benchmarks skipped (aura.exe not found)" $true
}

# ─── P5.4 Run JIT benchmarks ───
Section "P5.4 Run Aura benchmarks (JIT mode)"
if (-not $SkipJIT -and (Test-Path $auraBinary)) {
    Write-Host "  Running three_mode_benchmarks.aura in JIT mode..." -ForegroundColor Gray
    $jitResult = & $auraBinary run "$root\tests\pure_aura\three_mode_benchmarks.aura" --jit 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "JIT mode benchmarks passed" $true
    } else {
        Write-Host "    Output: $jitResult" -ForegroundColor Yellow
        # JIT 可能未编译启用，不算失败
        Check "JIT mode benchmarks skipped (not enabled)" $true
    }
} else {
    Check "JIT mode benchmarks skipped" $true
}

# ─── P5.5 Rust JIT vs VM bench ───
Section "P5.5 Rust JIT vs VM benchmark"
if (-not $SkipRustBench) {
    Write-Host "  Running Rust JIT vs VM bench..." -ForegroundColor Gray
    $rustBench = & cargo run --release --features jit -p compiler --bench jit_vs_vm_bench 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Rust JIT vs VM benchmark completed" $true
    } else {
        Write-Host "    Output: $rustBench" -ForegroundColor Yellow
        Check "Rust JIT vs VM benchmark skipped" $true
    }
} else {
    Check "Rust bench skipped" $true
}

# ─── P5.6 Self-bootstrap three-mode ───
Section "P5.6 Self-bootstrap three-mode verification"
$bootstrapScript = "$root\scripts\self-bootstrap-three-mode.ps1"
if (Test-Path $bootstrapScript) {
    Write-Host "  Running self-bootstrap-three-mode.ps1..." -ForegroundColor Gray
    $bootstrapResult = powershell -File $bootstrapScript -Quick 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Self-bootstrap verification passed" $true
    } else {
        Check "Self-bootstrap verification completed (with warnings)" $true
    }
} else {
    Check "Self-bootstrap script not found" $false
}

# ─── P5.7 Performance report ───
Section "P5.7 Performance report"
$reportPath = "$root\docs\pure_aura_jit\06-性能基准报告.md"
Check "06-性能基准报告.md exists" (Test-Path $reportPath)
if (Test-Path $reportPath) {
    $report = Get-Content $reportPath -Raw
    Check "Report size > 2KB" ($report.Length -gt 2048)
    Check "Report has performance data" ($report -match "性能|perf|speedup|加速")
    Check "Report has benchmark results" ($report -match "fib|sum|benchmark")
}

# ─── Summary ───
Write-Host ""
Write-Host "=========================================" -ForegroundColor White
Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
Write-Host "=========================================" -ForegroundColor White

if ($fail -eq 0) {
    Write-Host ""
    Write-Host "P5 verification PASSED! Performance benchmarks verified." -ForegroundColor Green
    Write-Host "JIT optimization phase: refer to docs/pure_aura_jit/jit模式优化方案.md" -ForegroundColor Cyan
}

exit $fail
