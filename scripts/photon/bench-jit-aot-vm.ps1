# -------------------------------------------------------------
# bench-jit-aot-vm.ps1 — Photon JIT / Photon AOT / VM 三方基准
#
# 负载（两种）：
#   sum: 循环累加 1..N（Int 溢出回绕不影响计时）
#   fib: 递归 Fibonacci(25)（调用密集）
#
# 执行目标：
#   Photon JIT : jit_vmhost.exe AURA_BENCH_JIT 模式（手编机器码经
#                JitBackend 装载 → RWX → executeNative）
#   Photon AOT : PhotonHatCompile → COFF → lld-link → 原生 exe
#   Aura VM    : build/hat-vmhost/current.exe 解释 .auc（不含 JitBackend
#                闭包的纯 VM 宿主；.auc 经路径槽交换喂入）
#   Seed VM    : rust/target/release/aura.exe run（Rust 字节码解释器）
#
# 指标：墙钟时间（Runs 取中位） + 峰值工作集（5ms 轮询）
#
# 已知边界：
#   1. JIT 目标依赖宿主构建布局（P5-① 发射器实参序漂移）——
#      脚本对 jit_vmhost 做多次构建重试，取自检通过的那次。
#   2. [R85]（2026-10-17）已修：JIT MedMs 现为宿主内 clockNowMs
#      分项墙钟（sum/fib 独立计时）。R70 记录的「恒同值」系误诊，
#      0 参外调探针实证正常递进。benchGateRaw 多令牌探针恒 1 仍
#      未修（P5-① 上游），负载选择维持 AURA_BENCH=1 全跑。
#
# 用法: powershell -File scripts\photon\bench-jit-aot-vm.ps1 [-Quick]
# -------------------------------------------------------------

param(
    [switch]$Quick
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"

$Aura   = Join-Path $Root 'rust\target\release\aura.exe'
$Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
$VMHost = Join-Path $Root 'build\jit-verify\jit_vmhost.exe'
$VMHostSrc = Join-Path $Root 'examples\compiler\jit_vmhost.aura'
$BenchDir = Join-Path $Root 'build\jit-bench'

if (-not (Test-Path $Driver)) { Write-Host "missing driver: $Driver" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $VMHostSrc)) { Write-Host "missing: $VMHostSrc" -ForegroundColor Red; exit 1 }
New-Item -ItemType Directory -Force -Path $BenchDir | Out-Null

# ---- 峰值内存轮询运行器 ----
function Run-WithPeak([string]$Exe, [System.Collections.Generic.Dictionary[string,string]]$EnvMap, [string]$ArgStr, [int]$TimeoutMs) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.Arguments = $ArgStr
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.WorkingDirectory = $Root
    foreach ($kv in $EnvMap.GetEnumerator()) { $psi.EnvironmentVariables[$kv.Key] = $kv.Value }
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $soT = $p.StandardOutput.ReadToEndAsync()
    $peak = 0
    while (-not $p.HasExited) {
        try { $ws = $p.PeakWorkingSet64; if ($ws -gt $peak) { $peak = $ws } } catch {}
        Start-Sleep -Milliseconds 5
        if ($sw.ElapsedMilliseconds -gt $TimeoutMs) { try { $p.Kill() } catch {}; break }
    }
    $sw.Stop()
    $so = ''
    try { $so = $soT.Result } catch {}
    if ($p.HasExited) {
        try { $ws = $p.PeakWorkingSet64; if ($ws -gt $peak) { $peak = $ws } } catch {}
    }
    return @{ Ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 0); PeakMB = [math]::Round($peak / 1MB, 1); Out = $so }
}

# ---- 基准源码生成（N 烘焙进源码）----
function Make-SumSource([string]$path, [int]$n, [bool]$forAot) {
    # AOT exe 需显式 Heap.init()（Rust VM 无 Heap_init 内置，种子变体不带）
    $heap = if ($forAot) { "    Heap.init()`n" } else { "" }
    $imp = if ($forAot) { "import aura.lang.native.Heap`n`n" } else { "" }
    $code = "${imp}fun sumTo(n: Int): Int {`n    var s: Int = 0`n    var i: Int = 0`n    while (i < n) { s = s + i; i = i + 1 }`n    return s`n}`n`nfun main(): Int {`n${heap}    println(toStr(sumTo($n)))`n    return 0`n}`n"
    [System.IO.File]::WriteAllText($path, $code, (New-Object System.Text.UTF8Encoding($false)))
}
function Make-FibSource([string]$path, [int]$n, [bool]$forAot) {
    $heap = if ($forAot) { "    Heap.init()`n" } else { "" }
    $imp = if ($forAot) { "import aura.lang.native.Heap`n`n" } else { "" }
    $code = "${imp}fun fib(n: Int): Int {`n    if (n <= 1) { return n }`n    return fib(n - 1) + fib(n - 2)`n}`n`nfun main(): Int {`n${heap}    println(toStr(fib($n)))`n    return 0`n}`n"
    [System.IO.File]::WriteAllText($path, $code, (New-Object System.Text.UTF8Encoding($false)))
}

# ---- 构建 jit_vmhost（重试至自检通过）----
# ⚠️ COFF hex 从 **driver 本次 stdout** 提取：驱动不写 frontend.out.txt，
# 旧实现读该文件拿到的是上一次 jit-native-verify 留下的**陈旧产物**——
# 三次"重建"链接的全是同一份过期 COFF（2026-10-09 修）。
# 现役二进制先过门（自检或 JIT 哨兵），通过则**免重建**——避免把已验证
# 的好构建换进布局轮盘（同一驱动输出确定，跨驱动构建才有漂移）。
# ⚠️ Extract-Coff/Write-HexFile 必须定义在**顶层**：旧版定义在下方 for
# 循环体内，"过门免重建"跳过循环体后函数未定义，AOT 段调用即
# CommandNotFound（实测）。
function Extract-Coff([string]$data, [string]$marker) {
    $i = $data.IndexOf($marker); if ($i -lt 0) { return $null }
    $j = $i + $marker.Length
    while ($j -lt $data.Length -and [char]::IsWhiteSpace($data[$j])) { $j++ }
    $k = $j
    while ($k -lt $data.Length -and $data[$k] -ne "`r" -and $data[$k] -ne "`n") { $k++ }
    return $data.Substring($j, $k - $j)
}
function Write-HexFile([string]$Hex, [string]$Path) {
    $bytes = New-Object byte[] ($Hex.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [Convert]::ToByte($Hex.Substring($i * 2, 2), 16) }
    [System.IO.File]::WriteAllBytes($Path, $bytes)
}
Write-Host "[build] Photon 编译 jit_vmhost（布局漂移重试）..." -ForegroundColor DarkCyan
$vmhostOk = $false
function Test-VmhostGate([string]$Exe) {
    # 哨兵：宿主去输入化基准（sum(1e9) 固定值）一次跑通即过门。
    $env2s = New-Object 'System.Collections.Generic.Dictionary[string,string]'
    $env2s['AURA_BENCH'] = '1'; $env2s['AURA_HEAP_MB'] = '256'
    $sr = Run-WithPeak $Exe $env2s "" 60000
    if ($sr.Out.Contains("[BENCH-SUM-RESULT]")) {
        $lines = $sr.Out -split "`r?`n"
        for ($li = 0; $li -lt $lines.Count - 1; $li++) {
            if ($lines[$li] -eq '[BENCH-SUM-RESULT]' -and $lines[$li + 1] -eq '499999999500000000') { return 'sentinel' }
        }
    }
    return ''
}
if (Test-Path $VMHost) {
    $gate = Test-VmhostGate $VMHost
    if ($gate -ne '') {
        $vmhostOk = $true
        Write-Host "[build] 现役 jit_vmhost 过门（$gate），免重建" -ForegroundColor Green
    }
}
for ($attempt = 1; $attempt -le 3 -and -not $vmhostOk; $attempt++) {
    $env:AURA_HAT_AURA       = $VMHostSrc
    $env:AURA_HAT_OUT        = (Join-Path $Root 'build\jit-verify')
    $env:AURA_HAT_MODULE     = 'jit_vmhost'
    $env:AURA_HAT_SRC        = (Join-Path $Root 'build\jit-verify\jit_vmhost.hat')
    $env:AURA_PHOTON_SKIP_LINK = '1'
    $env:AURA_MEM_LIMIT_MB   = '16384'
    $env:AURA_HEAP_MB        = '4096'
    $out = & $Driver 2>&1
    $env:AURA_HAT_AURA = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''; $env:AURA_HAT_SRC = ''
    $env:AURA_PHOTON_SKIP_LINK = ''; $env:AURA_MEM_LIMIT_MB = ''; $env:AURA_HEAP_MB = ''
    $data = ($out -join "`n")

    $coffMain = Extract-Coff $data '===COFF-MAIN==='
    $coffRt   = Extract-Coff $data '===COFF-RUNTIME==='
    if (-not $coffMain -or -not $coffRt) { continue }

    foreach ($f in @('jit_vmhost.obj','aura_runtime.obj')) {
        $p = Join-Path $Root "build\jit-verify\$f"
        if (Test-Path $p) { Remove-Item $p -Force }
    }
    Write-HexFile $coffMain (Join-Path $Root 'build\jit-verify\jit_vmhost.obj')
    Write-HexFile $coffRt   (Join-Path $Root 'build\jit-verify\aura_runtime.obj')

    $lld = (Get-Command 'lld-link.exe' -ErrorAction SilentlyContinue).Source
    if (-not $lld) { $lld = 'lld-link.exe' }
    $k32 = (Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\Lib" -Recurse -Filter 'kernel32.Lib' | Select-Object -First 1).FullName
    $ntd = Join-Path (Split-Path $k32 -Parent) 'ntdll.lib'
    $null = & $lld (Join-Path $Root 'build\jit-verify\jit_vmhost.obj') (Join-Path $Root 'build\jit-verify\aura_runtime.obj') "/OUT:$VMHost" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB "/STACK:16777216" $k32 $ntd 2>&1
    if (-not (Test-Path $VMHost)) { continue }

    # 自检门：自检全绿最好；退而求其次过 JIT 哨兵（sum n=10 → 45，
    # emitFunction/装载/数据槽/executeNative 全链真实执行）——JIT 基准
    # 只跑 sum，哨兵通过即可用。全不过才重试。
    $gate = Test-VmhostGate $VMHost
    if ($gate -ne '') { $vmhostOk = $true }
}
if ($vmhostOk) {
    Write-Host "[build] jit_vmhost 验证通过" -ForegroundColor Green
} else {
    Write-Host "[warn] jit_vmhost 自检+哨兵均未通过（P5-① 布局漂移）——JIT 数字将不可信" -ForegroundColor Yellow
}

# ---- 负载源码 ----
$sumSrc = Join-Path $BenchDir 'sum_bench.aura'
$fibSrc = Join-Path $BenchDir 'fib_bench.aura'
$SUM_N  = 1000000    # JIT/AOT/双 VM 同负载
$FIB_N  = 25         # 同负载（种子 VM fib(25) ≈ 数秒内）
Make-SumSource $sumSrc $SUM_N $false
Make-FibSource $fibSrc $FIB_N $false

# ---- .auc 生成（Aura VM 目标）----
Write-Host "[build] .auc 生成..." -ForegroundColor DarkCyan
$sumAuc = Join-Path $BenchDir 'sum_bench.auc'
$fibAuc = Join-Path $BenchDir 'fib_bench.auc'
$null = & $Aura build $sumSrc --output $sumAuc 2>&1
$null = & $Aura build $fibSrc --output $fibAuc 2>&1

# ---- AOT 构建（现役 exe 可用则免重建：AOT 产物与 JIT 宿主同受驱动
#      构建漂移影响，能跑就别换。复用门 = 自跑输出等于期望值（负载
#      口径变化时旧 exe 自动失格重建）。AOT sum 与 JIT 同负载 1e9；
#      VM 目标保持 1e6（解释执行 1e9 ≈ 40 分钟不现实）----
Write-Host "[build] Photon AOT..." -ForegroundColor DarkCyan
$AOT_SUM_N = 1000000000
foreach ($b in @('sum','fib')) {
    $src = Join-Path $BenchDir "aot_$b.aura"
    if ($b -eq 'sum') { Make-SumSource $src $AOT_SUM_N $true } else { Make-FibSource $src $FIB_N $true }
    $outDir = Join-Path $BenchDir "aot_$b"
    $exePath = Join-Path $outDir "$b.exe"
    $expect = if ($b -eq 'sum') { '499999999500000000' } else { '75025' }
    if (Test-Path $exePath) {
        $chk = & $exePath 2>&1
        if ($LASTEXITCODE -eq 0 -and ($chk -join '').Contains($expect)) {
            Write-Host "[build] AOT $b 复用现役 exe（自跑输出=$expect）" -ForegroundColor Green
            continue
        }
        Write-Host "[build] AOT $b 现役 exe 输出失格（期望 $expect），重建" -ForegroundColor Yellow
    }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $env:AURA_HAT_AURA   = $src
    $env:AURA_HAT_SRC    = (Join-Path $outDir "$b.hat")
    $env:AURA_HAT_OUT    = $outDir
    $env:AURA_HAT_MODULE = $b
    $outD = & $Driver 2>&1
    $env:AURA_HAT_AURA = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''
    $data = ($outD -join "`n")
    $coffMain = Extract-Coff $data '===COFF-MAIN==='
    $coffRt   = Extract-Coff $data '===COFF-RUNTIME==='
    if ($coffMain -and $coffRt) {
        Write-HexFile $coffMain (Join-Path $outDir "$b.obj")
        Write-HexFile $coffRt   (Join-Path $outDir 'aura_runtime.obj')
        $null = & $lld (Join-Path $outDir "$b.obj") (Join-Path $outDir 'aura_runtime.obj') "/OUT:$outDir\$b.exe" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB "/STACK:16777216" $k32 $ntd 2>&1
    }
}

# ---- 测量 ----
function Bench-Target([string]$label, [string]$exe, [System.Collections.Generic.Dictionary[string,string]]$envMap, [string]$ArgStr, [int]$runs, [int]$timeoutSec) {
    $times = @(); $peakMax = 0.0; $outSample = ''
    for ($i = 0; $i -lt $runs; $i++) {
        $r = Run-WithPeak $exe $envMap $ArgStr ($timeoutSec * 1000)
        if ($r.Ms -gt 0) { $times += $r.Ms }
        if ($r.PeakMB -gt $peakMax) { $peakMax = $r.PeakMB }
        if ($outSample -eq '' -and $r.Out -ne '') { $outSample = ($r.Out -split "`r?`n")[0] }
    }
    if ($times.Count -eq 0) { return @{ Med = -1; Peak = $peakMax; Out1 = $outSample } }
    $sorted = $times | Sort-Object
    return @{ Med = $sorted[[int][Math]::Floor($sorted.Count / 2)]; Peak = $peakMax; Out1 = $outSample }
}

$RESULTS = @()
$T = 600

Write-Host "`n=== 运行基准（每目标 3 次取中位）===" -ForegroundColor Cyan

# -- Photon JIT（去输入化：宿主一轮固定跑 sum(1e9)+fib(25)，解析
#    「标记行+裸值行」）--
# 宿主只依赖轮盘免疫形态：executeNative0（call0 单参）、Clock_nowMs
# （QPC 0 参）、benchGate（Env_probe1 1 参）、Memory.* 内联。
# 2 参 extern（env_get/clockGettime）在部分布局下 name/实参被腐坏
# （gdb 实证 AURA_BENCH_N 恒读成 default 1e9），已全部移出基准路径。
#
# [R85]（2026-10-17）分项口径已修：宿主 bench 路径用 clockNowMs
# （QPC 0 参外调）在进程内对 sum/fib 分别计时，MedMs = 分项内部
# 墙钟中位（纯负载成本）；进程墙钟仅留在原始输出。
# R70「clockNowMs 恒同值」系误诊——2026-10-17 探针实证 0 参外调
# 正常递进（41503626→41503806→41503980）。benchGateRaw 多令牌仍不可用，
# 负载选择维持 AURA_BENCH=1 全跑（属 P5-① 上游，未动）。
$jitTimes = @{ sum = @(); fib = @() }
$jitRes = @{ sum = ''; fib = '' }
# [R85] 分项内部墙钟（宿主 clockNowMs）；无标记时回退进程墙钟
$jitMs = @{ sum = -1; fib = -1 }
$jitPeak = 0.0
for ($i = 0; $i -lt 3; $i++) {
    $env2 = New-Object 'System.Collections.Generic.Dictionary[string,string]'
    $env2['AURA_BENCH'] = '1'; $env2['AURA_HEAP_MB'] = '256'
    $r = Run-WithPeak $VMHost $env2 "" $T
    if ($r.PeakMB -gt $jitPeak) { $jitPeak = $r.PeakMB }
    $lines = $r.Out -split "`r?`n"
    $okSum = $false; $okFib = $false
    for ($li = 0; $li -lt $lines.Count - 1; $li++) {
        if ($lines[$li] -eq '[BENCH-SUM-RESULT]') {
            $jitRes['sum'] = $lines[$li + 1]
            if ($jitRes['sum'] -eq '499999999500000000') { $okSum = $true }
        } elseif ($lines[$li] -eq '[BENCH-SUM-MS]') {
            $jitMs['sum'] = [int]$lines[$li + 1]
        } elseif ($lines[$li] -eq '[BENCH-FIB-RESULT]') {
            $jitRes['fib'] = $lines[$li + 1]
            if ($jitRes['fib'] -eq '75025') { $okFib = $true }
        } elseif ($lines[$li] -eq '[BENCH-FIB-MS]') {
            $jitMs['fib'] = [int]$lines[$li + 1]
        }
    }
    # [R85] MedMs = 宿主内 clockNowMs 分项墙钟（纯负载成本，
    # [R85] 优先吃宿主分项内部墙钟；缺失时回退进程墙钟（旧行为）
    if ($okSum) { if ($jitMs['sum'] -ge 0) { $jitTimes['sum'] += $jitMs['sum'] } else { $jitTimes['sum'] += $r.Ms } }
    if ($okFib) { if ($jitMs['fib'] -ge 0) { $jitTimes['fib'] += $jitMs['fib'] } else { $jitTimes['fib'] += $r.Ms } }
}
foreach ($w in @('sum','fib')) {
    $med = -1
    if ($jitTimes[$w].Count -gt 0) { $sorted = $jitTimes[$w] | Sort-Object; $med = $sorted[[int][Math]::Floor($sorted.Count / 2)] }
    $RESULTS += [pscustomobject]@{ Target = "Photon-JIT"; Workload = $w; MedMs = $med; PeakMB = $jitPeak; Out1 = $jitRes[$w] }
}

# -- Photon AOT --
foreach ($w in @('sum','fib')) {
    $exe = Join-Path $BenchDir "aot_$w\$w.exe"
    if (-not (Test-Path $exe)) { continue }
    $r = Bench-Target "AOT-$w" $exe (New-Object 'System.Collections.Generic.Dictionary[string,string]') "" 3 $T
    $RESULTS += [pscustomobject]@{ Target = "Photon-AOT"; Workload = $w; MedMs = $r.Med; PeakMB = $r.Peak; Out1 = $r.Out1 }
}

# -- Aura VM（原生 VM 宿主解释 .auc；宿主 = build/hat-vmhost/current.exe
#    ——不含 JitBackend 闭包的 VM 构建。该宿主硬编码 .auc 路径
#    build/vm/current.auc（jit_vmhost 的 Env.get+VmRunner 共存实参错位
#    同源，P5-①族，见 build/hat-vmhost/current.aura 注释）——基准经
#    路径交换（备份→替换→跑→恢复），宿主二进制零重建风险。--
$VmExe   = Join-Path $Root 'build\hat-vmhost\current.exe'
$AucSlot = Join-Path $Root 'build\vm\current.auc'
if (-not (Test-Path $VmExe)) { Write-Host "missing: $VmExe" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $AucSlot)) { Write-Host "missing: $AucSlot" -ForegroundColor Red; exit 1 }
Copy-Item $AucSlot (Join-Path $BenchDir 'current_auc.bak') -Force
foreach ($w in @('sum','fib')) {
    $auc = if ($w -eq 'sum') { $sumAuc } else { $fibAuc }
    Copy-Item $auc $AucSlot -Force
    $r = Bench-Target "AuraVM-$w" $VmExe (New-Object 'System.Collections.Generic.Dictionary[string,string]') "" 3 $T
    $RESULTS += [pscustomobject]@{ Target = "Aura-VM"; Workload = $w; MedMs = $r.Med; PeakMB = $r.Peak; Out1 = $r.Out1 }
}
Copy-Item (Join-Path $BenchDir 'current_auc.bak') $AucSlot -Force

# -- Seed VM（Rust 解释器；⚠ 参数须整体转义为单串）--
foreach ($w in @('sum','fib')) {
    $src = if ($w -eq 'sum') { $sumSrc } else { $fibSrc }
    $r = Bench-Target "SeedVM-$w" $Aura (New-Object 'System.Collections.Generic.Dictionary[string,string]') ('run "' + $src + '"') 3 $T
    $RESULTS += [pscustomobject]@{ Target = "Rust-VM"; Workload = $w; MedMs = $r.Med; PeakMB = $r.Peak; Out1 = $r.Out1 }
}

Write-Host ""
$RESULTS | Format-Table Target, Workload, MedMs, PeakMB, Out1 -AutoSize
$RESULTS | Export-Csv (Join-Path $BenchDir 'results.csv') -NoTypeInformation
Write-Host "结果已写入 build/jit-bench/results.csv"
