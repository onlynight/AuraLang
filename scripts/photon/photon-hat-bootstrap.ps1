# photon-hat-bootstrap.ps1 — 用 Photon HAT 独立链路编译 Aura 自举编译器
#
#   Main.aura(含递归 import) ──[原生 PhotonHatCompile.exe]──► HIR ──► SSA ──►
#   .hat ──► LIR ──► DAG ──► RegAlloc ──► X86 ──► COFF ──► lld-link ──► exe
#
# 全程**不生成、不读取 .phir**（HAT 文本是唯一中间表示），前端与后端都在原生
# 运行时下执行。本脚本只负责跑一次并采集两项指标：
#   1) 编译总时长 —— 驱动进程从 Start 到 Exit 的墙钟时间；
#   2) 编译期峰值内存 ——
#        · 驱动进程自身：读 `Process.PeakWorkingSet64`（系统记录的进程最高水位，
#          不依赖采样频率，最可靠）；
#        · 子进程（llc / clang / lld-link）：按名字 + 父子关系每 500ms 采样一次
#          工作集，取历史最大值（这些是短命进程，采样是唯一手段）。
#
# 用法:
#   powershell -File scripts\photon-hat-bootstrap.ps1
#   powershell -File scripts\photon-hat-bootstrap.ps1 -Rebuild -TimeoutSecs 3600
param(
    [int]$TimeoutSecs = 1800,
    [switch]$Rebuild,
    [string]$Entry = "",
    [string]$Driver = "",
    [string]$OutDir = "build\hat-bootstrap"
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$env:AURA_PHOTON_DEBUG_HIR = ''
$env:AURA_PHOTON_TRACE = ''

$Aura = Join-Path $Root 'rust\target\release\aura.exe'
if (-not $Entry)  { $Entry  = Join-Path $Root 'aura\compiler\aura\lang\compiler\Main.aura' }
if (-not $Driver) { $Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe' }
if (-not (Test-Path $Entry)) { Write-Host "entry not found: $Entry" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $Aura))  { Write-Host "aura.exe not found: $Aura" -ForegroundColor Red; exit 1 }

if ($Rebuild -or -not (Test-Path $Driver)) {
    $src = Join-Path $Root 'aura\compiler\aura\lang\compiler\backend\photon\PhotonHatCompile.aura'
    Write-Host "[build] AOT 构建原生 HAT 驱动 ..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path (Split-Path $Driver) | Out-Null
    $t = Get-Date
    & $Aura build --aot $src --output $Driver 2>&1 | Select-String -Pattern 'complete|failed|error' | Select-Object -Last 3
    Write-Host ("[build] driver ready ({0}s)" -f [Math]::Round(((Get-Date) - $t).TotalSeconds, 1))
}
if (-not (Test-Path $Driver)) { Write-Host "driver not built: $Driver" -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$hat = Join-Path $OutDir 'aura-compiler.hat'
$exe = Join-Path $OutDir 'aura-compiler.exe'

$env:AURA_HAT_AURA   = $Entry
$env:AURA_HAT_SRC    = $hat
$env:AURA_HAT_OUT    = $OutDir
$env:AURA_HAT_MODULE = 'aura-compiler'

# 被采样的子进程名（编译器管线可能调用的外部工具）
$ChildNames = @('llc','clang','lld-link','lld-link.exe','link','cmake')

# ── 子进程工作集采样 ────────────────────────────────────────────
# 只按名字取进程（不做全系统枚举），再用「父进程链」确认它属于驱动进程，
# 从而避免把机器上其它同名进程算进来。
$childParent = @{}
function Get-TreePids([int]$root) {
    $ids = New-Object 'System.Collections.Generic.List[int]'
    $ids.Add($root)
    $changed = $true
    while ($changed) {
        $changed = $false
        foreach ($kv in $childParent.GetEnumerator()) {
            if ($kv.Value -gt 0 -and $ids.Contains($kv.Value) -and -not $ids.Contains([int]$kv.Key)) {
                $ids.Add([int]$kv.Key)
                $changed = $true
            }
        }
    }
    return $ids
}

Write-Host ""
Write-Host "=== Photon HAT 编译 Aura 自举编译器 ===" -ForegroundColor Cyan
Write-Host ("  entry  : {0}" -f $Entry)
Write-Host ("  driver : {0}" -f $Driver)
Write-Host ("  out    : {0}" -f $OutDir)
Write-Host ("  timeout: {0}s" -f $TimeoutSecs)
Write-Host ""

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $Driver
$psi.WorkingDirectory = $Root
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError  = $true
$psi.UseShellExecute = $false
$proc = [System.Diagnostics.Process]::Start($psi)
$soTask = $proc.StandardOutput.ReadToEndAsync()
$seTask = $proc.StandardError.ReadToEndAsync()
$rootPid = $proc.Id

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$peakRoot = [long]0; $peakChildSum = [long]0; $peakChildOne = [long]0
$childDetail = ''; $samples = 0
$timedOut = $false
while (-not $proc.WaitForExit(500)) {
    if ($sw.Elapsed.TotalSeconds -gt $TimeoutSecs) {
        try { $proc.Kill() } catch {}
        $timedOut = $true
        break
    }
    $samples++
    try {
        $rows = Get-Process -Name $ChildNames -ErrorAction SilentlyContinue |
            Select-Object Id, ProcessName, Parent, WorkingSet64
        foreach ($r in $rows) {
            if ($r.Parent -ne $null -and $r.Parent -ne '') {
                $childParent[[int]$r.Id] = [int]$r.Parent
            }
        }
        $tree = @(Get-TreePids $rootPid)
        $sum = [long]0; $one = [long]0; $names = @()
        foreach ($r in $rows) {
            if ([int]$r.Id -notin $tree) { continue }
            $ws = [long]$r.WorkingSet64
            if ($ws -gt 0) {
                $sum += $ws
                if ($ws -gt $one) { $one = $ws }
                $names += ("{0}={1}MB" -f $r.ProcessName, [Math]::Round($ws / 1MB, 0))
            }
        }
        if ($sum -gt $peakChildSum) { $peakChildSum = $sum; $childDetail = ($names -join ', ') }
        if ($one -gt $peakChildOne) { $peakChildOne = $one }
    } catch { }
}
$sw.Stop()
$elapsed = $sw.Elapsed

# 驱动进程自身：读系统记录的峰值（进程已退出也能读到）
$driverPeak = [long]0
try { $driverPeak = [long]$proc.PeakWorkingSet64 } catch {}
$driverPriv = [long]0
try { $driverPriv = [long]$proc.PeakPagedPoolAllocated } catch {}

$so = ''; $se = ''
try { $so = $soTask.Result } catch {}
try { $se = $seTask.Result } catch {}
$code = -1
try { $code = $proc.ExitCode } catch {}

$env:AURA_HAT_AURA = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''

# ── 结果解析 ──
$result = ''; $errMsg = ''; $lastMarker = ''
foreach ($l in (($so + "`n" + $se) -split "`n")) {
    $t = $l.TrimEnd("`r")
    if ($t -eq '===COFF-MAIN===' -or $t -eq '===COFF-RUNTIME===' -or $t -eq '===LINK===') { continue }
    if ($t.StartsWith('===RESULT===')) { $result = $t.Substring(12).Trim(); continue }
    if ($t.StartsWith('===ERR==='))    { $errMsg = $t.Substring(8).Trim(); $lastMarker = 'err'; continue }
    if ($t.StartsWith('[hat-front]'))  { Write-Host ("  {0}" -f $t.Trim()) -ForegroundColor DarkCyan; continue }
    if ($t -ne '' -and $lastMarker -ne 'err') { $lastMarker = 'other' }
}

Write-Host ""
Write-Host "─── 指标 ─────────────────────────────────────────" -ForegroundColor Cyan
Write-Host ("  编译总时长                  : {0}" -f $elapsed.ToString("hh\:mm\:ss\.ff"))
Write-Host ("  峰值内存 · 驱动进程自身      : {0} MB   (PeakWorkingSet64)" -f [Math]::Round($driverPeak / 1MB, 1))
Write-Host ("  峰值内存 · 子进程最大单项    : {0} MB" -f [Math]::Round($peakChildOne / 1MB, 1))
Write-Host ("  峰值内存 · 子进程同刻总和    : {0} MB   {1}" -f [Math]::Round($peakChildSum / 1MB, 1), $childDetail)
Write-Host ("  采样次数(子进程, 500ms 一次) : {0}" -f $samples)
Write-Host ""
Write-Host ("  exit code     : {0}" -f $code)
Write-Host ("  ===RESULT===  : {0}" -f $result)
if ($timedOut) { Write-Host ("  TIMEOUT       : {0}s" -f $TimeoutSecs) -ForegroundColor Yellow }
Write-Host ("  .hat          : {0}" -f $(if (Test-Path $hat) { "$hat  ($((Get-Item $hat).Length) bytes)" } else { 'MISSING' }))
Write-Host ("  .exe          : {0}" -f $(if (Test-Path $exe) { "$exe  ($((Get-Item $exe).Length) bytes)" } else { '未生成（驱动只输出 COFF hex，不自行链接）' }))
if ($errMsg -ne '') { Write-Host ("  ===ERR===     : {0}" -f $errMsg) -ForegroundColor Red }
Write-Host ""

if (-not $timedOut -and $result -eq 'success' -and $code -eq 0) { exit 0 }
exit 1
