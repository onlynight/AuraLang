# ir-diff.ps1 - Differential check between the two AOT emitters.
#
# Background
# ----------
# This repo has TWO LLVM-IR emitters that must agree semantically:
#   * Rust emitter  : `aura.exe build <src> --aot --emit-llvm` (compiler/src/codegen/aot)
#   * Aura emitter  : the Aura-written compiler (aura/compiler/aura/lang/compiler/aot)
#
# Bootstrapping uses both: the Rust emitter produces the first carrier, the carrier
# produces every later stage with the Aura emitter.  A silent miscompilation in the
# Aura emitter therefore only shows up as "the self-hosted compiler misbehaves on
# input X" - typically as an access violation with no diagnostics, which is very
# hard to trace back to a single function.
#
# This script shortens that hunt: it emits the IR for the SAME input with both
# emitters, normalizes the value/label names (they are numbered independently) and
# reports every function whose instruction stream differs.  Pass -Function <name>
# to print the normalized side-by-side bodies of one function.
#
# Usage
# -----
#   powershell -File scripts/ir-diff.ps1
#   powershell -File scripts/ir-diff.ps1 -Source examples/language-test/05-classes.aura
#   powershell -File scripts/ir-diff.ps1 -Function Parser_parsePrimary
#
# NOTE: the two emitters are independent implementations, so a non-empty diff is
# expected (constant materialization style, `inbounds`, elided `toStr` on i8* ...).
# Treat the report as a *candidate list* to triage, not as a verdict.

param(
    # NOTE: named `Source`, not `Input` - `$input` is a PowerShell automatic
    # variable (pipeline enumerator) and would shadow the parameter.
    [string]$Source = "D:\Code\AuraLang\aura\compiler\aura\lang\compiler\Main.aura",
    [string]$Cli = "D:\Code\AuraLang\target\release\aura.exe",
    # Binary that hosts the *Aura* emitter.  Defaults to the self-hosted
    # compiler (which is exactly an Aura-emitter build).  Pass -BuildCarrier to
    # produce a fresh one from $Cli.
    [string]$Carrier = "D:\Code\AuraLang\build\aura-compiler-selfhost.exe",
    [switch]$BuildCarrier,
    [string]$OutDir = "D:\Code\AuraLang\build\ir-diff",
    [string]$Function = "",
    [int]$Top = 40
)

$ErrorActionPreference = "Continue"
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

# ── helpers ────────────────────────────────────────────────────────────────

# Split a .ll file into `<function name> -> body lines`.
function Get-LlFunctions([string]$Path) {
    $map = @{}
    $name = $null
    $buf = $null
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        if ($line -match '^define .*@([A-Za-z0-9_.]+)\(') {
            $name = $matches[1]
            $buf = New-Object System.Collections.ArrayList
            continue
        }
        if ($null -ne $name) {
            if ($line -eq '}') {
                $map[$name] = $buf.ToArray()
                $name = $null
                continue
            }
            $buf.Add($line) | Out-Null
        }
    }
    return $map
}

# Erase the parts that legitimately differ between the two emitters:
#   * `inbounds` (Aura emitter always emits it)
#   * SSA value names / global names / block labels (numbered independently)
function Get-NormBody($lines) {
    $out = New-Object System.Collections.ArrayList
    foreach ($l in $lines) {
        $x = $l -replace 'getelementptr inbounds', 'getelementptr'
        $x = $x -replace '%[A-Za-z0-9_.]+', '%V'
        $x = $x -replace '@[A-Za-z0-9_.]+', '@G'
        $x = $x -replace '^\s*([A-Za-z_][A-Za-z0-9_.]*):\s*$', 'L:'
        $out.Add($x.Trim()) | Out-Null
    }
    # fold runs of blank lines (alloca blocks are laid out differently)
    $folded = New-Object System.Collections.ArrayList
    $prevBlank = $false
    foreach ($l in $out) {
        if ($l -eq '') {
            if (-not $prevBlank) { $folded.Add('') | Out-Null }
            $prevBlank = $true
        } else {
            $folded.Add($l) | Out-Null
            $prevBlank = $false
        }
    }
    return ($folded -join "`n")
}

# ── 1. Rust emitter IR ─────────────────────────────────────────────────────

$rustLl = Join-Path $OutDir "rust-emitter.ll"
Write-Host "── [1/3] Rust emitter ─────────────────────────────────────" -ForegroundColor Cyan
Write-Host "  source: $Source"
# `--emit-llvm` writes the IR text even when the following llc stage rejects it,
# so a non-zero exit is only a warning here (the Rust emitter has its own bugs).
& $Cli build $Source --aot --emit-llvm --output $rustLl 2>$null | Out-Null
if (-not (Test-Path $rustLl)) { throw "Rust emitter failed: no IR at $rustLl" }
Write-Host "  ir    : $rustLl" -ForegroundColor Green

# ── 2. Aura emitter IR (carrier compiles the same input) ───────────────────

$auraExe = Join-Path $OutDir "aura-emitted.exe"
$auraLl = Join-Path $OutDir "aura-emitted.ll"

Write-Host "── [2/3] Aura emitter ─────────────────────────────────────" -ForegroundColor Cyan
if ($BuildCarrier) {
    $carrier = Join-Path $OutDir "carrier.exe"
    & $Cli build $Source --aot --output $carrier | Out-Null
    if (-not (Test-Path $carrier)) { throw "carrier build failed: $carrier" }
} else {
    $carrier = $Carrier
    if (-not (Test-Path $carrier)) { throw "carrier not found: $carrier" }
}
Write-Host "  carrier: $carrier" -ForegroundColor Green

& $carrier $Source -o $auraExe 2>$null | Out-Null
if (-not (Test-Path $auraLl)) {
    Write-Host "  !! Aura emitter produced no IR (compile crashed)" -ForegroundColor Red
    Write-Host "     -> the miscompilation is severe enough to abort the compiler" -ForegroundColor Red
    throw "no Aura-emitter IR"
}
Write-Host "  ir    : $auraLl" -ForegroundColor Green

# ── 3. per-function comparison ─────────────────────────────────────────────

Write-Host "── [3/3] function-by-function diff ────────────────────────" -ForegroundColor Cyan
$rf = Get-LlFunctions $rustLl
$af = Get-LlFunctions $auraLl
Write-Host ("  functions: rust={0} aura={1}" -f $rf.Count, $af.Count)

# 两侧函数名不一定一一对应（Aura 发射器会给伴生对象成员/属性访问器换名：
# `MathUtil_add` ↔ `add`、`Person___ctor1` ↔ `Person_init2`），因此「只在 Rust
# 侧存在」的条目再做一次**按末段方法名**的模糊匹配：命中记为 `renamed:<aura 名>`，
# 否则才是真正的 `rust-only`。把两者混在一起会淹没有效信号。
$rustOnly = New-Object System.Collections.ArrayList
$auraOnly = New-Object System.Collections.ArrayList
foreach ($k in ($rf.Keys | Sort-Object)) { if (-not $af.ContainsKey($k)) { $rustOnly.Add($k) | Out-Null } }
foreach ($k in ($af.Keys | Sort-Object)) { if (-not $rf.ContainsKey($k)) { $auraOnly.Add($k) | Out-Null } }

$rows = New-Object System.Collections.ArrayList
foreach ($k in ($rf.Keys | Sort-Object)) {
    if (-not $af.ContainsKey($k)) {
        $s = ($k -split '_')[-1]
        # 构造函数：Rust 侧 `C___ctorN` ↔ Aura 侧 `C_initN`
        if ($s -match '^ctor') { $s = 'init' }
        $hit = @($auraOnly | Where-Object { ($_ -split '_')[-1] -eq $s })
        $note = 'rust-only'
        if ($hit.Count -gt 0) { $note = 'renamed:' + ($hit -join '/') }
        $rows.Add([PSCustomObject]@{ Name = $k; Rust = $rf[$k].Count; Aura = 0; Delta = $note }) | Out-Null
        continue
    }
    $nr = Get-NormBody $rf[$k]
    $na = Get-NormBody $af[$k]
    if ($nr -ne $na) {
        $rows.Add([PSCustomObject]@{
            Name  = $k
            Rust  = $rf[$k].Count
            Aura  = $af[$k].Count
            Delta = ([math]::Abs($rf[$k].Count - $af[$k].Count))
        }) | Out-Null
    }
}

Write-Host ("  structurally differing: {0} / {1}" -f $rows.Count, $rf.Count) -ForegroundColor Yellow
Write-Host ("  rust-only={0} aura-only={1} (按末段模糊匹配消歧)" -f $rustOnly.Count, $auraOnly.Count)
$rows | Sort-Object -Property Delta -Descending | Select-Object -First $Top | Format-Table -AutoSize

if ($auraOnly.Count -gt 0) {
    Write-Host "  Aura-only functions (first 10):" -ForegroundColor DarkGray
    $auraOnly | Select-Object -First 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
}

# ── optional: side-by-side body of one function ────────────────────────────

if ($Function -ne "") {
    Write-Host "── $Function ─────────────────────────────────────────────" -ForegroundColor Cyan
    if (-not $rf.ContainsKey($Function)) { throw "no such function in rust IR: $Function" }
    $nr = (Get-NormBody $rf[$Function]) -split "`n"
    $na = @()
    if ($af.ContainsKey($Function)) { $na = (Get-NormBody $af[$Function]) -split "`n" }
    $max = [math]::Max($nr.Count, $na.Count)
    for ($i = 0; $i -lt $max; $i++) {
        $l = if ($i -lt $nr.Count) { $nr[$i] } else { '' }
        $r = if ($i -lt $na.Count) { $na[$i] } else { '' }
        $mark = if ($l -eq $r) { ' ' } else { '!' }
        Write-Host ("{0} {1,-58} | {2}" -f $mark, $l, $r)
    }
}

Write-Host ""
Write-Host "Done. Raw IR kept in $OutDir" -ForegroundColor Green
