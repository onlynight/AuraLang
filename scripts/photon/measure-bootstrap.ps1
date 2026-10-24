# ============================================================
# measure-bootstrap.ps1 - Photon AOT compile of the Aura
# self-hosting compiler: wall-clock + phase timing + memory
# ============================================================
#
# Measures ONE full run of the Photon HAT driver on the
# self-hosting compiler entry (aura/compiler/.../Main.aura):
#
#   time    : process wall clock + driver-internal [time] phases
#             (AURA_PHOTON_TIME=1 -> [time] link/ssa/hat/compile)
#   memory  : 50 ms sampling of WorkingSet64 / PrivateMemorySize64,
#             peak + full timeline + peak child (lld-link) working set
#
# Artifacts land in $OutDir:
#   run-stdout.log / run-stderr.log   full driver output (incl COFF hex)
#   timeline.csv                     t_ms,ws_bytes,priv_bytes per sample
#   summary.json                     machine-readable result
#
# Usage:
#   powershell -File scripts\photon\measure-bootstrap.ps1
#   powershell -File scripts\photon\measure-bootstrap.ps1 -OutDir build\hat-bootstrap-measure2
# ============================================================
param(
    [string]$Entry  = "aura\compiler\aura\lang\compiler\Main.aura",
    [string]$Driver = "build\hat-native\PhotonHatCompile.exe",
    [string]$OutDir = "build\hat-bootstrap-measure",
    [int]$SampleMs  = 50,
    [int]$TimeoutSecs = 3600,
    [int]$MemLimitMB = 16000,
    [int]$Repeat   = 1
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

# [R102] -Repeat N: run the whole measurement N times into $OutDir\run1..runN
# and print a median summary. Rationale: run-to-run noise on this machine is
# about +-5% (observed 312.6 s vs 319.1 s on the same binary, plus one 360.7 s
# outlier on identical code), so single-run deltas below that are meaningless.
if ($Repeat -gt 1) {
    $results = @()
    for ($i = 1; $i -le $Repeat; $i++) {
        $sub = Join-Path $OutDir ("run" + $i)
        Write-Host ("=== repeat " + $i + "/" + $Repeat + " -> " + $sub + " ===") -ForegroundColor Cyan
        & $PSCommandPath -Entry $Entry -Driver $Driver -OutDir $sub -SampleMs $SampleMs `
            -TimeoutSecs $TimeoutSecs -MemLimitMB $MemLimitMB -Repeat 1
        $sj = Join-Path $sub "summary.json"
        if (Test-Path $sj) {
            $o = Get-Content $sj -Raw | ConvertFrom-Json
            $results += [pscustomobject]@{ run = $i; wall = [double]($o.wall_ms / 1000.0); peakMB = [math]::Round($o.peak_priv_bytes/1MB,1); result = $o.result }
        }
    }
    $walls = $results | ForEach-Object { $_.wall } | Sort-Object
    $med = $walls[[int][math]::Floor($walls.Count / 2)]
    Write-Host ""
    Write-Host "==================== REPEAT SUMMARY ====================" -ForegroundColor Yellow
    $results | Format-Table -AutoSize
    Write-Host ("median wall = {0:N3} s   min = {1:N3} s   max = {2:N3} s" -f $med, $walls[0], $walls[$walls.Count-1])
    Write-Host "=======================================================" -ForegroundColor Yellow
    exit 0
}

$LLVMbin = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin"
$env:Path = "$LLVMbin;$env:Path"

$EntryAbs = (Resolve-Path $Entry).Path
$DriverAbs = (Resolve-Path $Driver).Path
if (-not (Test-Path $EntryAbs))  { Write-Host "entry not found: $EntryAbs" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $DriverAbs)) { Write-Host "driver not found: $DriverAbs" -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutAbs = (Resolve-Path $OutDir).Path
$hat = Join-Path $OutAbs "aura-compiler.hat"

# ---- driver environment (HAT channel) ----
$env:AURA_HAT_AURA   = $EntryAbs
$env:AURA_HAT_SRC    = $hat
$env:AURA_HAT_OUT    = $OutAbs
$env:AURA_HAT_MODULE = "aura-compiler"
$env:AURA_PHOTON_TIME = "1"   # internal per-phase ns timing -> [time] lines
# Self-bootstrap frontend peak ~12GB (SSA) + backend ~12GB; default 8GB guard is too low.
if ($MemLimitMB -gt 0) { $env:AURA_MEM_LIMIT_MB = "$MemLimitMB" }

$childNames = @('lld-link','lld-link.exe','clang','clang.exe','llc','llc.exe')

Write-Host ""
Write-Host "=== Photon AOT compile: Aura self-hosting compiler ===" -ForegroundColor Cyan
Write-Host ("  entry   : {0}" -f $EntryAbs)
Write-Host ("  driver  : {0}  ({1} B, mtime {2})" -f $DriverAbs, (Get-Item $DriverAbs).Length, (Get-Item $DriverAbs).LastWriteTime.ToString("yyyy-MM-dd HH:mm"))
Write-Host ("  out     : {0}" -f $OutAbs)
Write-Host ("  sample  : every {0} ms, timeout {1} s" -f $SampleMs, $TimeoutSecs)
Write-Host ""

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $DriverAbs
$psi.WorkingDirectory = $Root
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError  = $true
$psi.UseShellExecute = $false

$proc = [System.Diagnostics.Process]::Start($psi)
# MUST read both pipes asynchronously (driver writes a lot to stderr;
# sequential ReadToEnd would deadlock on the 4 KB pipe buffer).
$soTask = $proc.StandardOutput.ReadToEndAsync()
$seTask = $proc.StandardError.ReadToEndAsync()

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$peakWs = [long]0; $peakPriv = [long]0
$peakWsAt = 0;   $peakPrivAt = 0
$peakChildWs = [long]0
$tlw = New-Object System.IO.StreamWriter((Join-Path $OutAbs "timeline.csv"), $false)
$tlw.WriteLine("t_ms,ws_bytes,priv_bytes")
$sampleCount = 0
$timedOut = $false
$lastModLine = ""

while (-not $proc.WaitForExit($SampleMs)) {
    if ($sw.Elapsed.TotalSeconds -gt $TimeoutSecs) {
        try { $proc.Kill() } catch {}
        $timedOut = $true
        break
    }
    try {
        $proc.Refresh()
        $ws = [long]$proc.WorkingSet64
        $pv = [long]$proc.PrivateMemorySize64
        $t  = [int]$sw.Elapsed.TotalMilliseconds
        if ($ws -gt $peakWs)   { $peakWs = $ws;     $peakWsAt = $t }
        if ($pv -gt $peakPriv) { $peakPriv = $pv;   $peakPrivAt = $t }
        $tlw.WriteLine(("{0},{1},{2}" -f $t, $ws, $pv))
        $sampleCount++
        if (($sampleCount % 20) -eq 0) { $tlw.Flush() }   # flush every 1s so long runs are observable
        foreach ($cn in $childNames) {
            $kids = Get-Process -Name $cn -ErrorAction SilentlyContinue
            foreach ($k in $kids) {
                try { $k.Refresh(); $kws = [long]$k.WorkingSet64 } catch { $kws = 0 }
                if ($kws -gt $peakChildWs) { $peakChildWs = $kws }
            }
        }
    } catch {}
}
$sw.Stop()
$elapsed = $sw.Elapsed
$tlw.Flush(); $tlw.Close()

$driverPeakWs = [long]0
try { $driverPeakWs = [long]$proc.PeakWorkingSet64 } catch {}
$code = -1
try { $code = $proc.ExitCode } catch {}

$so = ''; $se = ''
try { $so = $soTask.Result } catch {}
try { $se = $seTask.Result } catch {}

$env:AURA_HAT_AURA = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''; $env:AURA_PHOTON_TIME = ''

# ---- persist raw output ----
$so | Out-File (Join-Path $OutAbs "run-stdout.log") -Encoding utf8
$se | Out-File (Join-Path $OutAbs "run-stderr.log") -Encoding utf8
# timeline.csv is written incrementally by the sampling loop above

# ---- parse protocol markers / phases ----
$result = ''; $errMsg = ''
$timeLines = @()
$modCount = 0
foreach ($l in (($so + "`n" + $se) -split "`n")) {
    $t = $l.TrimEnd("`r")
    if ($t.StartsWith('===RESULT===')) { $result = $t.Substring(12).Trim(); continue }
    if ($t.StartsWith('===ERR==='))    { $errMsg  = $t.Substring(8).Trim();  continue }
    if ($t -match '^\[time\]')         { $timeLines += $t.Trim(); continue }
    if ($t -like '[mod] nodes=*')      { $modCount++; $lastModLine = $t.Trim() }
}

function Sz($b) { if ($b -ge 1GB) { "{0:N2} GB" -f ($b/1GB) } elseif ($b -ge 1MB) { "{0:N1} MB" -f ($b/1MB) } else { "{0:N0} B" -f $b } }

$hatSize = if (Test-Path $hat) { (Get-Item $hat).Length } else { 0 }
$objMain = Join-Path $OutAbs "aura-compiler.obj"
$objRt   = Join-Path $OutAbs "aura_runtime.obj"
$exe     = Join-Path $OutAbs "aura-compiler.exe"

Write-Host ""
Write-Host "===================== RESULT =====================" -ForegroundColor Cyan
Write-Host ("  wall clock            : {0:hh\:mm\:ss\.fff}" -f $elapsed)
Write-Host ("  exit code             : {0}" -f $code)
Write-Host ("  ===RESULT===          : {0}" -f $result)
if ($errMsg) { Write-Host ("  ===ERR===             : {0}" -f $errMsg) -ForegroundColor Red }
Write-Host ("  peak working set      : {0}  (@ t={1} ms)" -f (Sz $peakWs), $peakWsAt)
Write-Host ("  peak private memory   : {0}  (@ t={1} ms)" -f (Sz $peakPriv), $peakPrivAt)
Write-Host ("  driver PeakWorkingSet : {0}" -f (Sz $driverPeakWs))
Write-Host ("  peak child WS (linker): {0}" -f (Sz $peakChildWs))
Write-Host ("  memory samples        : {0} @ {1} ms" -f $sampleCount, $SampleMs)
Write-Host ("  modules linked ([mod]): {0}   last: {1}" -f $modCount, $lastModLine)
Write-Host ("  .hat                  : {0}" -f $(if ($hatSize -gt 0) { Sz $hatSize } else { "MISSING" }))
Write-Host ("  main .obj             : {0}" -f $(if (Test-Path $objMain) { Sz (Get-Item $objMain).Length } else { "MISSING" }))
Write-Host ("  runtime .obj          : {0}" -f $(if (Test-Path $objRt) { Sz (Get-Item $objRt).Length } else { "MISSING" }))
Write-Host ("  exe                   : {0}" -f $(if (Test-Path $exe) { Sz (Get-Item $exe).Length } else { "not produced" }))
if ($timeLines.Count -gt 0) {
    Write-Host "  ---- internal [time] phases ----"
    foreach ($tl in $timeLines) { Write-Host ("  {0}" -f $tl) }
}
if ($timedOut) { Write-Host ("  TIMEOUT after {0} s" -f $TimeoutSecs) -ForegroundColor Yellow }
Write-Host "=================================================="

# ---- summary.json ----
$summary = [ordered]@{
    timestamp      = (Get-Date).ToString("yyyy-MM-ddTHH:mm:sszzz")
    entry          = $EntryAbs
    driver         = $DriverAbs
    driver_mtime   = (Get-Item $DriverAbs).LastWriteTime.ToString("yyyy-MM-dd HH:mm:ss")
    wall_ms        = [int64]$elapsed.TotalMilliseconds
    exit_code      = $code
    result         = $result
    err            = $errMsg
    peak_ws_bytes  = $peakWs
    peak_ws_at_ms  = $peakWsAt
    peak_priv_bytes = $peakPriv
    peak_priv_at_ms = $peakPrivAt
    driver_peak_ws_bytes = $driverPeakWs
    peak_child_ws_bytes  = $peakChildWs
    samples        = $sampleCount
    sample_ms      = $SampleMs
    modules_linked = $modCount
    hat_bytes      = $hatSize
    main_obj_bytes = if (Test-Path $objMain) { (Get-Item $objMain).Length } else { 0 }
    rt_obj_bytes   = if (Test-Path $objRt) { (Get-Item $objRt).Length } else { 0 }
    exe_bytes      = if (Test-Path $exe) { (Get-Item $exe).Length } else { 0 }
    time_lines     = $timeLines
    timed_out      = $timedOut
}
$summary | ConvertTo-Json -Depth 4 | Out-File (Join-Path $OutAbs "summary.json") -Encoding utf8

if (-not $timedOut -and $result -eq 'success' -and $code -eq 0) { exit 0 }
exit 1
