# photon_suite.ps1 — Photon 差分测试跑批（开发期临时工具，位于 build/）
#
# 对指定阶段目录下的每个 .aura：
#   1) 用 VM 跑一遍得到基准 stdout
#   2) aura build -b photon 编译（带超时，超时杀进程树）
#   3) 运行产物 exe（带超时）
#   4) 比较 stdout（与 scripts\photon-e2e-verify.ps1 的判定一致）
#
# 用法:
#   powershell -File build\photon_suite.ps1 -Phase P2
#   powershell -File build\photon_suite.ps1 -Files tests\photon\simple.aura
param(
    [string]$Phase = "",
    [string[]]$Files = @(),
    [int]$TimeoutSecs = 25,
    [string]$OutRoot = "build\suite"
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$env:AURA_PHOTON_DEBUG_HIR = ''

$Aura = Join-Path $Root 'rust\target\release\aura.exe'

function Invoke-Proc($exe, $argStr, $wd, $secs) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $argStr
    $psi.WorkingDirectory = $wd
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $soTask = $p.StandardOutput.ReadToEndAsync()
    $seTask = $p.StandardError.ReadToEndAsync()
    if ($p.WaitForExit($secs * 1000)) {
        return @{ Ok = $true; Out = $soTask.Result; Err = $seTask.Result; Code = $p.ExitCode }
    }
    taskkill /PID $p.Id /T /F 2>$null | Out-Null
    try { $p.Kill() } catch {}
    return @{ Ok = $false; Out = ""; Err = ""; Code = -999 }
}

if ($Phase -ne "") {
    $Files = Get-ChildItem "tests\photon\$Phase" -Filter "*.aura" -File | ForEach-Object { $_.FullName }
}
if ($Files.Count -eq 0) { Write-Host "no files" -ForegroundColor Red; exit 1 }

$pass = 0; $fail = 0
$fails = @()
foreach ($src in $Files) {
    $leaf = Split-Path -Leaf $src
    $stem = [IO.Path]::GetFileNameWithoutExtension($src)
    $outDir = Join-Path $Root (Join-Path $OutRoot $stem)
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $phir = Join-Path $outDir "$stem.phir"

    $vm = Invoke-Proc $Aura "run `"$src`"" $Root $TimeoutSecs
    $vmOut = ("" + $vm.Out) -replace "`r", ""
    $vmOut = $vmOut.Trim()

    $b = Invoke-Proc $Aura "build -b photon `"$src`" --output `"$phir`"" $Root $TimeoutSecs

    $exe = Join-Path $outDir "$stem.exe"
    $exeOut = ""; $code = -1; $tmo = $false
    if (Test-Path $exe) {
        $r = Invoke-Proc $exe "" $outDir $TimeoutSecs
        $exeOut = (("" + $r.Out) -replace "`r", "").Trim()
        $code = $r.Code
        $tmo = -not $r.Ok
    }

    $reasons = @()
    if (-not $b.Ok) { $reasons += "build-timeout" }
    if (-not (Test-Path $exe)) { $reasons += "no-exe" }
    if ($tmo) { $reasons += "run-timeout" }
    if ($vmOut -ne $exeOut) { $reasons += "output-diff" }

    if ($reasons.Count -eq 0) {
        Write-Host ("  PASS  {0}" -f $leaf) -ForegroundColor Green
        $pass++
    } else {
        Write-Host ("  FAIL  {0}  [{1}] exit={2}" -f $leaf, ($reasons -join ","), $code) -ForegroundColor Red
        Write-Host ("        VM : {0}" -f ($vmOut -replace "`n", " | "))
        Write-Host ("        EXE: {0}" -f ($exeOut -replace "`n", " | "))
        $fails += $leaf
        $fail++
    }
}
Write-Host "TOTAL: PASS=$pass FAIL=$fail" -ForegroundColor Cyan
if ($fail -gt 0) { Write-Host ("FAILED: " + ($fails -join ", ")) -ForegroundColor Red }
