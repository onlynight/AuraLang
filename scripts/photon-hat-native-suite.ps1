# photon-hat-native-suite.ps1 — HAT 独立链路端到端差分跑批
#
#   源码(.aura) ──[Aura 自举前端，原生]──► HIR ──► SSA MIR ──► .hat ──► HAT 后端 ──► exe
#
# 与 PHIR 链路（PhotonDriver：.phir → HIR → SSA → …）**完全独立**：
# 本脚本不生成、不读取任何 `.phir`。
#
# 前端 = 原生运行的自举编译器（build/hat-native/PhotonHatCompile.exe，
# 由 `aura build --aot` 产出）：源码(含递归 import) → AotModuleLinker → 合并 HIR
# → SSA MIR → .hat。原生执行是这个前端**唯一可行**的载体：冻结种子 VM 下
# AotModuleLinker 不可用（跨对象 arena 共享失真，std 模块一条都加载不了），
# 而原生实测 28 模块 / 1 万 HIR 节点仅 1s。
#
# 后端默认走 **VM**（`PhotonHatBuild.aura` 读 .hat → SSA → LIR → X86 → COFF）：
# 原生后端目前在 Phase D（寄存器分配）崩溃 0xC0000005（自举编译的 AOT 缺陷，
# 与 PHIR 链路历史上 Phase A 崩溃同源），而 VM 后端的 .hat→exe 已 15/15 验证。
# 用 `-NativeAll` 可切到「前端+后端都在原生」的完整形态（当前会在 Phase D 失败）。
#
# 每个用例：
#   1) VM 跑一遍得到基准 stdout（`aura run <src>`，仅作参照，不参与 HAT 链路）
#   2) 原生前端：源码 → HIR → SSA → .hat（-NativeAll 时同一次也产出 COFF）
#   3) 后端：.hat → COFF（默认 VM 的 PhotonHatBuild；-NativeAll 用第 2 步的产物）
#   4) COFF hex → .obj → lld-link → exe，运行并与 VM 基准比较 stdout
#
# 用法:
#   powershell -File scripts\photon-hat-native-suite.ps1 -Phase P1
#   powershell -File scripts\photon-hat-native-suite.ps1 -Phase P1,P2,P3
#   powershell -File scripts\photon-hat-native-suite.ps1 -Phase P1 -NativeAll
param(
    [string]$Phase = "P1,P2,P3",
    [string[]]$Files = @(),
    [int]$TimeoutSecs = 120,
    [string]$OutRoot = "build\hat-native-suite",
    [string]$Driver = "",
    [switch]$Rebuild,
    [switch]$NativeAll,
    # 打开阶段进度/心跳（[hat-front] / [Phase A-E] / [isa] / [hat-parse]）。
    # 默认关：驱动 stdout 只保留 ===...=== 协议标记，本脚本逐行解析 COFF hex。
    [switch]$Verbose
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$env:AURA_PHOTON_DEBUG_HIR = ''
$env:AURA_PHOTON_TRACE = ''
# 调试开关默认全关（见 PhotonPipeline.aura 的 verboseOn 注释）：
#   AURA_PHOTON_VERBOSE=1  阶段进度/心跳
#   AURA_HAT_TRACE=1       HAT 解析器心跳
if ($Verbose) { $env:AURA_PHOTON_VERBOSE = '1'; $env:AURA_HAT_TRACE = '1' }
else          { $env:AURA_PHOTON_VERBOSE = '';  $env:AURA_HAT_TRACE = '' }

$Aura = Join-Path $Root 'rust\target\release\aura.exe'
if (-not (Test-Path $Aura)) { Write-Host "aura.exe not found: $Aura" -ForegroundColor Red; exit 1 }

$DriverSrc = Join-Path $Root 'aura\compiler\aura\lang\compiler\backend\photon\PhotonHatCompile.aura'
if ($Driver -eq "") { $Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe' }

if ($Rebuild -or -not (Test-Path $Driver)) {
    Write-Host "[build] AOT 构建原生 HAT 驱动（前端 + 管线，需要数秒）..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Force -Path (Split-Path $Driver) | Out-Null
    & $Aura build --aot $DriverSrc --output $Driver 2>&1 | Select-String -Pattern "complete|failed|error" | Select-Object -Last 3
}
if (-not (Test-Path $Driver)) { Write-Host "driver not built: $Driver" -ForegroundColor Red; exit 1 }

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

function Find-Kernel32 {
    $roots = @()
    if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Lib') }
    if ($env:ProgramFiles)        { $roots += (Join-Path $env:ProgramFiles 'Windows Kits/10/Lib') }
    foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        foreach ($v in (Get-ChildItem $r -Directory | Sort-Object Name -Descending)) {
            $p = Join-Path $v.FullName 'um/x64/kernel32.Lib'
            if (Test-Path $p) { return $p }
        }
    }
    return $null
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

$lld = 'lld-link.exe'
$resolved = Get-Command $lld -ErrorAction SilentlyContinue
if ($resolved) { $lld = $resolved.Source }
$k32 = Find-Kernel32
if (-not $k32) { Write-Host "kernel32.Lib not found" -ForegroundColor Red; exit 1 }

if ($Files.Count -eq 0) {
    foreach ($ph in ($Phase -split ',')) {
        $dir = "tests\photon\$($ph.Trim())"
        if (Test-Path $dir) {
            $Files += (Get-ChildItem $dir -Filter "*.aura" -File | ForEach-Object { $_.FullName })
        }
    }
}
if ($Files.Count -eq 0) { Write-Host "no files" -ForegroundColor Red; exit 1 }

Write-Host "=== Photon HAT 独立链路差分（源码 → HIR → SSA → .hat → exe）===" -ForegroundColor Cyan
Write-Host "  driver : $Driver"
Write-Host "  lld    : $lld"
Write-Host "  cases  : $($Files.Count)"
Write-Host ""

$pass = 0; $fail = 0
$fails = @()
foreach ($src in $Files) {
    $leaf = Split-Path -Leaf $src
    $stem = [IO.Path]::GetFileNameWithoutExtension($src)
    $outDir = Join-Path $Root (Join-Path $OutRoot $stem)
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    $hat = Join-Path $outDir "$stem.hat"
    $exe = Join-Path $outDir "$stem.exe"

    # 1) VM 基准（参照）
    $vm = Invoke-Proc $Aura "run `"$src`"" $Root $TimeoutSecs
    $vmOut = (("" + $vm.Out) -replace "`r", "").Trim()

    # 2) 原生前端：源码 → HIR → SSA → .hat
    $env:AURA_HAT_AURA = $src
    $env:AURA_HAT_SRC = $hat
    $env:AURA_HAT_OUT = $outDir
    $env:AURA_HAT_MODULE = $stem
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $d = Invoke-Proc $Driver "" $Root $TimeoutSecs
    $sw.Stop()
    $env:AURA_HAT_AURA = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''
    $secs = [math]::Round($sw.Elapsed.TotalSeconds, 1)

    # 3) 后端：默认用 VM 读 .hat 产出 COFF（原生后端在 Phase D 崩溃）
    $b = $d
    if (-not $NativeAll -and (Test-Path $hat)) {
        $env:AURA_HAT_SRC = $hat
        $env:AURA_HAT_OUT = $outDir
        $env:AURA_HAT_MODULE = $stem
        $b = Invoke-Proc $Aura "run `"$(Join-Path $Root 'aura\compiler\aura\lang\compiler\backend\photon\PhotonHatBuild.aura')`"" $Root $TimeoutSecs
        $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''
    }

    $coffMain = $null; $coffRt = $null; $mode = ''; $errMsg = ''
    foreach ($l in ($b.Out -split "`n")) {
        $t = $l.Trim()
        if ($t -eq '===COFF-MAIN===')    { $mode = 'main';   continue }
        if ($t -eq '===COFF-RUNTIME===') { $mode = 'rt';     continue }
        if ($t -eq '===LINK===')         { $mode = 'link';   continue }
        if ($t -eq '===RESULT===')       { $mode = 'result'; continue }
        if ($t.StartsWith('===ERR==='))  { $errMsg = $t.Substring(9); continue }
        if ($t -eq '') { continue }
        if ($t -match '^[0-9a-fA-F]{40,}$') {
            if ($mode -eq 'main' -and -not $coffMain) { $coffMain = $t }
            elseif ($mode -eq 'rt' -and -not $coffRt) { $coffRt = $t }
        }
    }

    $reasons = @()
    if (-not (Test-Path $hat))  { $reasons += "no-hat" }
    if (-not $coffMain) { $reasons += "no-coff-main" }
    if (-not $coffRt)   { $reasons += "no-coff-runtime" }
    if (-not $d.Ok)     { $reasons += "front-timeout" }
    if (-not $b.Ok)     { $reasons += "backend-timeout" }

    if ($reasons.Count -eq 0) {
        $objMain = Join-Path $outDir "$stem.obj"
        $objRt   = Join-Path $outDir 'aura_runtime.obj'
        Write-HexFile -Hex $coffMain -Path $objMain | Out-Null
        Write-HexFile -Hex $coffRt   -Path $objRt   | Out-Null
        Remove-Item $exe -Force -ErrorAction SilentlyContinue
        $linkArgs = @($objMain, $objRt, "/OUT:$exe", "/SUBSYSTEM:CONSOLE", "/ENTRY:main", "/MACHINE:X64", "/NODEFAULTLIB", $k32)
        $linkOut = & $lld @linkArgs 2>&1
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) {
            $reasons += "link-fail"
            Write-Host ("        link: " + (("" + $linkOut) -replace "`n", " | ")) -ForegroundColor DarkGray
        }
    }

    $exeOut = ''; $code = -1; $tmo = $false
    if ((Test-Path $exe) -and $reasons.Count -eq 0) {
        $r = Invoke-Proc $exe '' $outDir $TimeoutSecs
        $exeOut = (("" + $r.Out) -replace "`r", "").Trim()
        $code = $r.Code
        $tmo = -not $r.Ok
        if ($tmo) { $reasons += "run-timeout" }
        if ($vmOut -ne $exeOut) { $reasons += "output-diff" }
    }

    if ($reasons.Count -eq 0) {
        Write-Host ("  PASS  {0}  ({1}s)" -f $leaf, $secs) -ForegroundColor Green
        $pass++
    } else {
        Write-Host ("  FAIL  {0}  [{1}] exit={2} ({3}s)" -f $leaf, ($reasons -join ','), $code, $secs) -ForegroundColor Red
        Write-Host ("        VM  : {0}" -f ($vmOut -replace "`n", " | "))
        Write-Host ("        EXE : {0}" -f ($exeOut -replace "`n", " | "))
        if ($errMsg -ne '') { Write-Host ("        ERR : {0}" -f $errMsg) -ForegroundColor DarkGray }
        $fails += $leaf
        $fail++
    }
}

Write-Host ""
Write-Host "TOTAL: PASS=$pass FAIL=$fail" -ForegroundColor Cyan
if ($fail -gt 0) { Write-Host ("FAILED: " + ($fails -join ", ")) -ForegroundColor Red }
exit $fail
