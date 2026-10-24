# -------------------------------------------------------------
# Dual-VM isolation guard (VM-PA-00 v3.2, P4.5 / ADR-005)
# Enforces decision D6: Rust VM and Aura VM never reference each other.
# ASCII-only on purpose (Windows PowerShell 5.1 code page safety).
# Usage: powershell -File scripts\check-dual-impl-isolation.ps1
# Mirrors scripts/check-dual-impl-isolation.sh
# -------------------------------------------------------------

$ErrorActionPreference = 'Continue'
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $RootDir

$violations = 0

function Is-Comment([string]$line) {
    return $line -match '^\s*(//|#)'
}

# --- Constraint 1: aura/compiler/**/*.aura, no Rust VM symbols (non-comment) ---
$auraFiles = Get-ChildItem -Path 'aura/compiler' -Recurse -Filter '*.aura' -File
foreach ($f in $auraFiles) {
    $n = 0
    foreach ($line in Get-Content $f.FullName) {
        $n++
        if ($line -match 'compiler::vm|interp::|Value::Int|jit_compile_cranelift|VmOptions|seed/compiler/src/vm') {
            if (-not (Is-Comment $line)) {
                Write-Host ("VIOLATION [constraint 1: aura->rust-vm] {0}:{1}: {2}" -f $f.FullName, $n, $line) -ForegroundColor Red
                $violations++
            }
        }
    }
}

# --- Constraint 2: seed/compiler/src/vm/*.rs, no Aura VM symbols (non-comment) ---
$vmFiles = Get-ChildItem -Path 'seed/compiler/src/vm' -Filter '*.rs' -File
foreach ($f in $vmFiles) {
    $n = 0
    foreach ($line in Get-Content $f.FullName) {
        $n++
        if ($line -match 'AucLoader|VmRunner|Vm\.aura|aura/lang/compiler/vm|aura/compiler/') {
            if (-not (Is-Comment $line)) {
                Write-Host ("VIOLATION [constraint 2: rust-vm->aura] {0}:{1}: {2}" -f $f.FullName, $n, $line) -ForegroundColor Red
                $violations++
            }
        }
    }
}

# --- Constraint 3: freeze-baseline marker present in every Rust VM file (P4.3) ---
foreach ($f in $vmFiles) {
    $found = Select-String -Path $f.FullName -Pattern 'FROZEN BASELINE' -Quiet
    if (-not $found) {
        Write-Host ("VIOLATION [constraint 3: freeze marker] {0}: missing D6 freeze-baseline marker" -f $f.FullName) -ForegroundColor Red
        $violations++
    }
}

# --- Constraint 4: coroutine.rs carries the D5 legacy marker ---
$coroutineRs = 'seed/compiler/src/vm/coroutine.rs'
if (Test-Path $coroutineRs) {
    $legacy = Select-String -Path $coroutineRs -Pattern 'LEGACY per D5' -Quiet
    if (-not $legacy) {
        Write-Host ("VIOLATION [constraint 4] {0}: D5 legacy marker missing" -f $coroutineRs) -ForegroundColor Red
        $violations++
    }
} else {
    Write-Host ("VIOLATION [constraint 4] {0} not found" -f $coroutineRs) -ForegroundColor Red
    $violations++
}

if ($violations -gt 0) {
    Write-Host ("=== dual-impl isolation guard: FAIL ({0} violation(s)) ===" -f $violations) -ForegroundColor Cyan
    exit 1
}
Write-Host '=== dual-impl isolation guard: PASS ===' -ForegroundColor Cyan
exit 0
