# photon-c-chain-diag.ps1 — 用「拼接链探测 + Phase C 探针」定位 Phase C 内存失控点
#
# 一次运行拿到两份证据：
#   1) build/<out>/chain_report.txt — C 运行库在 `aura_string_concat` 上抓到的
#      自指增长链（链长 / log 采样尺寸 / 峰值内容 head-tail / 每 32MiB 快照）。
#   2) build/<out>/isel_probe.log — Phase C 立即落盘的上下文（哪个函数、哪一遍、
#      处理到第几个块）。链超限时 C 侧 exit(71)，其最后几行就是出事的精确位置。
#
# 用法:
#   scripts\photon-c-chain-diag.ps1 [-TripKB 512] [-TimeoutSecs 300] [-MemMB 8192]
param(
    [int]$TripKB = 512,
    [int]$TimeoutSecs = 300,
    [int]$MemMB = 8192,
    [string]$OutDir = 'build\hat-bootstrap',
    [string]$Entry = 'aura\compiler\aura\lang\compiler\Main.aura'
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"

$Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
if (-not (Test-Path $Driver)) { Write-Host "driver missing: $Driver" -ForegroundColor Red; exit 1 }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# 诊断开关
$env:AURA_MEM_LIMIT_MB   = [string]$MemMB
$env:AURA_MEM_STATS      = '1'
$env:AURA_SEL_PROBE      = '1'
$env:AURA_CRASH_DUMP     = '1'
$env:AURA_CHAIN_TRIP_KB  = [string]$TripKB
$env:AURA_CHAIN_REPORT   = Join-Path $Root (Join-Path $OutDir 'chain_report.txt')
$env:AURA_HAT_SKIP_FRONT = '1'
$env:AURA_PHOTON_VERBOSE = '1'
$env:AURA_PHOTON_TRACE   = ''
$env:AURA_PHOTON_DEBUG_HIR = ''
$env:AURA_HAT_AURA   = Join-Path $Root $Entry
$env:AURA_HAT_SRC    = Join-Path $Root (Join-Path $OutDir 'aura-compiler.hat')
$env:AURA_HAT_OUT    = Join-Path $Root $OutDir
$env:AURA_HAT_MODULE = 'aura-compiler'

# 清掉上一轮的证据文件
Remove-Item (Join-Path $Root $env:AURA_CHAIN_REPORT) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'isel_probe.log')) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'xe_probe.log')) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'ra_probe.log')) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'pe_probe.log')) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'objw_probe.log')) -Force -ErrorAction SilentlyContinue
Remove-Item (Join-Path $Root (Join-Path $OutDir 'photon_trace.log')) -Force -ErrorAction SilentlyContinue

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Driver
$psi.WorkingDirectory = $Root
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError  = $true
$psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$soT = $p.StandardOutput.ReadToEndAsync()
$seT = $p.StandardError.ReadToEndAsync()

$t0 = Get-Date
$timedOut = $false
$rssPeak = 0
$rssLast = 0
$rssHist = New-Object System.Collections.Generic.List[string]
while (-not $p.WaitForExit(5000)) {
    try {
        $p.Refresh()
        $ws = $p.WorkingSet64
        if ($ws -gt $rssPeak) { $rssPeak = $ws }
        $rssLast = $ws
        $rssHist.Add(("{0,6:N0} MB @ {1,6:N0}s" -f ($ws / 1MB), ((Get-Date) - $t0).TotalSeconds))
    } catch {}
    if (((Get-Date) - $t0).TotalSeconds -gt $TimeoutSecs) {
        Write-Host "TIMEOUT -> kill" -ForegroundColor Yellow
        try { $p.Kill() } catch {}
        $timedOut = $true
        break
    }
}
$sw = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1)
$code = -1
try { $code = $p.ExitCode } catch {}

$so = ''; $se = ''
try { $so = $soT.Result } catch {}
try { $se = $seT.Result } catch {}

$env:AURA_MEM_LIMIT_MB=''; $env:AURA_MEM_STATS=''; $env:AURA_SEL_PROBE=''
$env:AURA_CHAIN_TRIP_KB=''; $env:AURA_CHAIN_REPORT=''
$env:AURA_HAT_SKIP_FRONT=''; $env:AURA_PHOTON_VERBOSE=''; $env:AURA_PHOTON_TRACE=''
$env:AURA_PHOTON_DEBUG_HIR=''; $env:AURA_HAT_AURA=''; $env:AURA_HAT_SRC=''
$env:AURA_HAT_OUT=''; $env:AURA_HAT_MODULE=''

Write-Host ""
Write-Host "================ DIAG ================" -ForegroundColor Cyan
Write-Host ("wall     : {0}s" -f $sw)
Write-Host ("exit code: {0}" -f $code)
Write-Host ("rss peak : {0} MB" -f [Math]::Round($rssPeak/1MB,1))
Write-Host ("rss last : {0} MB" -f [Math]::Round($rssLast/1MB,1))
Write-Host ("driver peak ws: {0} MB" -f [Math]::Round($p.PeakWorkingSet64/1MB,1))
if ($timedOut) { Write-Host "TIMEOUT" -ForegroundColor Yellow }
Write-Host "--------------------------------------"
Write-Host ("rss history (sampled every 5s, {0} samples):" -f $rssHist.Count)
$show = @()
if ($rssHist.Count -le 24) { $show = $rssHist }
else {
    $step = [Math]::Ceiling($rssHist.Count / 24.0)
    for ($k = 0; $k -lt $rssHist.Count; $k += $step) { $show.Add($rssHist[$k]) }
    $show.Add($rssHist[$rssHist.Count - 1])
}
$show | ForEach-Object { Write-Host "  $_" }
Write-Host "======================================"
Write-Host ""
Write-Host "--- stderr (tail 45) ---"
$sl = $se -split "`r?`n"
Write-Host ("> {0} lines" -f $sl.Count)
$sl | Select-Object -Last 45 | ForEach-Object { Write-Host $_ }
Write-Host ""

foreach ($f in @('chain_report.txt','isel_probe.log','ra_probe.log','pe_probe.log','objw_probe.log','xe_probe.log','photon_trace.log')) {
    $fp = Join-Path $Root (Join-Path $OutDir $f)
    Write-Host ("--- {0} ---" -f $f)
    if (Test-Path $fp) {
        $txt = Get-Content $fp
        $total = $txt.Count
        $bytes = (Get-Item $fp).Length
        Write-Host ("> {0} lines, {1} bytes" -f $total, $bytes)
        $head = 18; $tail = 45
        if ($total -le ($head + $tail + 10)) {
            $txt | ForEach-Object { Write-Host $_ }
        } else {
            $txt | Select-Object -First $head | ForEach-Object { Write-Host $_ }
            Write-Host ("  ... (skipped {0} lines) ..." -f ($total - $head - $tail))
            $txt | Select-Object -Last $tail | ForEach-Object { Write-Host $_ }
        }
    } else {
        Write-Host "(missing)"
    }
    Write-Host ""
}
