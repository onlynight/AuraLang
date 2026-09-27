# run-hat-on-main.ps1 — 直接跑一次「Photon HAT 驱动编译 Main.aura」并采集指标
param(
    [int]$BudgetSecs = 900,
    [int]$SampleSecs = 10,
    [string]$Entry = 'D:\Code\AuraLang\aura\compiler\aura\lang\compiler\Main.aura',
    [string]$Module = 'aura-compiler',
    [string]$HatName = 'aura-compiler.hat',
    # 打开阶段进度/心跳（[hat-front] / [Phase A-E] / [isa] / [hat-parse]）。
    [switch]$Verbose,
    # 节点级崩溃诊断 + <out>/photon_trace.log 落盘（隐含 -Verbose）。
    [switch]$Trace,
    [switch]$PerFn
)
$ErrorActionPreference = 'Continue'
$Root = 'D:\Code\AuraLang'
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$env:AURA_PHOTON_DEBUG_HIR = ''
if ($Trace) { $env:AURA_PHOTON_TRACE = '1' } else { $env:AURA_PHOTON_TRACE = '' }
if ($PerFn) { $env:AURA_SSA_PERFN = '1' } else { $env:AURA_SSA_PERFN = '' }
# 阶段进度/心跳默认关：驱动 stdout 只留 ===...=== 协议标记 + METRICS 段。
# Trace 隐含 Verbose（节点级诊断时自然也要看阶段进度）。
if ($Verbose -or $Trace) {
    $env:AURA_PHOTON_VERBOSE = '1'; $env:AURA_HAT_TRACE = '1'
} else {
    $env:AURA_PHOTON_VERBOSE = '';  $env:AURA_HAT_TRACE = ''
}

$Entry  = $Entry
$Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
$OutDir = Join-Path $Root 'build\hat-bootstrap'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$env:AURA_HAT_AURA   = $Entry
$env:AURA_HAT_SRC    = Join-Path $OutDir $HatName
$env:AURA_HAT_OUT    = $OutDir
$env:AURA_HAT_MODULE = $Module

$t0 = Get-Date
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Driver
$psi.WorkingDirectory = $Root
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError  = $true
$psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$soT = $p.StandardOutput.ReadToEndAsync()
$seT = $p.StandardError.ReadToEndAsync()

$timedOut = $false
$lastCpu = 0.0
$peakChildWs = 0; $peakChildName = ''; $peakChildPid = 0
$childNames = @('llc.exe','llc','clang.exe','clang','link.exe','link','lld-link.exe','lld-link')
while (-not $p.WaitForExit($SampleSecs * 1000)) {
    $cpu = 0.0; $ws = 0
    try { $cpu = $p.TotalProcessorTime.TotalSeconds } catch {}
    try { $ws  = $p.WorkingSet64 } catch {}
    $age = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 0)
    Write-Host ("t={0}s  driver ws={1}MB cpu={2}s (last={3}s)  exit?={4}" -f $age, [Math]::Round($ws/1MB,0), [Math]::Round($cpu,1), [Math]::Round($lastCpu,1), $p.ExitCode)
    $lastCpu = $cpu
    try {
        $children = Get-Process -Name $childNames -ErrorAction SilentlyContinue
        foreach ($c in $children) {
            $cws = $c.WorkingSet64
            if ($cws -gt $peakChildWs) { $peakChildWs = $cws; $peakChildName = $c.ProcessName; $peakChildPid = $c.Id }
            Write-Host ("   child {0} pid={1} ws={2}MB" -f $c.ProcessName, $c.Id, [Math]::Round($cws/1MB,0))
        }
    } catch {}
    if ($age -gt $BudgetSecs) {
        Write-Host "BUDGET EXCEEDED -> kill"
        try { $p.Kill() } catch {}
        $timedOut = $true
        break
    }
}
$sw = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 1)

$so = ''; $se = ''
try { $so = $soT.Result } catch {}
try { $se = $seT.Result } catch {}
$code = -1
try { $code = $p.ExitCode } catch {}

$env:AURA_HAT_AURA = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''

Write-Host ""
Write-Host "================ METRICS ================"
Write-Host ("wall clock        : {0}s" -f $sw)
Write-Host ("peak working set  : {0} MB   (PeakWorkingSet64, driver itself)" -f [Math]::Round($p.PeakWorkingSet64/1MB,1))
if ($peakChildWs -gt 0) {
    Write-Host ("peak child proc   : {0} MB   ({1}, pid {2}, sampled)" -f [Math]::Round($peakChildWs/1MB,1), $peakChildName, $peakChildPid)
}
Write-Host ("exit code         : {0}" -f $code)
if ($timedOut) { Write-Host ("TIMEOUT at {0}s" -f $BudgetSecs) }
Write-Host "========================================="
Write-Host ""

$FullLog = Join-Path $OutDir ($Module + '.hat-full.log')
[System.IO.File]::WriteAllText($FullLog, $so + "`n--- stderr ---`n" + $se)
Write-Host ("full log -> {0} ({1} chars)" -f $FullLog, ($so.Length + $se.Length))

$Lines = $so -split "`r?`n"
$n = $Lines.Count
Write-Host ("stdout lines: {0}" -f $n)
$Head = 60; $Tail = 60
if ($n -le ($Head + $Tail + 5)) {
    Write-Host $so
} else {
    Write-Host "--- stdout HEAD ({0} lines) ---" -f $Head
    $Lines[0..($Head-1)] | ForEach-Object { Write-Host $_ }
    Write-Host "... (skipped {0} lines) ..." -f ($n - $Head - $Tail)
    Write-Host "--- stdout TAIL ({0} lines) ---" -f $Tail
    $Lines[($n-$Tail)..($n-1)] | ForEach-Object { Write-Host $_ }
}
Write-Host ""
Write-Host "--- markers ---"
foreach ($m in @('===RESULT===','===LINK===','===COFF-','ERROR','error','undefined symbol')) {
    $cnt = ($so | Select-String -SimpleMatch $m | Measure-Object).Count
    Write-Host ("  {0,-20} x{1}" -f $m, $cnt)
}
if ($se.Trim() -ne '') {
    $sl = $se -split "`r?`n"
    Write-Host "--- stderr ({0} lines) ---" -f $sl.Count
    if ($sl.Count -gt 80) { $sl | Select-Object -First 80 | ForEach-Object { Write-Host $_ }; Write-Host "... (truncated)" }
    else { $sl | ForEach-Object { Write-Host $_ } }
}
