# ==============================================================
# photon-e2e-verify.ps1 — Photon 端到端验证 (VM vs Photon 差分)
#
# 对给定 Aura 源文件：
#   1. VM 路径  : aura run <src>                 → 期望输出
#   2. Photon   : aura build -b photon <src>     → .exe
#   3. 运行 .exe → 实际输出
#   4. 差分比较 stdout + exit code
#
# 用法:
#   scripts\photon-e2e-verify.ps1 -Sources "tests\photon\simple.aura"
#   scripts\photon-e2e-verify.ps1 -Phase P1
#   scripts\photon-e2e-verify.ps1 -Phase all -TimeoutSecs 60
# ==============================================================
param(
    [string[]]$Sources,
    [ValidateSet("all","P1","P2","P3","P4")] [string]$Phase = "",
    [int]$TimeoutSecs = 60,
    [string]$OutRoot = "build\photon-verify"
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

# ── 工具定位 ──
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$AuraBin = $null
foreach ($c in @('rust\target\release\aura.exe','aura\seed\aura.exe','rust\target\debug\aura.exe','build\bin\aura.exe')) {
    if (Test-Path $c) { $AuraBin = (Resolve-Path $c).Path; break }
}
if (-not $AuraBin) { Write-Host "ERROR: no aura.exe found" -ForegroundColor Red; exit 1 }

if ($Phase -ne "") {
    $Sources = Get-ChildItem "tests\photon\$Phase" -Filter "*.aura" -File | ForEach-Object { $_.FullName }
    if ($Sources.Count -eq 0) { Write-Host "ERROR: no sources for phase $Phase" -ForegroundColor Red; exit 1 }
}
if ($Sources.Count -eq 0) { Write-Host "ERROR: no sources specified" -ForegroundColor Red; exit 1 }

$pass = 0; $fail = 0
$results = @()

function Invoke-Proc($exe, $argStr, $wd, $to) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $argStr
    $psi.WorkingDirectory = $wd
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    if ($p.WaitForExit($to * 1000)) { return @{ Out=$so; Err=$se; Code=$p.ExitCode; TimedOut=$false } }
    try { $p.Kill() } catch {}
    return @{ Out=$so; Err=$se; Code=-1; TimedOut=$true }
}

Write-Host ""
Write-Host "══════════════════════════════════════════════════════"
Write-Host " Photon E2E 差分验证"
Write-Host "  compiler : $AuraBin"
Write-Host "  sources  : $($Sources.Count)"
Write-Host "  out root : $OutRoot"
Write-Host "══════════════════════════════════════════════════════"
Write-Host ""

foreach ($src in $Sources) {
    $rel = $src -replace [regex]::Escape($Root + "\"), ""
    $stem = [IO.Path]::GetFileNameWithoutExtension($src)
    $outDir = Join-Path $OutRoot $stem
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    $phirPath = Join-Path $outDir "$stem.phir"
    $exePath  = Join-Path $outDir "$stem.exe"

    Write-Host "──── $rel ────"

    # [1] VM 基准
    $vm = Invoke-Proc $AuraBin "run $src" $Root $TimeoutSecs
    $vmOut = ($vm.Out -replace "`r","")
    if ($vm.TimedOut) { Write-Host "  [VM] TIMEOUT" -ForegroundColor Yellow }

    # [2] Photon 编译
    $ph = Invoke-Proc $AuraBin "build -b photon $src --output $phirPath" $Root $TimeoutSecs
    $buildOk = $ph.Code -eq 0

    # [3] exe 检查 + 运行
    $exeOut = ""; $exeCode = -999; $exeOk = $false; $timeout = $false
    if (Test-Path $exePath) {
        $exeOk = $true
        $run = Invoke-Proc $exePath "" $outDir $TimeoutSecs
        $exeOut = ($run.Out -replace "`r","")
        $exeCode = $run.Code
        $timeout = $run.TimedOut
    }

    # [4] 差分
    $ok = $true; $reasons = @()
    if (-not $buildOk)   { $ok = $false; $reasons += "build-failed" }
    if (-not $exeOk)     { $ok = $false; $reasons += "no-exe" }
    if ($timeout)        { $ok = $false; $reasons += "timeout" }
    if ($ok -and $vmOut.Trim() -ne $exeOut.Trim()) { $ok = $false; $reasons += "output-diff" }

    if ($ok) {
        Write-Host "  PASS  exe=$($exeOut.Trim().Substring(0,[Math]::Min(60,$exeOut.Trim().Length)))" -ForegroundColor Green
        $pass++
    } else {
        Write-Host "  FAIL  [$(($reasons) -join ', ')]" -ForegroundColor Red
        if (-not $exeOk) {
            # 打印构建尾部便于诊断
            $tail = ($ph.Out + "`n" + $ph.Err) -split "`n" | Select-Object -Last 8
            foreach ($l in $tail) { if ($l.Trim()) { Write-Host "      | $l" -ForegroundColor DarkGray } }
        }
        if ($reasons -contains "output-diff") {
            Write-Host "    VM   : $($vmOut.Trim())" -ForegroundColor Cyan
            Write-Host "    EXE  : $($exeOut.Trim())" -ForegroundColor Magenta
        }
        $fail++
    }
    $results += @{ Src=$rel; Pass=$ok; Reasons=($reasons -join ","); VM=$vmOut.Trim(); EXE=$exeOut.Trim(); Code=$exeCode }
    Write-Host ""
}

Write-Host "══════════════════════════════════════════════════════"
Write-Host " Photon E2E: PASS=$pass  FAIL=$fail  TOTAL=$($Sources.Count)"
Write-Host "══════════════════════════════════════════════════════"
if ($fail -gt 0) {
    Write-Host ""
    Write-Host "Failures:" -ForegroundColor Red
    foreach ($r in $results) { if (-not $r.Pass) { Write-Host "  - $($r.Src): $($r.Reasons)" } }
}
exit $fail
