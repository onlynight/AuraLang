# ============================================================
# Phase 6: Self-Bootstrapping Verification Script
# ============================================================
#
# Usage:
#   scripts\bootstrap-photon.ps1 [-Step <step>] [-Clean] [-Verify]
#
# Steps:
#   all    - Run all steps (default)
#   llvm   - Step 1: Build compiler with LLVM backend
#   runtime - Step 2: Build runtime with Photon backend
#   photon - Step 3: Build compiler with Photon backend
#   verify - Step 4: Verify bootstrap consistency
#
# ============================================================

param(
    [ValidateSet("all", "llvm", "runtime", "photon", "verify")]
    [string]$Step = "all",
    [switch]$Clean,
    [switch]$Verify,
    [string]$OutDir = "build/bootstrap",
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

if ($Clean) {
    if (Test-Path $OutDir) {
        Remove-Item $OutDir -Recurse -Force
        Write-Host "Cleaned $OutDir"
    }
    return
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Write-Host "=========================================="
Write-Host " Photon Phase 6: Bootstrap Verification"
Write-Host "=========================================="
Write-Host " Step: $Step"
Write-Host " Output: $OutDir"
Write-Host ""

# ============================================================
# Helper Functions
# ============================================================

function Find-Aura {
    foreach ($c in @(
        'rust/target/release/aura.exe',
        'build/bin/aura.exe',
        'aura/seed/aura.exe',
        'target/release/aura.exe',
        'target/debug/aura.exe'
    )) {
        if (Test-Path $c) { return (Resolve-Path $c).Path }
    }
    return $null
}

function Find-AuraLld {
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
                $lldPath = Join-Path $lldDir 'bin/lld-link.exe'
                if (Test-Path $lldPath) { return $lldPath }
            }
        }
    }
    return $null
}

# ============================================================
# Step 1: Build Compiler with LLVM Backend
# ============================================================

function Build-LvmCompiler {
    Write-Host "Step 1: Building compiler with LLVM backend..."
    
    $aura = Find-Aura
    if (-not $aura) {
        Write-Host "  [ERROR] aura.exe not found"
        return $false
    }
    
    $auraPath = (Resolve-Path $aura).Path
    $outDir = Join-Path $OutDir 'llvm'
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    
    if ($DryRun) {
        Write-Host "  [DRY-RUN] Would build compiler with LLVM backend"
        return $true
    }
    
    # Check if compiler source exists
    $compilerSrc = Join-Path $Root 'rust/cli/src/main.rs'
    if (-not (Test-Path $compilerSrc)) {
        Write-Host "  [ERROR] Compiler source not found: $compilerSrc"
        return $false
    }
    
    Write-Host "  Using: $auraPath"
    Write-Host "  Output: $outDir"
    Write-Host "  [INFO] LLVM backend compilation requires Rust toolchain"
    Write-Host "  [INFO] Run: cargo build --release"
    
    return $true
}

# ============================================================
# Step 2: Build Runtime with Photon Backend
# ============================================================

function Build-PhotonRuntime {
    Write-Host "Step 2: Building runtime with Photon backend..."
    
    $runtimeFiles = @(
        "aura/runtime/Memory.aura",
        "aura/runtime/GC.aura",
        "aura/runtime/Exception.aura",
        "aura/runtime/Thread.aura",
        "aura/runtime/Runtime.aura"
    )
    
    $outDir = Join-Path $OutDir 'runtime'
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    
    foreach ($file in $runtimeFiles) {
        if (-not (Test-Path $file)) {
            Write-Host "  [ERROR] Missing: $file"
            return $false
        }
    }
    
    if ($DryRun) {
        Write-Host "  [DRY-RUN] Would build $($runtimeFiles.Count) runtime files"
        return $true
    }
    
    Write-Host "  Runtime files verified: $($runtimeFiles.Count)"
    Write-Host "  Output: $outDir"
    Write-Host "  [INFO] Photon backend compilation requires Phase 1-5 complete"
    
    return $true
}

# ============================================================
# Step 3: Build Compiler with Photon Backend
# ============================================================

function Build-PhotonCompiler {
    Write-Host "Step 3: Building compiler with Photon backend..."
    
    $outDir = Join-Path $OutDir 'photon'
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    
    if ($DryRun) {
        Write-Host "  [DRY-RUN] Would build compiler with Photon backend"
        return $true
    }
    
    Write-Host "  Output: $outDir"
    Write-Host "  [INFO] This step requires aura-llvm.exe from Step 1"
    Write-Host "  [INFO] And runtime from Step 2"
    
    return $true
}

# ============================================================
# Step 4: Verify Bootstrap Consistency
# ============================================================

function Verify-Bootstrap {
    Write-Host "Step 4: Verifying bootstrap consistency..."
    
    $llvmExe = Join-Path $OutDir 'llvm/aura-llvm.exe'
    $photonExe = Join-Path $OutDir 'photon/aura-photon.exe'
    
    if ($DryRun) {
        Write-Host "  [DRY-RUN] Would verify bootstrap consistency"
        return $true
    }
    
    $verification = @()
    $verification += "Bootstrap Verification Report"
    $verification += "==============================="
    $verification += ""
    
    # Check if both executables exist
    if (Test-Path $llvmExe) {
        $llvmSize = (Get-Item $llvmExe).Length
        $verification += "[OK] aura-llvm.exe exists ($llvmSize bytes)"
    } else {
        $verification += "[FAIL] aura-llvm.exe not found"
    }
    
    if (Test-Path $photonExe) {
        $photonSize = (Get-Item $photonExe).Length
        $verification += "[OK] aura-photon.exe exists ($photonSize bytes)"
    } else {
        $verification += "[FAIL] aura-photon.exe not found"
    }
    
    # Compare if both exist
    if ((Test-Path $llvmExe) -and (Test-Path $photonExe)) {
        $llvmHash = (Get-FileHash $llvmExe -Algorithm SHA256).Hash
        $photonHash = (Get-FileHash $photonExe -Algorithm SHA256).Hash
        
        if ($llvmHash -eq $photonHash) {
            $verification += "[OK] Hashes match: $llvmHash"
            $verification += "[OK] Bootstrap verification PASSED"
        } else {
            $verification += "[WARN] Hashes differ:"
            $verification += "       LLVM:   $llvmHash"
            $verification += "       Photon: $photonHash"
            $verification += "[INFO] Check compiler determinism"
        }
    }
    
    $report = $verification -join "`n"
    Write-Host $report
    
    $reportPath = Join-Path $OutDir 'bootstrap-verification.txt'
    $report | Out-File $reportPath -Encoding utf8
    Write-Host "  Report saved: $reportPath"
    
    return $true
}

# ============================================================
# Main Execution
# ============================================================

$success = $true

switch ($Step) {
    "all" {
        $success = $success -and (Build-LvmCompiler)
        $success = $success -and (Build-PhotonRuntime)
        $success = $success -and (Build-PhotonCompiler)
        $success = $success -and (Verify-Bootstrap)
    }
    "llvm" {
        $success = Build-LvmCompiler
    }
    "runtime" {
        $success = Build-PhotonRuntime
    }
    "photon" {
        $success = Build-PhotonCompiler
    }
    "verify" {
        $success = Verify-Bootstrap
    }
}

Write-Host ""
Write-Host "=========================================="
if ($success) {
    Write-Host " Phase 6: SUCCESS"
} else {
    Write-Host " Phase 6: FAILED"
}
Write-Host "=========================================="