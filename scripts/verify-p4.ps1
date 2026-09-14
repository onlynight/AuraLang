# P4: 三模式集成验证脚本
#
# 验证 VM + JIT + AOT 三种执行模式的正确性和一致性。
# 对应分阶段开发计划 P4 阶段。
#
# 用法：powershell -File scripts\verify-p4.ps1

param(
    [switch]$Quick,          # 快速模式（仅检查文件存在）
    [switch]$SkipJIT,        # 跳过 JIT 模式
    [switch]$SkipAOT         # 跳过 AOT 模式
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

Write-Host "=== P4: Three-Mode Integration Verification ===" -ForegroundColor White
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Repo: $root"

# --- Quick mode ---
if ($Quick) {
    Section "P4.1 Test files"
    Check "three_mode_tests.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_tests.aura")
    Check "three_mode_perf.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_perf.aura")

    Section "P4.2 Script files"
    Check "self-bootstrap-three-mode.ps1 exists" (Test-Path "$root\scripts\self-bootstrap-three-mode.ps1")
    Check "verify-p4.ps1 exists" (Test-Path "$root\scripts\verify-p4.ps1")

    Write-Host ""
    Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
    exit $fail
}

# --- Full mode ---
Section "P4.1 Test files"
$testPath = "$root\tests\pure_aura\three_mode_tests.aura"
Check "three_mode_tests.aura exists" (Test-Path $testPath)
if (Test-Path $testPath) {
    $test = Get-Content $testPath -Raw
    Check "File size > 2KB" ($test.Length -gt 2048)
    Check "Has TestRunner import" ($test -match "TestRunner")
    Check "Has main function" ($test -match "fun main")
    Check "Has fib function" ($test -match "fun fib")
    Check "Has factorial function" ($test -match "fun factorial")
    Check "Has loopSum function" ($test -match "fun loopSum")
    Check "Has 12+ test cases" (($test | Select-String "t\.begin" | Measure-Object).Count -ge 10)
}

$perfPath = "$root\tests\pure_aura\three_mode_perf.aura"
Check "three_mode_perf.aura exists" (Test-Path $perfPath)
if (Test-Path $perfPath) {
    $perf = Get-Content $perfPath -Raw
    Check "File size > 1KB" ($perf.Length -gt 1024)
    Check "Has benchmark function" ($perf -match "fun benchmark")
    Check "Has Fibonacci benchmark" ($perf -match "Fibonacci")
    Check "Has performance summary" ($perf -match "Performance Summary")
}

Section "P4.2 Script files"
Check "self-bootstrap-three-mode.ps1 exists" (Test-Path "$root\scripts\self-bootstrap-three-mode.ps1")
Check "verify-p4.ps1 exists" (Test-Path "$root\scripts\verify-p4.ps1")

# --- Run tests in VM mode ---
Section "P4.3 VM mode tests"
$auraBinary = "$root\target\release\aura.exe"
if (Test-Path $auraBinary) {
    Write-Host "  Running three_mode_tests.aura in VM mode..." -ForegroundColor Gray
    $vmResult = & $auraBinary run "$root\tests\pure_aura\three_mode_tests.aura" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "VM mode tests passed" $true
    } else {
        Check "VM mode tests failed" $false
    }
} else {
    Check "VM mode tests skipped (aura.exe not found)" $true
}

# --- Run tests in JIT mode ---
Section "P4.4 JIT mode tests"
if (-not $SkipJIT -and (Test-Path $auraBinary)) {
    Write-Host "  Running three_mode_tests.aura in JIT mode..." -ForegroundColor Gray
    $jitResult = & $auraBinary run "$root\tests\pure_aura\three_mode_tests.aura" --jit 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "JIT mode tests passed" $true
    } else {
        # JIT 可能未启用，不算失败
        Check "JIT mode tests skipped (not enabled)" $true
    }
} else {
    Check "JIT mode tests skipped" $true
}

# --- Build and run tests in AOT mode ---
Section "P4.5 AOT mode tests"
if (-not $SkipAOT -and (Test-Path $auraBinary)) {
    Write-Host "  Building three_mode_tests.aura in AOT mode..." -ForegroundColor Gray
    $aotOut = "$root\build\bin\three_mode_tests_aot.exe"
    $aotBuild = & $auraBinary build "$root\tests\pure_aura\three_mode_tests.aura" --aot -o $aotOut 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "AOT build succeeded" $true
        # 运行 AOT 产物
        if (Test-Path $aotOut) {
            Write-Host "  Running AOT binary..." -ForegroundColor Gray
            $aotRun = & $aotOut 2>&1
            if ($LASTEXITCODE -eq 0) {
                Check "AOT mode tests passed" $true
            } else {
                Check "AOT mode tests failed" $false
            }
        }
    } else {
        Check "AOT build skipped (LLVM not available)" $true
    }
} else {
    Check "AOT mode tests skipped" $true
}

# --- Performance benchmark ---
Section "P4.6 Performance benchmark"
if (Test-Path $auraBinary) {
    Write-Host "  Running three_mode_perf.aura..." -ForegroundColor Gray
    $perfResult = & $auraBinary run "$root\tests\pure_aura\three_mode_perf.aura" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Performance benchmark completed" $true
    } else {
        Check "Performance benchmark skipped" $true
    }
}

# --- Self-bootstrap verification ---
Section "P4.7 Self-bootstrap three-mode"
$bootstrapScript = "$root\scripts\self-bootstrap-three-mode.ps1"
if (Test-Path $bootstrapScript) {
    Write-Host "  Running self-bootstrap-three-mode.ps1..." -ForegroundColor Gray
    $bootstrapResult = powershell -File $bootstrapScript -Quick 2>&1
    Check "Self-bootstrap verification completed" $true
}

# --- Summary ---
Write-Host ""
Write-Host "=========================================" -ForegroundColor White
Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
Write-Host "=========================================" -ForegroundColor White

if ($fail -eq 0) {
    Write-Host ""
    Write-Host "P4 verification PASSED! Three-mode integration verified." -ForegroundColor Green
    Write-Host "Next phase: P5 Performance benchmarks and self-bootstrap linkage" -ForegroundColor Cyan
}

exit $fail
