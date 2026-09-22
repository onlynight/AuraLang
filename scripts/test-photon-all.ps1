# ============================================================
# Photon P1-P4 Test Suite (Simplified)
# ============================================================
#
# Usage:
#   scripts\test-photon-all.ps1 -Phase P1
#   scripts\test-photon-all.ps1 -Phase all
#   scripts\test-photon-all.ps1 -DryRun
#
# ============================================================

param(
    [ValidateSet("all", "P1", "P2", "P3", "P4")]
    [string]$Phase = "all",
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

$OutDir = Join-Path $Root 'build/photon-tests'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# ---- Find tools ----
function Find-Aura {
    foreach ($c in @(
        'rust/target/release/aura.exe', 'build/bin/aura.exe',
        'aura/seed/aura.exe', 'target/release/aura.exe', 'target/debug/aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Invoke-Captured {
    param([string]$Exe, [string[]]$ArgList, [string]$WorkDir)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    if ($WorkDir) { $psi.WorkingDirectory = $WorkDir }
    $psi.Arguments = ($ArgList -join ' ')
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return @{ Out = $so; Err = $se; Code = $p.ExitCode }
}

# Find tools
$AuraBin = Find-Aura
if (-not $AuraBin) { throw "no Aura compiler found" }
Write-Host "Aura:     $AuraBin"

if ($DryRun) {
    Write-Host "`nDry run mode - no compilation performed"
    exit 0
}

# ---- Test collection ----
$tests = @()
if ($Phase -eq "all" -or $Phase -eq "P1") {
    $tests += @(
        @{ Name="01_hello_world"; Phase="P1"; Expect="Hello, World!`r`n" },
        @{ Name="02_simple_vars"; Phase="P1"; Expect="x = 42`r`ny = 1000000`r`nz = true`r`n" },
        @{ Name="03_arithmetic"; Phase="P1"; Expect="a + b = 13`r`na - b = 7`r`na * b = 30`r`na / b = 3`r`na % b = 1`r`nc = 6`r`n" },
        @{ Name="04_control_flow"; Phase="P1"; Expect="x > 3`r`nwhile i = 0`r`nwhile i = 1`r`nwhile i = 2`r`nwhile i = 3`r`nwhile i = 4`r`nfor j = 0`r`nfor j = 1`r`nfor j = 2`r`n" },
        @{ Name="05_functions"; Phase="P1"; Expect="add(3,4) = 7`r`nmultiply(5,6) = 30`r`nfactorial(5) = 120`r`n" }
    )
}
if ($Phase -eq "all" -or $Phase -eq "P2") {
    $tests += @(
        @{ Name="01_nested_loop"; Phase="P2"; Expect="sum = 100`r`n" },
        @{ Name="02_fibonacci"; Phase="P2"; Expect="fib_recursive(10) = 55`r`nfib_iterative(10) = 55`r`n" },
        @{ Name="03_array_ops"; Phase="P2"; Expect="arr[0] = 1`r`narr[4] = 5`r`narr[2] = 99`r`nsum = 111`r`n" },
        @{ Name="04_string_ops"; Phase="P2"; Expect="s3 = Hello World`r`nlen = 11`r`nMatch!`r`n" }
    )
}
if ($Phase -eq "all" -or $Phase -eq "P3") {
    $tests += @(
        @{ Name="test_syscall_write"; Phase="P3"; Expect="Syscall write test" },
        @{ Name="test_syscall_mmap"; Phase="P3"; Expect="Syscall mmap test" },
        @{ Name="test_syscall_read"; Phase="P3"; Expect="Syscall read test" },
        @{ Name="test_nt_writefile"; Phase="P3"; Expect="NtWriteFile test" },
        @{ Name="test_nt_createfile"; Phase="P3"; Expect="NtCreateFile test" },
        @{ Name="06_syscall_exit"; Phase="P3"; Expect="Before exit" }
    )
}
if ($Phase -eq "all" -or $Phase -eq "P4") {
    $tests += @(
        @{ Name="test_memory_alloc"; Phase="P4"; Expect="Memory alloc test" },
        @{ Name="test_arc_refcount"; Phase="P4"; Expect="ARC test" },
        @{ Name="test_exception"; Phase="P4"; Expect="Exception test" },
        @{ Name="test_thread"; Phase="P4"; Expect="Thread test" },
        @{ Name="test_runtime_init"; Phase="P4"; Expect="Runtime init test" },
        @{ Name="06_gc_collect"; Phase="P4"; Expect="GC test" },
        @{ Name="07_mutex_ops"; Phase="P4"; Expect="Mutex test" }
    )
}

Write-Host "`n=========================================="
Write-Host " Photon P1-P4 Test Suite"
Write-Host "=========================================="
Write-Host " Phase: $Phase"
Write-Host " Tests: $($tests.Count)"
Write-Host " Output: $OutDir"
Write-Host ""

# ---- Compile and run each test ----
$passed = 0
$failed = 0
$skipped = 0
$results = @()

foreach ($test in $tests) {
    $testName = $test.Name
    $phase = $test.Phase
    $source = "tests/photon/$phase/$testName.aura"
    $hirOut = Join-Path $OutDir "$phase-$testName.photon.hir"
    
    Write-Host "[$phase] $testName..."
    
    # Check if source exists
    if (-not (Test-Path $source)) {
        Write-Host "  [SKIP] Source not found: $source" -ForegroundColor Yellow
        $skipped++
        $results += @{ Name="$phase/$testName"; Status="SKIP"; Reason="Source not found" }
        continue
    }
    
    # Step 1: Compile with Rust front-end (aura build -b photon)
    $buildArgs = @('build', '-b', 'photon', $source, '--output', $hirOut)
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $res = Invoke-Captured -Exe $AuraBin -ArgList $buildArgs -WorkDir $Root
    $ErrorActionPreference = $prevEap
    
    if ($res.Code -ne 0) {
        Write-Host "  [FAIL] Front-end failed" -ForegroundColor Red
        $failed++
        $results += @{ Name="$phase/$testName"; Status="FAIL"; Reason="Front-end failed"; Error=$res.Err }
        continue
    }
    
    if (-not (Test-Path $hirOut)) {
        Write-Host "  [FAIL] HIR output not found" -ForegroundColor Red
        $failed++
        $results += @{ Name="$phase/$testName"; Status="FAIL"; Reason="HIR not generated" }
        continue
    }
    
    $hirSize = (Get-Item $hirOut).Length
    Write-Host "  [PASS] HIR generated ($hirSize bytes)" -ForegroundColor Green
    $passed++
    $results += @{ Name="$phase/$testName"; Status="PASS"; Size=$hirSize }
    
    # Cleanup
    if (Test-Path $hirOut) { Remove-Item $hirOut -Force }
    
    Write-Host ""
}

# ---- Summary ----
Write-Host "=========================================="
Write-Host " Test Summary"
Write-Host "=========================================="
Write-Host " Total:   $($tests.Count)"
Write-Host " Passed:  $passed" -ForegroundColor Green
Write-Host " Failed:  $failed" -ForegroundColor Red
Write-Host " Skipped: $skipped" -ForegroundColor Yellow
Write-Host ""

if ($failed -gt 0) {
    Write-Host "Failed tests:" -ForegroundColor Red
    foreach ($r in $results | Where-Object { $_.Status -eq "FAIL" }) {
        Write-Host "  - $($r.Name): $($r.Reason)"
        if ($r.Error) { Write-Host "    Error: $($r.Error)" }
    }
}

exit $failed