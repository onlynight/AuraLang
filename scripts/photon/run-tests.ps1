# Test runner using cmd.exe to capture output
param([string]$Label = "test")
$RootDir = 'D:\Code\AuraLang'
Set-Location $RootDir
$exe = "rust\target\release\aura.exe"

$tests = Get-ChildItem tests -Recurse -Filter "*.aura" |
    Where-Object { $_.Name -notmatch 'Debug|Test_' } |
    Where-Object { $_.FullName -notmatch '\\photon\\' } |
    Where-Object { $_.FullName -notmatch '\\complier\\' }

$passed = 0
$failed = 0
$failedList = @()
$unresolved = @{}
$totalUnresolved = 0

foreach ($t in $tests) {
    $rel = $t.FullName.Substring($RootDir.Length + 1)
    # Use cmd.exe to capture raw output
    $cmdLine = "`"$exe`" run `"$($t.FullName)`" 2>&1"
    $result = & cmd.exe /c $cmdLine
    $code = $LASTEXITCODE
    $out = $result -join "`n"

    $matches = [regex]::Matches($out, "未解析的函数调用 '([^']+)'")
    foreach ($m in $matches) {
        $name = $m.Groups[1].Value
        if (-not $unresolved.ContainsKey($name)) { $unresolved[$name] = 0 }
        $unresolved[$name]++
        $totalUnresolved++
    }

    if ($code -eq 0) { $passed++ } else { $failed++; $failedList += $rel }
}

Write-Host "=== [$Label] $passed OK / $failed FAIL (of $($tests.Count)) ===" -ForegroundColor Cyan
Write-Host "=== [$Label] total unresolved call points: $totalUnresolved ===" -ForegroundColor Cyan
if ($unresolved.Count -gt 0) {
    Write-Host "--- Top unresolved names ---"
    $unresolved.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 40 |
        ForEach-Object { Write-Host ("  {0}: {1}" -f $_.Name, $_.Value) }
}
Write-Host "=== [$Label] failed tests ===" -ForegroundColor Yellow
$failedList | ForEach-Object { Write-Host "  $_" }
