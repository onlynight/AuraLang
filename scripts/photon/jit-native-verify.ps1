# -------------------------------------------------------------
# Photon JIT 原生验证（P1-3）— 原生 VM 宿主 + 内建 JIT 自检
#
# 架构定位（docs/pure_aura_jit/07 §2.3）：JIT 是 **VM 运行时优化**——
# Photon AOT 的对象是 VM 自身。本脚本验证链：
#
#   1. 重建原生 HAT 驱动（可选 -RebuildDriver）
#   2. Photon 编译参数化 VM 宿主 build/hat-vmhost/jit_vmhost.aura
#      → COFF hex（前端产出，AURA_PHOTON_SKIP_LINK=1 跳过管线内链接）
#   3. lld-link（kernel32 + ntdll，/STACK:16M）→ jit_vmhost.exe
#   4. AURA_VM_SELFTEST=1 运行：宿主内建 JIT 自检直接驱动 JitBackend
#      原生链——allocRW（低地址 W^X 区）→ writeCode（真实字节写入）→
#      protect RX → executeNative（JitExec 间接 call）→ 返回值校验；
#      另覆盖多函数跨段 call rel32 回填与数据段读写。
#   5. 解析 === SELFTEST-ALL-PASS ===（且无 [FAIL] 行）
#
# 用法:
#   scripts\photon\jit-native-verify.ps1
#   scripts\photon\jit-native-verify.ps1 -RebuildDriver
#   scripts\photon\jit-native-verify.ps1 -Keep     # 保留中间产物
#
# 前置: lld-link 在 PATH 或 aura.toml [lld]；Windows SDK（kernel32/ntdll.lib）
#
# 已知边界（P5，见 docs/pure_aura_jit/09 §七）：
#   * .auc 字节码侧的 JIT 路径（VM 值模型/单例初始化/多参编排缺口）
#     未打通——原生验证由宿主内建自检承载；种子侧守卫回归由
#     tests/photon/P1/p12、p13 覆盖（run-tests 基线 109 OK / 25 FAIL）。
#   * compileBatch 的重定位解析路径依赖 String.substring（3 参方法调用
#     接收者错位，原生段错误）——自检以手编 blob 等价验证跨段回填语义。
# -------------------------------------------------------------
param(
    [switch]$RebuildDriver,
    [switch]$Keep
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

function Find-Aura {
    foreach ($c in @(
        'rust\target\release\aura.exe', 'aura\seed\aura.exe',
        'build\bin\aura.exe', 'target\release\aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Get-LldDirFromManifest {
    param([string]$Manifest)
    if (-not (Test-Path $Manifest)) { return $null }
    $text = [System.IO.File]::ReadAllText($Manifest, [System.Text.Encoding]::UTF8)
    $inLld = $false
    foreach ($raw in ($text -split "`r?`n")) {
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        if ($line.StartsWith('[')) { $inLld = ($line -eq '[lld]'); continue }
        if (-not $inLld) { continue }
        $eq = $line.IndexOf('=')
        if ($eq -le 0) { continue }
        $val = $line.Substring($eq + 1).Trim()
        if ($val.StartsWith('"')) {
            $end = $val.IndexOf('"', 1)
            if ($end -gt 1) { $val = $val.Substring(1, $end - 1) }
        }
        return $val
    }
    return $null
}

function Find-Kernel32 {
    param([string]$Arch = 'x64')
    $roots = @()
    if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Lib') }
    if ($env:ProgramFiles)        { $roots += (Join-Path $env:ProgramFiles 'Windows Kits/10/Lib') }
    foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        foreach ($v in (Get-ChildItem $r -Directory | Sort-Object Name -Descending)) {
            $p = Join-Path $v.FullName ("um/$Arch/kernel32.Lib")
            if (Test-Path $p) { return (Resolve-Path $p).Path }
        }
    }
    throw "kernel32.Lib not found"
}

function Write-HexFile {
    param([string]$Hex, [string]$Path)
    if ($Hex.Length % 2 -ne 0) { throw "odd hex length: $($Hex.Length)" }
    $bytes = New-Object byte[] ($Hex.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $bytes[$i] = [Convert]::ToByte($Hex.Substring($i * 2, 2), 16)
    }
    [System.IO.File]::WriteAllBytes($Path, $bytes)
    return $bytes.Length
}

# --- 工具链 ---
$Aura = Find-Aura
if (-not $Aura) { Write-Host "aura.exe not found" -ForegroundColor Red; exit 1 }

$lldDir = Get-LldDirFromManifest (Join-Path $Root 'aura.toml')
if ($lldDir) { $env:PATH = "$lldDir;$env:PATH" }
$resolved = Get-Command 'lld-link.exe' -ErrorAction SilentlyContinue
if ($resolved) { $lld = $resolved.Source } else { Write-Host "lld-link not found" -ForegroundColor Red; exit 1 }

# ntdll.lib 与 kernel32.Lib 同目录；JIT 分配桩走 IAT 导入
# NtAllocateVirtualMemory，不链 ntdll 就是 undefined symbol。
$k32 = Find-Kernel32
$ntd = Join-Path (Split-Path $k32 -Parent) 'ntdll.lib'
if (-not (Test-Path $ntd)) { $ntd = Join-Path (Split-Path $k32 -Parent) 'ntdll.Lib' }
if (-not (Test-Path $ntd)) { throw "ntdll.lib not found next to $k32" }

$DriverSrc = Join-Path $Root 'aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura'
$Driver    = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
$HostSrc   = Join-Path $Root 'examples\compiler\jit_vmhost.aura'
$OutDir    = Join-Path $Root 'build\jit-verify'
if (-not $Keep -and (Test-Path $OutDir)) { Remove-Item $OutDir -Recurse -Force -ErrorAction SilentlyContinue }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "=== Photon JIT 原生验证（原生 VM 宿主 + 内建自检） ===" -ForegroundColor Cyan
Write-Host "  aura    : $Aura"
Write-Host "  driver  : $Driver"
Write-Host "  vmhost  : $HostSrc"
Write-Host "  lld     : $lld"
Write-Host "  k32/ntd : $k32"

# --- 1. 驱动 ---
if ($RebuildDriver -or -not (Test-Path $Driver)) {
    Write-Host "[1/4] 重建原生 HAT 驱动..." -ForegroundColor DarkCyan
    & $Aura build --aot $DriverSrc --output $Driver 2>&1 | Select-String -Pattern "\[ok\]|failed|error" | Select-Object -Last 2
    if (-not (Test-Path $Driver)) { Write-Host "driver build failed" -ForegroundColor Red; exit 1 }
} else {
    Write-Host "[1/4] 复用现有驱动（-RebuildDriver 强制重建）" -ForegroundColor DarkCyan
}

# --- 2-4. 编译 → 链接 → 自检（⚠️ 布局漂移重试：自举发射器对源码/布局
# 敏感，同一源码不同构建崩点会迁移——P5 已记。构建成功即冻结 exe。）---
$maxAttempts = 3
$attempt = 0
$verifyOk = $false
while ($attempt -lt $maxAttempts -and -not $verifyOk) {
$attempt++
if ($attempt -gt 1) {
    Write-Host ("    重试第 {0}/{1} 次（布局漂移）..." -f $attempt, $maxAttempts) -ForegroundColor Yellow
}
Write-Host "[2/4] Photon 编译原生 VM 宿主（第 $attempt 次构建）..." -ForegroundColor DarkCyan
$env:AURA_HAT_AURA       = $HostSrc
$env:AURA_HAT_OUT        = $OutDir
$env:AURA_HAT_MODULE     = 'jit_vmhost'
$env:AURA_HAT_SRC        = (Join-Path $OutDir 'jit_vmhost.hat')
$env:AURA_PHOTON_SKIP_LINK = '1'   # 前端只产 COFF hex，链接由本脚本接管
$env:AURA_MEM_LIMIT_MB   = '24576'
$env:AURA_HEAP_MB        = '4096'  # 宿主闭包大（含 JitBackend→PhotonPipeline），driver 需扩堆
$sw = [Diagnostics.Stopwatch]::StartNew()
# 驱动峰值内存采样（5ms 轮询 PeakWorkingSet64）——2026-10-09 内存回归观测
$drvPeakMB = -1
$drvOut = New-Object System.Collections.Generic.List[string]
$drvPsi = New-Object System.Diagnostics.ProcessStartInfo
$drvPsi.FileName = $Driver
$drvPsi.UseShellExecute = $false
$drvPsi.RedirectStandardOutput = $true
$drvPsi.RedirectStandardError = $true
$drvPsi.WorkingDirectory = $Root
$drvProc = [System.Diagnostics.Process]::Start($drvPsi)
$drvSoT = $drvProc.StandardOutput.ReadToEndAsync()
$drvSeT = $drvProc.StandardError.ReadToEndAsync()
while (-not $drvProc.HasExited) {
    try { $ws = $drvProc.PeakWorkingSet64; if ($ws -gt $drvPeakMB) { $drvPeakMB = $ws } } catch {}
    Start-Sleep -Milliseconds 5
}
try { $ws2 = $drvProc.PeakWorkingSet64; if ($ws2 -gt $drvPeakMB) { $drvPeakMB = $ws2 } } catch {}
$so = ''; try { $so = $drvSoT.Result } catch {}
$seDrv = ''; try { $seDrv = $drvSeT.Result } catch {}
$out = @($so) + @($seDrv)
$sw.Stop()
Write-Host ("        driver peak={0} MB wall={1}s" -f [math]::Round($drvPeakMB / 1MB, 0), [math]::Round($sw.Elapsed.TotalSeconds, 1))
$env:AURA_HAT_AURA = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''; $env:AURA_HAT_SRC = ''
$env:AURA_PHOTON_SKIP_LINK = ''; $env:AURA_MEM_LIMIT_MB = ''; $env:AURA_HEAP_MB = ''
$data = ($out -join "`n")
[IO.File]::WriteAllText((Join-Path $OutDir 'frontend.out.txt'), $data)
$secs = [math]::Round($sw.Elapsed.TotalSeconds, 1)

function Extract-CoffAfterMarker {
    param([string]$data, [string]$marker)
    $i = $data.IndexOf($marker)
    if ($i -lt 0) { return $null }
    $j = $i + $marker.Length
    while ($j -lt $data.Length) {
        $ch = $data[$j]
        if ($ch -ne ' ' -and $ch -ne "`t" -and $ch -ne "`r" -and $ch -ne "`n") { break }
        $j++
    }
    $k = $j
    while ($k -lt $data.Length -and $data[$k] -ne "`r" -and $data[$k] -ne "`n") { $k++ }
    return $data.Substring($j, $k - $j)
}
$data = ($out -join "`n")
$coffMain = Extract-CoffAfterMarker $data '===COFF-MAIN==='
$coffRt   = Extract-CoffAfterMarker $data '===COFF-RUNTIME==='
Write-Host ("        main={0}B runtime={1}B ({2}s)" -f $(if ($coffMain) { $coffMain.Length } else { 0 }), $(if ($coffRt) { $coffRt.Length } else { 0 }), $secs)
if (-not $coffMain -or -not $coffRt) {
    Write-Host "        frontend produced no COFF" -ForegroundColor Red
    exit 1
}

$objMain = Join-Path $OutDir 'jit_vmhost.obj'
$objRt   = Join-Path $OutDir 'aura_runtime.obj'
Write-HexFile -Hex $coffMain -Path $objMain | Out-Null
Write-HexFile -Hex $coffRt   -Path $objRt   | Out-Null

# --- 3. 链接（kernel32 + ntdll + 16M 栈：AucLoader 深递归必需）---
Write-Host "[3/4] lld-link..." -ForegroundColor DarkCyan
$exe = Join-Path $OutDir 'jit_vmhost.exe'
$co = & $lld $objMain $objRt "/OUT:$exe" "/SUBSYSTEM:CONSOLE" "/ENTRY:main" "/MACHINE:X64" "/NODEFAULTLIB" "/STACK:16777216" $k32 $ntd 2>&1
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) {
    Write-Host "        link failed: $co" -ForegroundColor Red
    exit 1
}
Write-Host ("        exe={0:N0} bytes" -f (Get-Item $exe).Length)

# --- 4. 运行内建 JIT 自检 ---
Write-Host "[4/4] 运行内建 JIT 自检..." -ForegroundColor DarkCyan
$env:AURA_VM_SELFTEST = '1'
$env:AURA_HEAP_MB     = '512'
$run = & $exe 2>&1
$code = $LASTEXITCODE
$env:AURA_VM_SELFTEST = ''; $env:AURA_HEAP_MB = ''
$runText = ($run -join "`n")
Write-Host ($runText -replace "`n", "`n        ")

$passLines = ([regex]::Matches($runText, '\[PASS\]')).Count
$failLines = ([regex]::Matches($runText, '\[FAIL\]')).Count
if ($code -eq 0 -and $failLines -eq 0 -and $runText.Contains('SELFTEST-ALL-PASS') -and $passLines -gt 0) {
    $verifyOk = $true
    Write-Host ""
    Write-Host "=== 原生 JIT 验证通过（pass=$passLines，第 $attempt 次构建）===" -ForegroundColor Green
    exit 0
}
Write-Host ""
Write-Host "=== 第 $attempt 次构建自检失败 (exit=$code pass=$passLines fail=$failLines) ===" -ForegroundColor Yellow
}  # retry loop
Write-Host "=== 原生 JIT 验证失败（$maxAttempts 次构建均崩，见 P5 布局漂移清单）===" -ForegroundColor Red
exit 1
