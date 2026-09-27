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
    [string]$AuraBin,
    [string]$Lld
)

$ErrorActionPreference = 'Stop'

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root

# NOTE: PowerShell variable names are case-insensitive, so the `-Lld` parameter and
# a local `$lld` would be THE SAME variable. Snapshot it immediately and never
# touch `$Lld` again (otherwise the driver-parsing block below silently wipes it).
$LldOverride = $Lld

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

# Read the lld bin directory from aura.toml's [lld] section.
#
# Why this lives in PowerShell and not only in Aura code: the seed VM cannot
# execute the Aura-side parser (PhotonLldConfig). Its stdlib String.auc does not
# load ("Unlinked external function `String.substring`"), so every
# substring/indexOf/startsWith returns a default value and the Aura parser yields
# an empty directory. aura.toml is the single source of truth for the lld path, so
# the build script reads the same section directly.
function Get-LldDirFromManifest {
    param(
        [string]$Manifest,
        [string]$Triple = 'x86_64-pc-windows-msvc'
    )
    if (-not (Test-Path $Manifest)) { return $null }
    # MUST read as UTF-8 explicitly: the manifest carries Chinese comments, and
    # `Get-Content -Raw` under Windows PowerShell 5.1 decodes with the ANSI code
    # page. That mis-decoding merges bytes across the newline in front of `[lld]`,
    # so the section header is swallowed and the key lookup silently finds nothing.
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
        if ($line.Substring(0, $eq).Trim() -ne $Triple) { continue }
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
# The seed VM's Process.exit is not reliable (stdlib externs are unlinked), so a
# non-zero exit code here does NOT mean the driver failed. Success is decided by
# the emitted markers below.
if ($driverCode -ne 0) {
    Write-Host "[photon-hello] driver exit=$driverCode (ignored; markers decide success)"
}

$mainHex = $null; $rtHex = $null; $lldDriver = $null; $linkArgs = $null; $hexFiles = $null; $mode = ''
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
        if (-not $lldDriver) { $lldDriver = $t }
    } elseif ($mode -eq 'args') {
        if (-not $linkArgs) { $linkArgs = $t }
    }
}
if (-not $mainHex)   { throw "driver did not emit ===MAIN===" }
if (-not $rtHex)     { throw "driver did not emit ===RUNTIME===" }

# ---- Resolve lld-link ----
# Precedence: -Lld (explicit) > driver value (only if it is a real file) >
#             aura.toml [lld] (the single source of truth) > PATH.
#
# The driver value matters only when it is an existing path: the Aura-side
# PhotonLldConfig cannot read aura.toml under the seed VM (its String helpers are
# unlinked externals), so it falls back to the *bare* name `lld-link.exe`, which
# is not a file. Treating that as "resolved" was the earlier bug.
$lldPath = $null
if ($LldOverride) {
    $lldPath = $LldOverride
} elseif ($lldDriver -and (Test-Path $lldDriver)) {
    $lldPath = $lldDriver
    Write-Host "[photon-hello] lld from driver : $lldPath"
}

if (-not $lldPath) {
    $lldDir = Get-LldDirFromManifest -Manifest (Join-Path $Root 'aura.toml')
    if ($lldDir) {
        $lldPath = Join-Path $lldDir 'lld-link.exe'
        Write-Host "[photon-hello] lld from aura.toml [lld] : $lldDir"
    }
}

if (-not $lldPath) { $lldPath = 'lld-link.exe' }
if (-not (Test-Path $lldPath)) {
    $resolved = Get-Command $lldPath -ErrorAction SilentlyContinue
    if (-not $resolved) {
        throw "lld-link not found (driver='$lldDriver'): pass -Lld <path>, or fix aura.toml [lld]"
    }
    $lldPath = $resolved.Source
}
$lld = $lldPath

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

# Link arguments: prefer the driver's list; otherwise build the same command the
# Aura-side PhotonSystemLinker would produce. See Get-LldDirFromManifest for why
# the Aura-side string helpers are unusable under the seed VM.
if (-not $linkArgs) {
    $objMain = Join-Path $OutDir 'hello.obj'
    $objRt   = Join-Path $OutDir 'aura_runtime.obj'
    $outStem = Join-Path $OutDir 'hello'
    $linkArgs = "$objMain $objRt /OUT:$outStem" +
        " /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB"
    Write-Host "[photon-hello] link args : (built by script; driver emitted none)"
}

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
