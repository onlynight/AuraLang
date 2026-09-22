# -------------------------------------------------------------
# Photon full compilation pipeline - Windows
#
# Pipeline:
#   1. aura build -b photon <source> -> HIR (Rust front-end)
#   2. aura run PhotonDriver.aura -> COFF (Aura back-end)
#   3. lld-link -> executable
#   4. run executable and verify
#
# Usage:
#   scripts\build-photon-full.ps1 <source-file> [-OutDir <dir>] [-Keep]
#   scripts\build-photon-full.ps1 <source-file> -DryRun
# -------------------------------------------------------------
param(
    [Parameter(Position=0, Mandatory=$true)]
    [string]$Source,
    
    [string]$OutDir,
    [switch]$Keep,
    [switch]$DryRun,
    [string]$AuraBin,
    [string]$Lld
)

$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

$LldOverride = $Lld

if (-not $OutDir) {
    $OutDir = Join-Path $Root 'build/photon'
}

$SourceAbs = (Resolve-Path $Source).Path
$OutDirAbs = Join-Path $Root $OutDir
$ModuleName = [System.IO.Path]::GetFileNameWithoutExtension($SourceAbs)

Write-Host "╔══════════════════════════════════════════════╗"
Write-Host "║   Photon Full Compilation Pipeline           ║"
Write-Host "╚══════════════════════════════════════════════╝"
Write-Host "  Source:   $SourceAbs"
Write-Host "  Output:   $OutDirAbs"
Write-Host "  Module:   $ModuleName"

# Create output directory
New-Item -ItemType Directory -Force -Path $OutDirAbs | Out-Null

function Find-Aura {
    foreach ($c in @(
        'rust/target/release/aura.exe', 'build/bin/aura.exe',
        'aura/seed/aura.exe', 'target/release/aura.exe', 'target/debug/aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Find-Lld {
    param([string]$Explicit)
    if ($Explicit) {
        if (Test-Path $Explicit) { return $Explicit }
    }
    
    # Read from aura.toml
    $manifest = Join-Path $Root 'aura.toml'
    if (Test-Path $manifest) {
        $text = [System.IO.File]::ReadAllText($manifest, [System.Text.Encoding]::UTF8)
        $inLld = $false
        foreach ($line in ($text -split '\r?\n')) {
            $line = $line.Trim()
            if ($line -eq '' -or $line.StartsWith('#')) { continue }
            if ($line.StartsWith('[')) {
                $inLld = ($line -eq '[lld]')
                continue
            }
            if (-not $inLld) { continue }
            if ($line -match '^x86_64-pc-windows-msvc\s*=\s*"(.+)"') {
                $lldDir = $Matches[1]
                $lldPath = Join-Path $lldDir 'lld-link.exe'
                if (Test-Path $lldPath) { return $lldPath }
            }
        }
    }
    
    # Try PATH
    $resolved = Get-Command 'lld-link.exe' -ErrorAction SilentlyContinue
    if ($resolved) { return $resolved.Source }
    
    return $null
}

function Find-Kernel32 {
    $roots = @()
    if (${env:ProgramFiles(x86)}) { $roots += (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/Lib') }
    if ($env:ProgramFiles)        { $roots += (Join-Path $env:ProgramFiles 'Windows Kits/10/Lib') }
    foreach ($r in $roots) {
        if (-not (Test-Path $r)) { continue }
        $vers = Get-ChildItem $r -Directory | Sort-Object Name -Descending
        foreach ($v in $vers) {
            $p = Join-Path $v.FullName "um/x64/kernel32.Lib"
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
    param([string]$Exe, [string[]]$ArgList, [string]$WorkDir)
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    if ($WorkDir) { $psi.WorkingDirectory = $WorkDir }
    $psi.Arguments = ($ArgList -join ' ')
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    $p.WaitForExit()
    return @{ Out = $so; Err = $se; Code = $p.ExitCode }
}

# ---- 1. Find tools ----
if (-not $AuraBin) { $AuraBin = Find-Aura }
if (-not $AuraBin) { throw "no Aura compiler found" }
Write-Host "  Aura:     $AuraBin"

$lld = Find-Lld -Explicit $LldOverride
if (-not $lld) { throw "lld-link not found" }
Write-Host "  lld-link: $lld"

$k32 = Find-Kernel32
if (-not $k32) { throw "kernel32.Lib not found" }
Write-Host "  kernel32: $k32"

if ($DryRun) {
    Write-Host "`nDry run mode - no compilation performed"
    exit 0
}

# ---- 2. Run Rust front-end to generate HIR ----
Write-Host "`n[1/3] Running Rust front-end (aura build -b photon)..."

$hirOut = Join-Path $OutDirAbs "$ModuleName.hir"
$buildArgs = @('build', '-b', 'photon', $SourceAbs, '--output', $hirOut)

$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$res = Invoke-Captured -Exe $AuraBin -ArgList $buildArgs -WorkDir $Root
$ErrorActionPreference = $prevEap

if ($res.Code -ne 0) {
    Write-Host $res.Err
    throw "Front-end failed with exit code $($res.Code)"
}

Write-Host $res.Out

if (-not (Test-Path $hirOut)) {
    throw "HIR output not found: $hirOut"
}

$hirSize = (Get-Item $hirOut).Length
Write-Host "  HIR generated: $hirOut ($hirSize bytes)"

# ---- 3. Run Photon backend via Aura VM ----
Write-Host "`n[2/3] Running Photon backend (aura run PhotonDriver.aura)..."

$driverPath = 'aura/compiler/aura/lang/compiler/backend/photon/PhotonDriver.aura'
$driverArgs = @('run', $driverPath)

$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$res = Invoke-Captured -Exe $AuraBin -ArgList $driverArgs -WorkDir $Root
$ErrorActionPreference = $prevEap

Write-Host $res.Out

# Parse driver output for markers
$mainHex = $null
$rtHex = $null
$hexFiles = $null
$mode = ''

foreach ($l in $res.Out -split "`r?`n") {
    $t = $l.Trim()
    if ($t -eq '===MAIN===')      { $mode = 'main'; continue }
    if ($t -eq '===RUNTIME===')   { $mode = 'rt';   continue }
    if ($t -eq '===HEX-FILES===') { $mode = 'hexfiles'; continue }
    if ($t -eq '===RESULT===')    { $mode = 'result'; continue }
    if ($t -eq '') { continue }
    
    if ($t -match '^[0-9a-f]{40,}$') {
        if ($mode -eq 'main') { $mainHex = $t } 
        elseif ($mode -eq 'rt') { $rtHex = $t }
    }
    elseif ($mode -eq 'hexfiles' -and -not $hexFiles) {
        $hexFiles = $t
    }
}

# Prefer hex files if available
$mainObjPath = Join-Path $OutDirAbs "$ModuleName.obj"
$rtObjPath = Join-Path $OutDirAbs "aura_runtime.obj"

if ($hexFiles) {
    $f = @($hexFiles -split '\s+' | Where-Object { $_ -ne '' })
    if ($f.Count -ge 4 -and $f[1] -eq 'OK' -and $f[3] -eq 'OK') {
        $candidateMain = Join-Path $Root $f[0]
        $candidateRt   = Join-Path $Root $f[2]
        if ((Test-Path $candidateMain) -and (Test-Path $candidateRt)) {
            Copy-Item $candidateMain $mainObjPath -Force
            Copy-Item $candidateRt $rtObjPath -Force
            Write-Host "  Using hex files from driver"
            $mainHex = $null  # Already have .obj files
            $rtHex = $null
        }
    }
}

# If we have hex strings, write them to .obj files
if ($mainHex) {
    $n1 = Write-HexFile -Hex $mainHex -Path $mainObjPath
    Write-Host "  $ModuleName.obj: $n1 bytes"
}
if ($rtHex) {
    $n2 = Write-HexFile -Hex $rtHex -Path $rtObjPath
    Write-Host "  aura_runtime.obj: $n2 bytes"
}

if (-not (Test-Path $mainObjPath)) {
    throw "Main object file not found: $mainObjPath"
}

# ---- 4. Link ----
Write-Host "`n[3/3] Linking executable..."

$outExe = Join-Path $OutDirAbs "$ModuleName.exe"
$linkArgs = @($mainObjPath, $rtObjPath, "/OUT:$outExe", "/SUBSYSTEM:CONSOLE", "/ENTRY:main", "/MACHINE:X64", "/NODEFAULTLIB", $k32)

$res = Invoke-Captured -Exe $lld -ArgList $linkArgs -WorkDir $OutDirAbs

if ($res.Code -ne 0) {
    Write-Host $res.Out
    Write-Host $res.Err
    throw "lld-link failed with exit code $($res.Code)"
}

$exeSize = (Get-Item $outExe).Length
Write-Host "  Linked: $outExe ($exeSize bytes)"

# ---- 5. Run and verify ----
Write-Host "`nRunning executable..."
$res = Invoke-Captured -Exe $outExe -ArgList @()
Write-Host "  stdout: $($res.Out)"
Write-Host "  stderr: $($res.Err)"
Write-Host "  exit code: $($res.Code)"

if ($res.Code -eq 0) {
    Write-Host "`n[PASS] Compilation and execution succeeded!" -ForegroundColor Green
} else {
    Write-Host "`n[FAIL] Executable exited with non-zero code" -ForegroundColor Red
}

# Cleanup
if (-not $Keep) {
    Remove-Item $mainObjPath, $rtObjPath, $hirOut -Force -ErrorAction SilentlyContinue
}

exit $res.Code