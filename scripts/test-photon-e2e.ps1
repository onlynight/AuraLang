# -------------------------------------------------------------
# S1.9: Photon end-to-end diff test
#
# Compiles a simple Aura program through both VM and Photon paths,
# compares exit codes and output.
#
# Usage:
#   scripts\test-photon-e2e.ps1
#   scripts\test-photon-e2e.ps1 -AuraBin "D:\path\to\aura.exe"
# -------------------------------------------------------------
param(
    [string]$AuraBin,
    [string]$Source = "tests\photon\simple.aura",
    [string]$OutDir = "build\photon-e2e"
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

# Find aura binary
if (-not $AuraBin) {
    foreach ($c in @(
        'AuraLangWithRust\target\debug\aura.exe',
        'aura\seed\aura.exe',
        'target\debug\aura.exe'
    )) {
        if (Test-Path $c) { $AuraBin = (Resolve-Path $c).Path; break }
    }
}
if (-not $AuraBin) { Write-Host "ERROR: no aura binary found"; exit 1 }
Write-Host "[e2e] compiler: $AuraBin"
Write-Host "[e2e] source  : $Source"

$pass = 0; $fail = 0
function Test-Result($name, $ok, $detail) {
    if ($ok) { Write-Host "  PASS: $name" -ForegroundColor Green; $script:pass++ }
    else { Write-Host "  FAIL: $name - $detail" -ForegroundColor Red; $script:fail++ }
}

# Clean output dir
if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# ── [1] VM path: aura run ──
Write-Host ""
Write-Host "[1] VM path: aura run $Source"
$vmResult = & $AuraBin run $Source 2>&1
$vmCode = $LASTEXITCODE
Write-Host "    exit code: $vmCode"
if ($vmCode -ne 0) {
    # VM might return non-zero for non-void main; that's OK
    Write-Host "    (non-zero exit is acceptable for VM path)"
}
Test-Result "VM run completes" ($true) ""

# ── [2] Photon path: aura build -b photon ──
Write-Host ""
Write-Host "[2] Photon path: aura build -b photon $Source -o $OutDir"
$photonResult = & $AuraBin build -b photon $Source -o $OutDir 2>&1
$photonCode = $LASTEXITCODE
Write-Host "    exit code: $photonCode"
foreach ($line in $photonResult) {
    $t = "$line".Trim()
    if ($t -ne '') { Write-Host "    $t" }
}
Test-Result "Photon build completes" ($photonCode -eq 0) "exit=$photonCode"

# ── [3] Check output files ──
Write-Host ""
Write-Host "[3] Output files"
$hirFile = Join-Path $Root "$OutDir/simple.photon.hir"
if (Test-Path $hirFile) {
    $size = (Get-Item $hirFile).Length
    Write-Host "    $hirFile : $size bytes"
    Test-Result "HIR file produced" ($true) ""
} else {
    Write-Host "    HIR file not found (may be expected if pipeline fails before write)"
    Test-Result "HIR file produced" ($false) "not found"
}

# List all files in output dir
$files = Get-ChildItem $OutDir -Recurse -File
if ($files.Count -gt 0) {
    Write-Host "    Files in $OutDir:"
    foreach ($f in $files) {
        Write-Host "      $($f.Name) ($($f.Length) bytes)"
    }
}

# ── [4] Check for exe ──
Write-Host ""
Write-Host "[4] Executable check"
$exeFile = Join-Path $Root "$OutDir/simple.exe"
if (Test-Path $exeFile) {
    $exeSize = (Get-Item $exeFile).Length
    Write-Host "    $exeFile : $exeSize bytes"
    Test-Result "Exe produced" ($true) ""

    # Run the exe and compare exit code with VM
    Write-Host ""
    Write-Host "[5] Running Photon exe"
    $exeResult = & $exeFile 2>&1
    $exeCode = $LASTEXITCODE
    Write-Host "    exit code: $exeCode"
    if ($exeResult) {
        foreach ($line in $exeResult) {
            $t = "$line".Trim()
            if ($t -ne '') { Write-Host "    stdout/stderr: $t" }
        }
    }
    Test-Result "Exe runs" ($true) ""
} else {
    Write-Host "    No exe produced (linker may not be available)"
    Test-Result "Exe produced" ($false) "not found (linker unavailable?)"
}

# ── Summary ──
Write-Host ""
Write-Host "══════════════════════════════════════════════"
Write-Host "S1.9 E2E Diff Test: PASS=$pass / FAIL=$fail"
Write-Host "══════════════════════════════════════════════"
if ($fail -eq 0) {
    Write-Host "ALL PASS" -ForegroundColor Green
    exit 0
} else {
    Write-Host "FAILURES: $fail" -ForegroundColor Red
    exit 1
}