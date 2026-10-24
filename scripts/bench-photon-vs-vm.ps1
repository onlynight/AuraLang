# -------------------------------------------------------------
# 3-way benchmark: Photon HAT AOT vs Aura VM (AOT substrate) vs Rust VM
# ASCII-only for PS 5.1 safety.
#
#   Photon AOT : PhotonHatCompile -> COFF -> lld-link -> native exe
#   Aura VM    : build/bin/auc_vm_runner.exe (pure-Aura Vm.aura compiled
#                AOT by the seed; loads build/vm/current.auc)
#   Rust VM    : seed `aura run <src>` (bytecode interpreter)
#
# Usage: powershell -File scripts\bench-photon-vs-vm.ps1 [-Runs 5] [-ThreeWay]
# -------------------------------------------------------------

param(
    [int]$Runs = 5,
    [string]$OutRoot = "build\bench",
    [switch]$ThreeWay
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"

$Aura = Join-Path $Root 'seed\target\release\aura.exe'
$Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
$Runner = Join-Path $Root 'build\bin\auc_vm_runner.exe'
foreach ($tool in @($Aura, $Driver)) {
    if (-not (Test-Path $tool)) { Write-Host "missing: $tool" -ForegroundColor Red; exit 1 }
}
$haveRunner = Test-Path $Runner

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

function Invoke-Proc([string]$exe, [string]$argsStr, [string]$workDir, [int]$timeoutMs) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $argsStr
    $psi.WorkingDirectory = $workDir
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $soT = $p.StandardOutput.ReadToEndAsync()
    $seT = $p.StandardError.ReadToEndAsync()
    if (-not $p.WaitForExit($timeoutMs)) {
        try { $p.Kill() } catch {}
        return @{ Ok = $false; Out = ''; Code = -999 }
    }
    $so = ''; $se = ''
    try { $so = $soT.Result } catch {}
    try { $se = $seT.Result } catch {}
    return @{ Ok = $true; Out = $so; Err = $se; Code = $p.ExitCode }
}

function Time-Runs([string]$exe, [string]$argsStr, [string]$workDir, [int]$runs, [int]$timeoutMs) {
    $times = @()
    $lastOut = ''
    for ($i = 0; $i -lt $runs; $i++) {
        $sw = [Diagnostics.Stopwatch]::StartNew()
        $r = Invoke-Proc $exe $argsStr $workDir $timeoutMs
        $sw.Stop()
        if (-not $r.Ok) { return @{ Min = -1; Med = -1; Out = '<TIMEOUT>' } }
        $times += [math]::Round($sw.Elapsed.TotalMilliseconds, 0)
        $lastOut = (($r.Out -split "`r?`n") | Where-Object { $_.Trim() -ne '' } | Select-Object -Last 1)
    }
    $sorted = $times | Sort-Object
    return @{ Min = $sorted[0]; Med = $sorted[[int][Math]::Floor($sorted.Count / 2)]; Out = $lastOut }
}

function Write-HexFile([string]$Hex, [string]$Path) {
    $bytes = New-Object byte[] ($Hex.Length / 2)
    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $bytes[$i] = [Convert]::ToByte($Hex.Substring($i * 2, 2), 16)
    }
    [System.IO.File]::WriteAllBytes($Path, $bytes)
}

$lld = (Get-Command 'lld-link.exe' -ErrorAction SilentlyContinue).Source
if (-not $lld) { $lld = 'lld-link.exe' }
$k32 = Find-Kernel32

$benches = @('bench_fib', 'bench_loop')
$results = @()

foreach ($b in $benches) {
    $src = Join-Path $Root "$OutRoot\$b.aura"
    $outDir = Join-Path $Root "$OutRoot\$b"
    if (Test-Path $outDir) { Remove-Item $outDir -Recurse -Force -ErrorAction SilentlyContinue }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null

    Write-Host "=== $b ===" -ForegroundColor Cyan

    # ---- Photon AOT ----
    $env:AURA_HAT_AURA   = $src
    $env:AURA_HAT_SRC    = (Join-Path $outDir "$b.hat")
    $env:AURA_HAT_OUT    = $outDir
    $env:AURA_HAT_MODULE = $b
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $d = Invoke-Proc $Driver '' $Root (600 * 1000)
    $sw.Stop()
    $compileSecs = [math]::Round($sw.Elapsed.TotalSeconds, 1)

    $coffMain = $null; $coffRt = $null; $mode = ''
    foreach ($l in ($d.Out -split "`r?`n")) {
        $t = $l.Trim()
        if ($t -eq '===COFF-MAIN===')    { $mode = 'main'; continue }
        if ($t -eq '===COFF-RUNTIME===') { $mode = 'rt';   continue }
        if ($t -match '^[0-9a-fA-F]{40,}$') {
            if ($mode -eq 'main' -and -not $coffMain) { $coffMain = $t }
            elseif ($mode -eq 'rt' -and -not $coffRt) { $coffRt = $t }
        }
    }
    $aotMin = -1; $aotMed = -1
    if ($coffMain -and $coffRt -and $k32) {
        $objMain = Join-Path $outDir "$b.obj"
        $objRt   = Join-Path $outDir 'aura_runtime.obj'
        Write-HexFile $coffMain $objMain | Out-Null
        Write-HexFile $coffRt   $objRt   | Out-Null
        $exe = Join-Path $outDir "$b.exe"
        $lk = Invoke-Proc $lld ('"'+$objMain+'" "'+$objRt+'" /OUT:"'+$exe+'" "'+$k32+'" /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB') $Root (120 * 1000)
        if ($lk.Ok -and (Test-Path $exe)) {
            $aot = Time-Runs $exe ('"' + $exe + '"') $Root $Runs (120 * 1000)
            $aotMin = $aot.Min; $aotMed = $aot.Med
        }
    }

    # ---- Aura VM (AOT substrate): compile to .auc, copy to fixed channel ----
    $avmMin = -1; $avmMed = -1
    if ($ThreeWay -and $haveRunner) {
        $auc = Join-Path $Root 'build\vm\current.auc'
        [void](Invoke-Proc $Aura ('build "' + $src + '" --output "' + $auc + '"') $Root (300 * 1000))
        if (Test-Path $auc) {
            $avm = Time-Runs $Runner '' $Root $Runs (1800 * 1000)
            $avmMin = $avm.Min; $avmMed = $avm.Med
        }
    }

    # ---- Rust VM ----
    $vm = Time-Runs $Aura ('run "' + $src + '"') $Root $Runs (600 * 1000)

    $spPhoton = if ($aotMed -gt 0) { [math]::Round($vm.Med / $aotMed, 1) } else { -1 }
    $spAuraVm = if ($avmMed -gt 0) { [math]::Round($avmMed / $aotMed, 1) } else { -1 }
    Write-Host ("  Photon AOT: compile={0}s run med={1}ms   AuraVM med={2}ms (x{3})   RustVM med={4}ms (x{5})" -f `
        $compileSecs, $aotMed, $avmMed, $spAuraVm, $vm.Med, $spPhoton) -ForegroundColor Green
    $results += [pscustomobject]@{
        Bench = $b; AotCompileSecs = $compileSecs
        AotMedMs = $aotMed; AuraVmMedMs = $avmMed; RustVmMedMs = $vm.Med
        SpeedupVsRustVm = $spPhoton; SpeedupVsAuraVm = $spAuraVm
    }
}

Write-Host ''
Write-Host '=== [bench] summary (median) ===' -ForegroundColor Cyan
$results | Format-Table -AutoSize | Out-String | Write-Host
