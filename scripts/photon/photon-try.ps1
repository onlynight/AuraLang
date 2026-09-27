# photon_try.ps1 — 带超时的 Photon 编译/运行辅助脚本（开发期临时工具，位于 build/）
#
# 用法:
#   powershell -File build\photon_try.ps1 -Src tests\photon\P1\04_control_flow.aura -Out build\p1\04\04.phir
#   powershell -File build\photon_try.ps1 -Src ... -Out ... -Debug -Grep "^while"
#
# 行为:
#   aura build -b photon <Src> --output <Out>   (默认 30s 超时，超时杀进程树)
#   -Debug 时设置 AURA_PHOTON_DEBUG_HIR=1
param(
    [Parameter(Mandatory=$true)][string]$Src,
    [Parameter(Mandatory=$true)][string]$Out,
    [int]$TimeoutSecs = 30,
    [string[]]$Grep = @(),
    [switch]$Dbg
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
if ($Dbg) { $env:AURA_PHOTON_DEBUG_HIR = '1' } else { $env:AURA_PHOTON_DEBUG_HIR = '' }

$outDir = Split-Path -Parent $Out
if ($outDir -and -not (Test-Path $outDir)) { New-Item -ItemType Directory -Force -Path $outDir | Out-Null }
$log = Join-Path $Root ($Out + '.build.log')

$a = @('build', '-b', 'photon', $Src, '--output', $Out)
$p = Start-Process -FilePath (Join-Path $Root 'rust\target\release\aura.exe') `
    -ArgumentList $a -NoNewWindow -PassThru `
    -RedirectStandardOutput $log -RedirectStandardError ($log + '.err')

$done = $p.WaitForExit($TimeoutSecs * 1000)
if (-not $done) {
    taskkill /PID $p.Id /T /F 2>$null | Out-Null
    Write-Host "*** TIMEOUT after ${TimeoutSecs}s (killed) ***" -ForegroundColor Red
} else {
    Write-Host "exit=$($p.ExitCode)"
}

if (Test-Path $log) {
    if ($Grep.Count -gt 0) {
        Get-Content $log | Select-String -Pattern $Grep | ForEach-Object { $_.Line }
    } else {
        Get-Content $log | Select-Object -Last 40
    }
}
