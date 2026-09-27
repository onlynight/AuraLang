# photon-hat-suite.ps1 — HAT 格式端到端差分跑批（aura 源码 → .hat → Photon HAT 后端 → exe）
#
# 与 scripts\photon-suite.ps1（PHIR 路径）并列：同一批用例、同一判定标准，
# 但后端输入是 **HAT IR 文本**而不是 .phir 伪源码。
#
# 每个用例的流程：
#   1) VM 跑一遍得到基准 stdout                    aura run <src>.aura
#   2) Rust 前端产出 HIR 文本（.phir 仅作前端序列化通道，HAT 管线本身不读它）
#                                                  aura build -b photon <src> --output <out>/<stem>.phir
#   3) HIR → SSA → HAT 文本，再由 HAT 文本 → SSA → LIR → X86 → COFF
#                                                  aura run .../PhotonHatBuild.aura
#      （AURA_HAT_PHIR / AURA_HAT_SRC / AURA_HAT_OUT / AURA_HAT_MODULE）
#   4) COFF hex → .obj → lld-link → exe，运行并与 VM 基准比较 stdout
#
# 用法:
#   powershell -File scripts\photon-hat-suite.ps1 -Phase P1
#   powershell -File scripts\photon-hat-suite.ps1 -Phase P1,P2,P3
#   powershell -File scripts\photon-hat-suite.ps1 -Files tests\photon\P1\01_hello_world.aura
param(
    [string]$Phase = "P1,P2,P3",
    [string[]]$Files = @(),
    [int]$TimeoutSecs = 120,
    [string]$OutRoot = "build\hat-suite",
    # 打开阶段进度/心跳（[hat-front] / [Phase A-E] / [isa] / [hat-parse]）。
    # 默认关：驱动 stdout 只保留 ===...=== 协议标记，本脚本逐行解析 COFF hex。
    [switch]$Verbose
)
$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
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

$Driver = Join-Path $Root 'aura\compiler\aura\lang\compiler\backend\photon\PhotonHatBuild.aura'

# ── 进程调用（异步双管道读，超时杀进程树）──
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

# ── 收集用例 ──
if ($Files.Count -eq 0) {
    foreach ($ph in ($Phase -split ',')) {
        $dir = "tests\photon\$($ph.Trim())"
        if (Test-Path $dir) {
            $Files += (Get-ChildItem $dir -Filter "*.aura" -File | ForEach-Object { $_.FullName })
        }
    }
}
if ($Files.Count -eq 0) { Write-Host "no files" -ForegroundColor Red; exit 1 }

Write-Host "=== Photon HAT 端到端差分（aura → HAT → exe）===" -ForegroundColor Cyan
Write-Host "  aura   : $Aura"
Write-Host "  driver : PhotonHatBuild.aura"
Write-Host "  lld    : $lld"
Write-Host "  cases  : $($Files.Count)"
Write-Host ""

$pass = 0; $fail = 0
$fails = @()
foreach ($src in $Files) {
    $leaf = Split-Path -Leaf $src
    $stem = [IO.Path]::GetFileNameWithoutExtension($src)
    $outDir = Join-Path $Root (Join-Path $OutRoot $stem)
    $hatOut = Join-Path $outDir 'hat'
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    New-Item -ItemType Directory -Force -Path $hatOut | Out-Null

    $phir = Join-Path $outDir "$stem.phir"
    $hat  = Join-Path $outDir "$stem.hat"
    $exe  = Join-Path $hatOut "$stem.exe"

    # 1) VM 基准
    $vm = Invoke-Proc $Aura "run `"$src`"" $Root $TimeoutSecs
    $vmOut = (("" + $vm.Out) -replace "`r", "").Trim()

    # 2) 前端产出 HIR 文本
    $b1 = Invoke-Proc $Aura "build -b photon `"$src`" --output `"$phir`"" $Root $TimeoutSecs

    # 3) HAT 管线（HIR → SSA → .hat → 解析 → SSA → … → COFF）
    $env:AURA_HAT_PHIR = $phir
    $env:AURA_HAT_SRC = $hat
    $env:AURA_HAT_OUT = $hatOut
    $env:AURA_HAT_MODULE = $stem
    $d = Invoke-Proc $Aura "run `"$Driver`"" $Root $TimeoutSecs
    $env:AURA_HAT_PHIR = ''; $env:AURA_HAT_SRC = ''; $env:AURA_HAT_OUT = ''; $env:AURA_HAT_MODULE = ''

    # 4) 解析标记 → 写 .obj → 链接
    $coffMain = $null; $coffRt = $null; $mode = ''; $errMsg = ''
    foreach ($l in ($d.Out -split "`n")) {
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
    if (-not (Test-Path $phir)) { $reasons += "no-phir" }
    if (-not (Test-Path $hat))  { $reasons += "no-hat" }
    if (-not $coffMain) { $reasons += "no-coff-main" }
    if (-not $coffRt)   { $reasons += "no-coff-runtime" }
    if (-not $d.Ok)     { $reasons += "hat-timeout" }

    if ($reasons.Count -eq 0) {
        $objMain = Join-Path $hatOut "$stem.obj"
        $objRt   = Join-Path $hatOut 'aura_runtime.obj'
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
        $r = Invoke-Proc $exe '' $hatOut $TimeoutSecs
        $exeOut = (("" + $r.Out) -replace "`r", "").Trim()
        $code = $r.Code
        $tmo = -not $r.Ok
        if ($tmo) { $reasons += "run-timeout" }
        if ($vmOut -ne $exeOut) { $reasons += "output-diff" }
    }

    if ($reasons.Count -eq 0) {
        Write-Host ("  PASS  {0}" -f $leaf) -ForegroundColor Green
        $pass++
    } else {
        Write-Host ("  FAIL  {0}  [{1}] exit={2}" -f $leaf, ($reasons -join ','), $code) -ForegroundColor Red
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
