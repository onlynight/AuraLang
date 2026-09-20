# -------------------------------------------------------------
# Photon E3 smoke test: build and run "hello world" - Windows
#
# Pipeline:
#   1. aura/seed/aura.exe run PhotonHelloBuild.aura
#        -> COFF hex for main.obj / aura_runtime.obj
#        -> lld-link path (resolved from aura.toml [lld])
#        -> lld-link arguments (without kernel32.lib)
#   2. hex -> .obj  (this script; Aura/VM has no raw byte writer)
#   3. lld-link <args> <kernel32.lib> -> hello.exe
#   4. run hello.exe and verify stdout bytes
#
# Usage:
#   scripts\build-photon-hello.ps1
#   scripts\build-photon-hello.ps1 -SdkLib "C:\path\to\kernel32.Lib"
#   scripts\build-photon-hello.ps1 -Keep      # keep .obj / .hex files
#
# NOTE: this script is ASCII-only on purpose, so it runs correctly
#       under Windows PowerShell 5.1 regardless of the active code page.
# -------------------------------------------------------------
param(
    [switch]$Keep,
    [string]$SdkLib,
    [string]$AuraBin
)

$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

$Driver = 'aura/compiler/aura/lang/compiler/backend/photon/PhotonHelloBuild.aura'
$OutDir = Join-Path $Root 'build/lldtest'
$Expect = 'hello world' + "`r`n"

function Find-Aura {
    foreach ($c in @(
        'aura/seed/aura.exe', 'build/bin/aura.exe',
        'target/release/aura.exe', 'target/debug/aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Find-Kernel32 {
    param([string]$Explicit, [string]$Arch = 'x64')
    if ($Explicit) {
        if (-not (Test-Path $Explicit)) { throw "kernel32.Lib not found: $Explicit" }
        return (Resolve-Path $Explicit).Path
    }
    $roots = @()
    if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Lib') }
    if ($env:ProgramFiles)        { $roots += (Join-Path $env:ProgramFiles 'Windows Kits/10/Lib') }
    foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        $vers = Get-ChildItem $r -Directory | Sort-Object Name -Descending
        foreach ($v in $vers) {
            $p = Join-Path $v.FullName ("um/$Arch/kernel32.Lib")
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

function Quote-Arg {
    param([string]$a)
    if ($a -match '[\s"]') { return '"' + ($a -replace '"', '\"') + '"' }
    return $a
}

# NOTE: uses StartupInfo.Arguments (not ArgumentList) so it also works on
#       Windows PowerShell 5.1 / .NET Framework, where ArgumentList is absent.
function Invoke-Captured {
    param([string]$Exe, [string[]]$ArgList)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.Arguments = (($ArgList | ForEach-Object { Quote-Arg $_ }) -join ' ')
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return @{ Out = $so; Err = $se; Code = $p.ExitCode }
}

# ---- 1. Run the Photon driver -----------------------------------
if (-not $AuraBin) { $AuraBin = Find-Aura }
if (-not $AuraBin) { throw "no Aura compiler found (aura/seed/aura.exe, build/bin/aura.exe, target/...)" }
Write-Host "[photon-hello] compiler : $AuraBin"
Write-Host "[photon-hello] driver   : $Driver"

# NOTE: the Aura VM writes semantic warnings to stderr; with
#       $ErrorActionPreference='Stop' PS 5.1 turns that into a terminating
#       NativeCommandError, so relax it for the duration of this call.
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$lines = & $AuraBin run $Driver 2>$null
$driverCode = $LASTEXITCODE
$ErrorActionPreference = $prevEap
if ($driverCode -ne 0) { throw "driver failed with exit code $driverCode" }

$mainHex = $null; $rtHex = $null; $lld = $null; $linkArgs = $null; $hexFiles = $null; $mode = ''
foreach ($l in $lines) {
    $t = $l.Trim()
    if ($t -eq '===MAIN===')      { $mode = 'main'; continue }
    if ($t -eq '===RUNTIME===')   { $mode = 'rt';   continue }
    if ($t -eq '===HEX-FILES===') { $mode = 'hexfiles'; continue }
    if ($t -eq '===LLD===')       { $mode = 'lld';  continue }
    if ($t -eq '===LINK-ARGS===') { $mode = 'args'; continue }
    if ($t -eq '') { continue }
    if ($t -match '^[0-9a-f]{40,}$') {
        if ($mode -eq 'main') { $mainHex = $t } elseif ($mode -eq 'rt') { $rtHex = $t }
    } elseif ($mode -eq 'hexfiles') {
        if (-not $hexFiles) { $hexFiles = $t }
    } elseif ($mode -eq 'lld') {
        if (-not $lld) { $lld = $t }
    } elseif ($mode -eq 'args') {
        if (-not $linkArgs) { $linkArgs = $t }
    }
}
if (-not $mainHex)   { throw "driver did not emit ===MAIN===" }
if (-not $rtHex)     { throw "driver did not emit ===RUNTIME===" }
if (-not $lld)       { throw "driver did not emit ===LLD===" }
if (-not $linkArgs)  { throw "driver did not emit ===LINK-ARGS===" }

# ---- 1b. Prefer the .hex files written by the driver -----------------
# The driver writes `<name>.obj.hex` through PhotonObjectWriterUtils.saveHexFile()
# (VM-path file output). Use them when they are present AND well-formed; otherwise
# fall back to the hex captured from stdout (which always works).
$mainHexPath = $null
$rtHexPath   = $null
if ($hexFiles) {
    $f = @($hexFiles -split '\s+' | Where-Object { $_ -ne '' })
    if ($f.Count -ge 4 -and $f[1] -eq 'OK' -and $f[3] -eq 'OK') {
        $candidateMain = Join-Path $Root $f[0]
        $candidateRt   = Join-Path $Root $f[2]
        if ((Test-Path $candidateMain) -and (Test-Path $candidateRt)) {
            $fileMain = (Get-Content $candidateMain -Raw).Trim()
            $fileRt   = (Get-Content $candidateRt -Raw).Trim()
            if ($fileMain -match '^[0-9a-fA-F]+$' -and $fileRt -match '^[0-9a-fA-F]+$') {
                if ($fileMain -ne $mainHex -or $fileRt -ne $rtHex) {
                    throw "hex file content differs from stdout (main equal=$($fileMain -eq $mainHex), runtime equal=$($fileRt -ne $rtHex))"
                }
                $mainHexPath = $candidateMain
                $rtHexPath   = $candidateRt
                $mainHex = $fileMain.ToLower()
                $rtHex   = $fileRt.ToLower()
                Write-Host "[photon-hello] hex files  : $($f[0]) / $($f[2])"
            }
        }
    }
}
if (-not $mainHexPath) {
    Write-Host "[photon-hello] hex files  : (not available, using stdout hex)"
}

Write-Host "[photon-hello] lld-link : $lld"

# ---- 2. hex -> object files -------------------------------------
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$n1 = Write-HexFile -Hex $mainHex -Path (Join-Path $OutDir 'hello.obj')
$n2 = Write-HexFile -Hex $rtHex   -Path (Join-Path $OutDir 'aura_runtime.obj')
Write-Host "[photon-hello] hello.obj        : $n1 bytes"
Write-Host "[photon-hello] aura_runtime.obj : $n2 bytes"

# ---- 3. link ----------------------------------------------------
$k32 = Find-Kernel32 -Explicit $SdkLib
if (-not $k32) { throw "kernel32.Lib not found; pass -SdkLib <path>" }
Write-Host "[photon-hello] kernel32.Lib : $k32"

$argv = @($linkArgs -split '\s+' | Where-Object { $_ -ne '' })
$argv += $k32
$res = Invoke-Captured -Exe $lld -ArgList $argv
if ($res.Code -ne 0) {
    Write-Host $res.Out
    Write-Host $res.Err
    throw "lld-link failed with exit code $($res.Code)"
}
$exe = Join-Path $OutDir 'hello.exe'
Write-Host "[photon-hello] linked    : $exe ($((Get-Item $exe).Length) bytes)"

# ---- 4. run -----------------------------------------------------
$run = Invoke-Captured -Exe $exe -ArgList @()
$actual = $run.Out
$hex = ($actual.ToCharArray() | ForEach-Object { '{0:X2}' -f [int]$_ }) -join ' '
Write-Host "[photon-hello] stdout    : $hex"
Write-Host "[photon-hello] exit code : $($run.Code)"

if ($actual -ne $Expect) {
    throw "unexpected stdout: got [$actual], expected [$Expect]"
}
Write-Host "[photon-hello] OK: hello world printed via Photon runtime" -ForegroundColor Green

if (-not $Keep) {
    Remove-Item (Join-Path $OutDir 'hello.obj'), (Join-Path $OutDir 'aura_runtime.obj'),
                (Join-Path $OutDir 'hello.obj.hex'), (Join-Path $OutDir 'aura_runtime.obj.hex') `
                -Force -ErrorAction SilentlyContinue
}
