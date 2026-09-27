# -------------------------------------------------------------
# Photon HAT v2.0 端到端构建脚本
#
# 管线: HAT 文本 -> SSA MIR -> LIR -> X86 -> COFF -> lld-link -> exe
#
# 用法:
#   scripts\build-photon-hat.ps1
#   scripts\build-photon-hat.ps1 -Keep
#
# 前置: rust\target\release\aura.exe 已编译
#       LLVM lld-link 可用 (aura.toml [lld] 或 PATH)
# -------------------------------------------------------------
param(
    [switch]$Keep,
    [string]$HatSrc = "tests/hat/hello_world.hat",
    [string]$OutDir = "build/hat_test",
    [string]$Module = "hello",
    [string]$Lld,
    [string]$SdkLib
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$LldOverride = $Lld

# --- 1. Find aura.exe ---
function Find-Aura {
    foreach ($c in @(
        'rust\target\release\aura.exe', 'aura\seed\aura.exe',
        'build\bin\aura.exe', 'target\release\aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

# --- 2. Find lld-link ---
function Get-LldDirFromManifest {
    param([string]$Manifest)
    if (-not (Test-Path $Manifest)) { return $null }
    $text = [System.IO.File]::ReadAllText($Manifest, [System.Text.Encoding]::UTF8)
    $inLld = $false
    foreach ($raw in ($text -split '\r?\n')) {
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        if ($line.StartsWith('[')) {
            $inLld = ($line -eq '[lld]')
            continue
        }
        if (-not $inLld) { continue }
        $eq = $line.IndexOf('=')
        if ($eq -le 0) { continue }
        $key = $line.Substring(0, $eq).Trim()
        if ($key -ne 'x86_64-pc-windows-msvc') { continue }
        $val = $line.Substring($eq + 1).Trim()
        if ($val.StartsWith('"')) {
            $end = $val.IndexOf('"', 1)
            if ($end -gt 1) { $val = $val.Substring(1, $end - 1) }
        } else {
            $h = $val.IndexOf('#')
            if ($h -ge 0) { $val = $val.Substring(0, $h).Trim() }
        }
        return $val
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

function Invoke-Captured {
    param([string]$Exe, [string[]]$ArgList)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $psi.Arguments = (($ArgList | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"','\"') + '"' } else { $_ } }) -join ' ')
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return @{ Out = $so; Err = $se; Code = $p.ExitCode }
}

# --- 3. Run the driver ---
$AuraBin = Find-Aura
if (-not $AuraBin) { throw "aura.exe not found" }
Write-Host "[photon-hat] compiler : $AuraBin" -ForegroundColor Cyan
Write-Host "[photon-hat] source   : $HatSrc"
Write-Host "[photon-hat] outDir   : $OutDir"

$env:AURA_HAT_SRC = $HatSrc
$env:AURA_HAT_OUT = $OutDir
$env:AURA_HAT_MODULE = $Module

$Driver = 'aura/compiler/aura/lang/compiler/backend/photon/PhotonHatBuild.aura'
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$lines = & $AuraBin run $Driver 2>$null
$driverCode = $LASTEXITCODE
$ErrorActionPreference = $prevEap

if ($driverCode -ne 0) {
    Write-Host "[photon-hat] driver exit=$driverCode (ignored; markers decide)" -ForegroundColor Yellow
}

# --- 4. Parse markers ---
$coffMain = $null; $coffRt = $null; $linkCmd = $null; $mode = ''
foreach ($l in $lines) {
    $t = $l.Trim()
    if ($t -eq '===COFF-MAIN===')    { $mode = 'main'; continue }
    if ($t -eq '===COFF-RUNTIME===') { $mode = 'rt';   continue }
    if ($t -eq '===LINK===')         { $mode = 'link'; continue }
    if ($t -eq '===RESULT===')       { $mode = 'result'; continue }
    if ($t -eq '===ERR===')          { $mode = 'err';  continue }
    if ($t -eq '') { continue }
    if ($t -match '^[0-9a-fA-F]{40,}$') {
        if ($mode -eq 'main') { $coffMain = $t }
        elseif ($mode -eq 'rt') { $coffRt = $t }
    } elseif ($mode -eq 'link') {
        if (-not $linkCmd) { $linkCmd = $t }
    }
}

if (-not $coffMain) { throw "driver did not emit ===COFF-MAIN===" }
if (-not $coffRt)   { throw "driver did not emit ===COFF-RUNTIME===" }

Write-Host "[photon-hat] COFF main    : $($coffMain.Length / 2) bytes"
Write-Host "[photon-hat] COFF runtime : $($coffRt.Length / 2) bytes"

# --- 5. Resolve lld-link ---
$lldPath = $null
if ($LldOverride) {
    $lldPath = $LldOverride
} elseif ($linkCmd -and ($linkCmd -match '^(\S+)')) {
    $candidate = $linkCmd -replace '/','\'  | ForEach-Object { ($_ -split '\s+')[0] }
    if (Test-Path $candidate) {
        $lldPath = $candidate
        Write-Host "[photon-hat] lld from link  : $lldPath"
    }
}

if (-not $lldPath) {
    $lldDir = Get-LldDirFromManifest -Manifest (Join-Path $Root 'aura.toml')
    if ($lldDir) {
        $lldPath = Join-Path $lldDir 'lld-link.exe'
        Write-Host "[photon-hat] lld from aura.toml : $lldDir"
    }
}

if (-not $lldPath) { $lldPath = 'lld-link.exe' }
if (-not (Test-Path $lldPath)) {
    $resolved = Get-Command $lldPath -ErrorAction SilentlyContinue
    if (-not $resolved) { throw "lld-link not found" }
    $lldPath = $resolved.Source
}
Write-Host "[photon-hat] lld-link     : $lldPath"

# --- 6. Write .obj files ---
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$objMain = Join-Path $OutDir "$Module.obj"
$objRt   = Join-Path $OutDir "aura_runtime.obj"
$n1 = Write-HexFile -Hex $coffMain -Path $objMain
$n2 = Write-HexFile -Hex $coffRt   -Path $objRt
Write-Host "[photon-hat] $objMain    : $n1 bytes"
Write-Host "[photon-hat] $objRt      : $n2 bytes"

# --- 7. Find kernel32.lib ---
$k32 = Find-Kernel32 -Explicit $SdkLib
if (-not $k32) { throw "kernel32.Lib not found; pass -SdkLib <path>" }
Write-Host "[photon-hat] kernel32.Lib : $k32"

# --- 8. Link ---
# /OUT must carry the .exe extension explicitly: without it this lld-link writes
# an extension-less file, while step 9 verifies/runs "$Module.exe" and would then
# execute a stale leftover exe from an earlier run (spurious FAIL).
$exe = Join-Path $OutDir "$Module.exe"
Remove-Item $exe -Force -ErrorAction SilentlyContinue
$linkArgs = @($objMain, $objRt, "/OUT:$exe", "/SUBSYSTEM:CONSOLE", "/ENTRY:main", "/MACHINE:X64", "/NODEFAULTLIB", $k32)
Write-Host "[photon-hat] linking..."
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$linkOut = & $lldPath @linkArgs 2>&1
$ErrorActionPreference = $prevEap
$code = $LASTEXITCODE
if ($code -ne 0) {
    Write-Host $linkOut
    throw "lld-link failed with exit code $code"
}

if (-not (Test-Path $exe)) {
    throw "exe not produced: $exe"
}
Write-Host "[photon-hat] linked       : $exe ($((Get-Item $exe).Length) bytes)" -ForegroundColor Green

# --- 9. Run and verify ---
Write-Host "[photon-hat] running..."
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$actual = & $exe 2>$null
$ErrorActionPreference = $prevEap
$exitCode = $LASTEXITCODE
$hex = ($actual.ToCharArray() | ForEach-Object { '{0:X2}' -f [int]$_ }) -join ' '
Write-Host "[photon-hat] stdout hex   : $hex"
Write-Host "[photon-hat] stdout text  : $actual"
Write-Host "[photon-hat] exit code    : $exitCode"

if ($exitCode -eq 0 -and $actual.Contains("Hello")) {
    Write-Host ""
    Write-Host "=== PASS: HAT IR Hello World compiled and executed successfully ===" -ForegroundColor Green
    Write-Host "    Pipeline: HAT -> SSA MIR -> LIR -> X86 -> COFF -> lld-link -> exe" -ForegroundColor Cyan
} else {
    Write-Host ""
    Write-Host "=== FAIL: unexpected output or exit code ===" -ForegroundColor Red
    exit 1
}

if (-not $Keep) {
    Remove-Item $objMain, $objRt -Force -ErrorAction SilentlyContinue
}
