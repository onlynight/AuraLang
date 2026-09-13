# Language test runner using AOT native compiler
param(
    [switch]$SkipBuild,
    [string]$CompilerExe = "target\release\aura.exe"
)

$ErrorActionPreference = "Continue"
$ProjectRoot = Split-Path $PSScriptRoot -Parent
$CompilerPath = Join-Path $ProjectRoot $CompilerExe
$TestDir = Join-Path $ProjectRoot "examples\language-test"
$OutDir = Join-Path $ProjectRoot "build\bin\lang-test"

if (!(Test-Path $CompilerPath)) {
    Write-Host "ERROR: Compiler not found at $CompilerPath" -ForegroundColor Red
    exit 1
}

if (!(Test-Path $OutDir)) {
    New-Item -ItemType Directory -Path $OutDir -Force | Out-Null
}

# Collect all .aura test files (exclude libs and target subdirs)
$testFiles = Get-ChildItem -Path $TestDir -Filter "*.aura" -Recurse | Where-Object {
    $_.FullName -notmatch "\\libs\\|\\target\\"
} | Sort-Object Name

Write-Host ""
Write-Host "Running Language Tests with AOT Compiler" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Compiler: $CompilerPath" -ForegroundColor Gray
Write-Host "Test files: $($testFiles.Count)" -ForegroundColor Gray
Write-Host ""

$passCount = 0
$failCount = 0
$failures = @()

foreach ($file in $testFiles) {
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($file.Name)
    $outputExe = Join-Path $OutDir "$baseName.exe"
    
    Write-Host -NoNewline "[$baseName] "
    
    # Remove old output
    if (Test-Path $outputExe) {
        Remove-Item $outputExe -Force -ErrorAction SilentlyContinue
    }
    
    # Run AOT compilation
    $output = & $CompilerPath build $file.FullName --aot --output $outputExe 2>&1
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0 -and (Test-Path $outputExe)) {
        Write-Host "PASS" -ForegroundColor Green
        $passCount++
    } else {
        Write-Host "FAIL" -ForegroundColor Red
        $failCount++
        $errLine = ($output | Where-Object { $_ -match "error:|Error:" } | Select-Object -First 3) -join "; "
        $failures += [PSCustomObject]@{
            File = $file.Name
            Errors = $errLine
        }
        if ($errLine) {
            Write-Host "  -> $errLine" -ForegroundColor Yellow
        }
    }
}

Write-Host ""
Write-Host "===========================================" -ForegroundColor Cyan
Write-Host "Results: $passCount passed, $failCount failed" -ForegroundColor $(if ($failCount -eq 0) { "Green" } else { "Yellow" })

if ($failCount -gt 0) {
    Write-Host ""
    Write-Host "Failed tests:" -ForegroundColor Red
    foreach ($f in $failures) {
        Write-Host "  $ ($f.File)" -ForegroundColor Yellow
        if ($f.Errors) {
            Write-Host "    $ ($f.Errors)" -ForegroundColor DarkYellow
        }
    }
}

exit 0