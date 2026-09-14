# run-lang-tests.ps1 - Compile and run all language tests using the self-bootstrapping compiler
# Uses: aura-compiler-selfhost.exe to AOT compile, then run the .exe
param(
    [string]$Compiler = "D:\Code\AuraLang\build\aura-compiler-selfhost.exe",
    [string]$TestDir = "D:\Code\AuraLang\examples\language-test",
    [string]$OutputDir = "D:\Code\AuraLang\build\test\selfhost"
)

New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

$tests = @(
    "01-lexer.aura",
    "02-types-variables.aura",
    "03-functions.aura",
    "04-control-flow.aura",
    "05-classes.aura",
    "06-null-safety.aura",
    "07-error-handling.aura",
    "08-concurrency.aura",
    "10-memory.aura",
    "11-imports.aura",
    "12-annotations.aura",
    "13-stdlib.aura",
    "14-string-interp.aura",
    "15-advanced.aura"
)

$pass = 0; $fail = 0
$results = @()

Write-Host "============================================================"
Write-Host "  Aura Language Tests - Self-Bootstrapping Compiler"
Write-Host "  Compiler: $Compiler"
Write-Host "  Tests:    $($tests.Count)"
Write-Host "============================================================"

foreach ($test in $tests) {
    $inFile = Join-Path $TestDir $test
    $base = $test -replace '\.aura$',''
    $exe = Join-Path $OutputDir "$base.exe"
    
    if (Test-Path $exe) { Remove-Item $exe -Force }
    
    Write-Host ""
    Write-Host "── $test ──" -ForegroundColor Cyan
    
    # Compile
    Write-Host "  [1] Compile..." -NoNewline
    $out = & $Compiler $inFile -o $exe 2>&1
    $exit = $LASTEXITCODE
    if ($exit -ne 0 -or -not (Test-Path $exe)) {
        Write-Host " FAIL" -ForegroundColor Red
        $errs = @($out | Where-Object { $_ -match 'error|Error|panic' } | Select-Object -First 5)
        $errs | ForEach-Object { Write-Host "    $_" -ForegroundColor Yellow }
        $fail++
        $results += [PSCustomObject]@{Test=$test; Compile='FAIL'; Run='SKIP'}
        continue
    }
    Write-Host " OK" -ForegroundColor Green
    
    # Run
    Write-Host "  [2] Run..." -NoNewline
    $runOut = & $exe 2>&1
    $runExit = $LASTEXITCODE
    if ($runExit -eq 0) {
        Write-Host " OK" -ForegroundColor Green
        $pass++
        $results += [PSCustomObject]@{Test=$test; Compile='OK'; Run='OK'}
    } else {
        Write-Host " FAIL (exit $runExit)" -ForegroundColor Red
        $lines = @($runOut)
        if ($lines.Count -gt 0) {
            Write-Host "  Last 3 output lines:" -ForegroundColor Yellow
            $lines | Select-Object -Last 3 | ForEach-Object { Write-Host "    $_" -ForegroundColor Yellow }
        }
        $fail++
        $results += [PSCustomObject]@{Test=$test; Compile='OK'; Run="FAIL($runExit)"}
    }
}

Write-Host ""
Write-Host "============================================================"
Write-Host "  Results: $pass passed, $fail failed / $($tests.Count) total"
Write-Host "============================================================"
$results | Format-Table -AutoSize
