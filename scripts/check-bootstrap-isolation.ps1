# -------------------------------------------------------------
# Bootstrap isolation guard (VM-PA-00 v3.1, P0.1 / ADR-004)
#
# Enforces decision D1: seed/compiler/src/bootstrap/ is KEPT but
# MUST NOT be referenced from any production path.
#
# Constraints (any violation exits 1):
#   1. aura/compiler/**/*.aura      : no non-comment reference to "bootstrap"
#   2. seed/compiler/src/vm/        : zero references to "bootstrap"
#   3. seed/compiler/src/ (outside bootstrap/): only comment hits,
#      plus the single mount point `pub mod bootstrap;` in lib.rs
#   4. tests/ and bootstrap/ itself : unrestricted
#
# NOTE: ASCII-only on purpose, so it runs correctly under Windows
#       PowerShell 5.1 regardless of the active code page.
#
# Usage: powershell -File scripts\check-bootstrap-isolation.ps1
# -------------------------------------------------------------

$ErrorActionPreference = 'Stop'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$violations = 0

function Test-IsCommentLine([string]$line) {
    return $line -match '^\s*(//|#)'
}

# --- Constraint 1: aura/compiler/*.aura must not reference bootstrap ---
$auraFiles = Get-ChildItem aura\compiler -Recurse -Filter '*.aura' -File
foreach ($f in $auraFiles) {
    $lineNo = 0
    foreach ($line in Get-Content $f.FullName) {
        $lineNo++
        if ($line -match 'bootstrap') {
            if (-not (Test-IsCommentLine $line)) {
                Write-Host ("VIOLATION [c1] {0}:{1}: {2}" -f $f.FullName, $lineNo, $line.Trim()) -ForegroundColor Red
                $violations++
            }
        }
    }
}

# --- Constraint 2: seed/compiler/src/vm/ must have zero references ---
$vmFiles = Get-ChildItem seed\compiler\src\vm -Recurse -Filter '*.rs' -File
foreach ($f in $vmFiles) {
    $lineNo = 0
    foreach ($line in Get-Content $f.FullName) {
        $lineNo++
        if ($line -match 'bootstrap') {
            Write-Host ("VIOLATION [c2] {0}:{1}: {2}" -f $f.FullName, $lineNo, $line.Trim()) -ForegroundColor Red
            $violations++
        }
    }
}

# --- Constraint 3: rest of seed/compiler/src allows comments + lib.rs mount only ---
$srcFiles = Get-ChildItem seed\compiler\src -Recurse -Filter '*.rs' -File |
    Where-Object { $_.FullName -notmatch '\\bootstrap\\' }
foreach ($f in $srcFiles) {
    $lineNo = 0
    $isLibRs = $false
    if ($f.Name -eq 'lib.rs') { $isLibRs = $true }
    foreach ($line in Get-Content $f.FullName) {
        $lineNo++
        $hit = $line -match 'bootstrap'
        if (-not $hit) { continue }
        if (Test-IsCommentLine $line) { continue }
        if ($isLibRs) {
            $trimmed = $line.Trim()
            if ($trimmed -eq 'pub mod bootstrap;') { continue }
        }
        Write-Host ("VIOLATION [c3] {0}:{1}: {2}" -f $f.FullName, $lineNo, $line.Trim()) -ForegroundColor Red
        $violations++
    }
}

# --- Constraint 4: isolation marker in bootstrap/mod.rs must exist ---
$modRs = 'seed\compiler\src\bootstrap\mod.rs'
if (Test-Path $modRs) {
    $head = (Get-Content $modRs -TotalCount 20) -join "`n"
    if ($head -notmatch 'ISOLATED.*DO NOT REFERENCE') {
        Write-Host ("VIOLATION [c4] {0}: isolation marker missing (see ADR-004)" -f $modRs) -ForegroundColor Red
        $violations++
    }
} else {
    Write-Host ("VIOLATION [c4] {0} not found" -f $modRs) -ForegroundColor Red
    $violations++
}

if ($violations -gt 0) {
    Write-Host "=== bootstrap isolation guard: FAIL ($violations violation(s)) ===" -ForegroundColor Red
    exit 1
}
Write-Host "=== bootstrap isolation guard: PASS ===" -ForegroundColor Green
exit 0
