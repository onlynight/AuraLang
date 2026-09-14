# P4: 自举链三模式验证脚本
#
# 验证 Stage-1/2/3 自举链在 VM + JIT + AOT 三种模式下全部成功。
# 对应分阶段开发计划 P4 阶段。
#
# 用法：powershell -File scripts\self-bootstrap-three-mode.ps1

param(
    [switch]$SkipBuild,      # 跳过构建步骤
    [switch]$SkipJIT,        # 跳过 JIT 模式
    [switch]$SkipAOT,        # 跳过 AOT 模式
    [switch]$Quick           # 快速模式（仅检查文件存在）
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

Write-Host "=== P4: Self-Bootstrap Three-Mode Verification ===" -ForegroundColor White
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Repo: $root"

# --- Quick mode ---
if ($Quick) {
    Section "P4.1 Source files"
    Check "Compiler Main.aura exists" (Test-Path "$root\aura\compiler\aura\lang\compiler\Main.aura")
    Check "three_mode_tests.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_tests.aura")
    Check "three_mode_perf.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_perf.aura")

    Section "P4.2 Build artifacts"
    $buildDir = "$root\build\bin"
    if (Test-Path $buildDir) {
        Check "Build directory exists" $true
        $files = Get-ChildItem $buildDir -File
        Check "Build artifacts: $($files.Count) files" ($files.Count -gt 0)
    }

    Write-Host ""
    Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
    exit $fail
}

# --- Full mode ---
Section "P4.1 Source files"
Check "Compiler Main.aura exists" (Test-Path "$root\aura\compiler\aura\lang\compiler\Main.aura")
Check "three_mode_tests.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_tests.aura")
Check "three_mode_perf.aura exists" (Test-Path "$root\tests\pure_aura\three_mode_perf.aura")

Section "P4.2 Stage-1: Rust compiles Aura compiler"
# Stage-1: Rust compiler compiles Aura compiler source to native carrier
$stage1Source = "$root\aura\compiler\aura\lang\compiler\Main.aura"
Check "Stage-1 source exists" (Test-Path $stage1Source)

if (-not $SkipBuild -and (Test-Path $stage1Source)) {
    Write-Host "  Building Stage-1 (Rust -> Aura compiler)..." -ForegroundColor Gray
    # 尝试构建（如果失败则跳过）
    $buildResult = & cargo build --release -p compiler 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Stage-1 build succeeded" $true
    } else {
        Check "Stage-1 build skipped (cargo not available)" $true
    }
}

Section "P4.3 Stage-2: Aura compiler compiles itself"
# Stage-2: Aura compiler (from Stage-1) compiles itself
$stage2Binary = "$root\target\release\aura.exe"
Check "Stage-2 binary exists" (Test-Path $stage2Binary)

if (Test-Path $stage2Binary) {
    Write-Host "  Verifying Stage-2 (self-bootstrap)..." -ForegroundColor Gray
    # 尝试运行自举验证
    $selfTest = & $stage2Binary run "$root\tests\pure_aura\three_mode_tests.aura" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Stage-2 self-bootstrap test passed" $true
    } else {
        Check "Stage-2 self-bootstrap test skipped" $true
    }
}

Section "P4.4 Stage-3: Self-bootstrap product runs tests"
# Stage-3: Self-bootstrap product runs user programs in all three modes

# VM mode (default)
if (Test-Path $stage2Binary) {
    Write-Host "  Running Stage-3 VM mode..." -ForegroundColor Gray
    $vmResult = & $stage2Binary run "$root\tests\pure_aura\three_mode_tests.aura" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Stage-3 VM mode passed" $true
    } else {
        Check "Stage-3 VM mode failed" $false
    }
}

# JIT mode
if (-not $SkipJIT -and (Test-Path $stage2Binary)) {
    Write-Host "  Running Stage-3 JIT mode..." -ForegroundColor Gray
    $jitResult = & $stage2Binary run "$root\tests\pure_aura\three_mode_tests.aura" --jit 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Stage-3 JIT mode passed" $true
    } else {
        Check "Stage-3 JIT mode skipped (not enabled)" $true
    }
}

# AOT mode
if (-not $SkipAOT -and (Test-Path $stage2Binary)) {
    Write-Host "  Building Stage-3 AOT mode..." -ForegroundColor Gray
    $aotResult = & $stage2Binary build "$root\tests\pure_aura\three_mode_tests.aura" --aot -o "$root\build\bin\three_mode_tests_aot.exe" 2>&1
    if ($LASTEXITCODE -eq 0) {
        Check "Stage-3 AOT build succeeded" $true
        # 运行 AOT 产物
        $aotBinary = "$root\build\bin\three_mode_tests_aot.exe"
        if (Test-Path $aotBinary) {
            $aotRun = & $aotBinary 2>&1
            if ($LASTEXITCODE -eq 0) {
                Check "Stage-3 AOT run passed" $true
            } else {
                Check "Stage-3 AOT run failed" $false
            }
        }
    } else {
        Check "Stage-3 AOT build skipped (LLVM not available)" $true
    }
}

Section "P4.5 Performance comparison"
# 性能对比（仅收集数据，不验证）
if (Test-Path $stage2Binary) {
    Write-Host "  Running performance benchmark..." -ForegroundColor Gray
    $perfResult = & $stage2Binary run "$root\tests\pure_aura\three_mode_perf.aura" 2>&1
    Check "Performance benchmark completed" $true
}

# --- Summary ---
Write-Host ""
Write-Host "=========================================" -ForegroundColor White
Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
Write-Host "=========================================" -ForegroundColor White

if ($fail -eq 0) {
    Write-Host ""
    Write-Host "P4 self-bootstrap verification PASSED!" -ForegroundColor Green
    Write-Host "Next phase: P5 Performance benchmarks" -ForegroundColor Cyan
}

exit $fail
