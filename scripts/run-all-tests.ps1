# -------------------------------------------------------------
# Run all Aura tests with snapshot comparison
#
# Usage:
#   scripts\run-all-tests.ps1                  # run all tests
#   scripts\run-all-tests.ps1 -Filter "S1"     # only S1 tests
#   scripts\run-all-tests.ps1 -Verbose         # show test output
#   scripts\run-all-tests.ps1 -SnapshotMode    # create/update snapshots
#   scripts\run-all-tests.ps1 -Backend vm      # specify backend (vm|aot|photon)
# -------------------------------------------------------------
param(
    [switch]$Verbose,
    [string]$Filter = "",
    [switch]$SnapshotMode,
    [string]$Backend = "",
    [string]$AuraExe = ""
)

$ErrorActionPreference = 'Stop'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

# Find aura.exe
if ($AuraExe -ne "" -and (Test-Path $AuraExe)) {
    $exe = $AuraExe
} elseif (Test-Path "build/bin/aura.exe") {
    $exe = "build/bin/aura.exe"
} elseif (Test-Path "aura/seed/aura.exe") {
    $exe = "aura/seed/aura.exe"
} else {
    Write-Host "[run-all-tests] ERROR: aura.exe not found" -ForegroundColor Red
    Write-Host "  Run scripts\build-aura-compiler.ps1 first."
    exit 1
}

$SnapshotDir = "tests/snapshots"
if (-not (Test-Path $SnapshotDir)) { New-Item -ItemType Directory -Path $SnapshotDir -Force | Out-Null }

# Collect test files
$allTests = Get-ChildItem "tests" -Recurse -Filter "*.aura" | 
    Where-Object { $_.Name -notmatch 'Debug|Test_' } |
    Where-Object { $_.FullName -notmatch '\\complier\\' }

if ($Filter -ne "") {
    $tests = $allTests | Where-Object { $_.FullName -match $Filter }
} else {
    $tests = $allTests
}

$passed = 0
$failed = 0
$skipped = 0

Write-Host "=== Running $($tests.Count) tests (aura: $exe) ===" -ForegroundColor Cyan

foreach ($test in $tests) {
    $relPath = $test.FullName.Substring($RootDir.Length + 1)
    
    # Build command
    $args = @("run", $test.FullName)
    if ($Backend -ne "") {
        $args += @("-b", $Backend)
    }
    
    Write-Host "  $relPath" -NoNewline
    
    # Run test
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    $output = (& $exe @args 2>&1 | Out-String).Trim()
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = $prevEap
    
    # Determine snapshot file path
    $testName = $relPath -replace '\\', '-' -replace '\.', '-'
    $snapshotFile = Join-Path $SnapshotDir "$testName.txt"
    
    if ($SnapshotMode) {
        # Create/update snapshot
        Set-Content -Path $snapshotFile -Value $output -Encoding UTF8
        Write-Host " SNAPSHOT" -ForegroundColor Magenta
        $passed++
    } elseif ($exitCode -eq 0) {
        # Compare with snapshot if exists
        if (Test-Path $snapshotFile) {
            $expected = (Get-Content $snapshotFile -Raw).Trim()
            if ($output -eq $expected) {
                Write-Host " PASS" -ForegroundColor Green
                $passed++
            } else {
                Write-Host " FAIL (snapshot mismatch)" -ForegroundColor Yellow
                if ($Verbose) {
                    Write-Host "    --- Expected ---"
                    Write-Host "    $expected"
                    Write-Host "    --- Actual ---"
                    Write-Host "    $output"
                }
                $failed++
            }
        } else {
            Write-Host " PASS (no snapshot)" -ForegroundColor DarkGreen
            $passed++
        }
    } else {
        Write-Host " FAIL (exit=$exitCode)" -ForegroundColor Red
        if ($Verbose) {
            Write-Host "    --- Output ---"
            Write-Host "    $output"
        }
        $failed++
    }
}

Write-Host ""
Write-Host "=== Results: $passed passed, $failed failed, $skipped skipped ===" -ForegroundColor $(if ($failed -gt 0) { "Yellow" } else { "Green" })

if ($failed -gt 0) { exit 1 }