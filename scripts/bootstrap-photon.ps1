# ============================================================
# bootstrap-photon.ps1 - Photon backend bootstrap verification (P1)
# ============================================================
#
# Bootstrap chain: Rust(seed) -> AOT(LLVM) -> Photon -> self-verify
#
#   Step 1  AOT backend compile     - verify LLVM AOT path, emit reference exe
#   Step 2  Photon compile runtime  - aura/*.aura -> COFF .obj
#   Step 3  Photon compile compiler - entry .aura -> .exe
#   Step 4  Self-verify consistency - rebuild twice, byte compare
#   Step 5  COFF determinism        - timestamp/random-field check
#
# Usage:
#   scripts\bootstrap-photon.ps1
#   scripts\bootstrap-photon.ps1 -Step 1
#   scripts\bootstrap-photon.ps1 -Step 1,2,5
#   scripts\bootstrap-photon.ps1 -Clean
#   scripts\bootstrap-photon.ps1 -DryRun
# ============================================================

param(
    [ValidateSet("1","2","3","4","5")]
    [string[]]$Step,
    [switch]$Clean,
    [switch]$DryRun,
    [string]$OutDir = "build\bootstrap",
    [int]$TimeoutSecs = 600
)

$ErrorActionPreference = 'Continue'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

# ---- toolchain ----
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
$LLVM_HOME = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc"
$env:AURA_LLVM_HOME = $LLVM_HOME

$AuraBin = $null
foreach ($c in @('rust\target\release\aura.exe','aura\seed\aura.exe','rust\target\debug\aura.exe','build\bin\aura.exe')) {
    if (Test-Path $c) { $AuraBin = (Resolve-Path $c).Path; break }
}
$LldLink = Join-Path $LLVM_HOME "bin\lld-link.exe"

function Invoke-Proc($exe, $argStr, $wd, $to) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = $argStr
    $psi.WorkingDirectory = $wd
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $so = $p.StandardOutput.ReadToEnd()
    $se = $p.StandardError.ReadToEnd()
    if ($p.WaitForExit($to * 1000)) { return @{ Out=$so; Err=$se; Code=$p.ExitCode; TimedOut=$false } }
    try { $p.Kill() } catch {}
    return @{ Out=$so; Err=$se; Code=-1; TimedOut=$true }
}

# ---- reporting ----
$report = [System.Collections.Generic.List[string]]::new()
$pass = 0; $fail = 0; $skip = 0
function Log($s)    { Write-Host $s; $report.Add($s) }
function StepLog($s) { Write-Host "    $s" }
function Pass($name)  { $script:pass++; Write-Host "  PASS: $name" -ForegroundColor Green; $report.Add("  PASS: $name") }
function Fail($name, $detail) { $script:fail++; Write-Host "  FAIL: $name - $detail" -ForegroundColor Red; $report.Add("  FAIL: $name - $detail") }
function Skip($name, $why) { $script:skip++; Write-Host "  SKIP: $name - $why" -ForegroundColor Yellow; $report.Add("  SKIP: $name - $why") }
function ShaOf($p)  { if (Test-Path $p) { (Get-FileHash $p -Algorithm SHA256).Hash } else { $null } }
function FirstErr($txt) { ($txt -split "`n" | Select-String -Pattern "error|failed|undefined|Error" | Select-Object -First 2) -join " | " }

# ---- clean ----
if ($Clean) {
    if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force; Write-Host "Cleaned $OutDir" }
    return
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Log ""
Log "============================================================"
Log " Photon backend bootstrap verification (P1)"
Log "============================================================"
Log "  compiler : $($AuraBin)"
Log "  lld-link : $(if (Test-Path $LldLink) { $LldLink } else { 'NOT FOUND' })"
Log "  out dir  : $OutDir"
Log ""

$steps = if ($Step.Count -gt 0) { $Step } else { @("1","2","3","4","5") }

# ---- prerequisites ----
if (-not $AuraBin) {
    Fail "prereq" "aura.exe not found"
} elseif (-not (Test-Path $LldLink)) {
    Fail "prereq" "lld-link.exe not found at $LldLink"
} else {
    Pass "prereq (compiler + linker)"
}

# ============================================================
# Step 1: AOT backend compile (Rust LLVM AOT)
# ============================================================
if ($steps -contains "1") {
    Log ""
    Log "-------- Step 1: AOT backend compile (Rust LLVM AOT) --------"
    $s1 = Join-Path $OutDir "step1"; New-Item -ItemType Directory -Force -Path $s1 | Out-Null

    if ($DryRun) {
        Skip "Step 1" "dry-run"
    } else {
        $probe = Invoke-Proc $AuraBin "build --aot `"$Root\tests\photon\simple.aura`" --output `"$s1\probe.exe`"" $Root $TimeoutSecs
        if ($probe.TimedOut) {
            Fail "Step 1a AOT" "timeout"
        } elseif (Test-Path "$s1\probe.exe") {
            $run = Invoke-Proc "$s1\probe.exe" "" $s1 $TimeoutSecs
            if ($run.Code -eq 42) { Pass "Step 1a AOT (simple.exe exit=42)" }
            else { Fail "Step 1a AOT" "exit=$($run.Code) (expect 42)" }
        } else {
            Fail "Step 1a AOT" "no exe produced. $(FirstErr $probe.Err)"
        }

        $s1b = Join-Path $s1 "hw.exe"
        $probe2 = Invoke-Proc $AuraBin "build --aot `"$Root\tests\photon\P1\01_hello_world.aura`" --output `"$s1b`"" $Root $TimeoutSecs
        if (Test-Path $s1b) {
            $run2 = Invoke-Proc $s1b "" $s1 $TimeoutSecs
            $o = ($run2.Out -replace "`r","").Trim()
            if ($o -eq "Hello, World!") { Pass "Step 1b AOT stdlib (hello world)" }
            else { Fail "Step 1b AOT stdlib" "stdout='$o' (expect 'Hello, World!')" }
        } else {
            Fail "Step 1b AOT stdlib" "no exe. $(FirstErr $probe2.Err)"
        }
    }
}

# ============================================================
# Step 2: Photon compile runtime (.aura -> .obj)
# ============================================================
if ($steps -contains "2") {
    Log ""
    Log "-------- Step 2: Photon compile runtime --------"
    $s2 = Join-Path $OutDir "step2"; New-Item -ItemType Directory -Force -Path $s2 | Out-Null

    # runtime sources live under aura/core/aura/lang/ (aura/runtime/ was merged in)
    $runtimeFiles = @(
        "aura\core\aura\lang\native\Memory.aura",
        "aura\core\aura\lang\native\GC.aura",
        "aura\core\aura\lang\errors\Exception.aura",
        "aura\core\aura\lang\concurrent\Thread.aura",
        "aura\core\aura\lang\native\Runtime.aura"
    )
    $missing = @(); foreach ($f in $runtimeFiles) { if (-not (Test-Path $f)) { $missing += $f } }
    if ($missing.Count -gt 0) {
        Fail "Step 2 sources" ("missing: " + ($missing -join ", "))
    } else {
        Pass "Step 2 sources present ($($runtimeFiles.Count) files)"
    }

    if ($DryRun) {
        Skip "Step 2 compile" "dry-run"
    } else {
        foreach ($f in $runtimeFiles) {
            if (-not (Test-Path $f)) { continue }
            $stem = [IO.Path]::GetFileNameWithoutExtension($f)
            $phir = Join-Path $s2 "$stem.phir"
            $r = Invoke-Proc $AuraBin "build -b photon `"$Root\$f`" --output `"$phir`"" $Root $TimeoutSecs
            if ($r.TimedOut) { Fail "Step 2 $stem" "timeout"; continue }
            $obj = Join-Path $s2 "$stem.obj"
            if (Test-Path $obj) { Pass "Step 2 $stem -> .obj ($((Get-Item $obj).Length) B)" }
            else { Fail "Step 2 $stem" ("no .obj. " + (FirstErr $r.Err)) }
        }
    }
}

# ============================================================
# Step 3: Photon compile compiler entry (.aura -> .exe)
# ============================================================
if ($steps -contains "3") {
    Log ""
    Log "-------- Step 3: Photon compile compiler entry --------"
    $s3 = Join-Path $OutDir "step3"; New-Item -ItemType Directory -Force -Path $s3 | Out-Null

    $entry = "aura\compiler\aura\lang\compiler\Main.aura"
    if (-not (Test-Path $entry)) {
        Fail "Step 3 entry" "Main.aura not found"
    } elseif ($DryRun) {
        Skip "Step 3" "dry-run"
    } else {
        $phir = Join-Path $s3 "compiler.phir"
        $r = Invoke-Proc $AuraBin "build -b photon `"$Root\$entry`" --output `"$phir`"" $Root $TimeoutSecs
        if ($r.TimedOut) {
            Fail "Step 3 compile" "timeout"
        } elseif (Test-Path "$s3\Main.exe") {
            Pass "Step 3 compile -> Main.exe ($((Get-Item "$s3\Main.exe").Length) B)"
        } else {
            $err = FirstErr $r.Err
            StepLog "  reason: $err"
            Fail "Step 3 compile" "no Main.exe (multi-file compiler exceeds single-file pipeline)"
        }
    }
}

# ============================================================
# Step 4: self-verify consistency
# ============================================================
if ($steps -contains "4") {
    Log ""
    Log "-------- Step 4: self-verify consistency --------"
    $s4 = Join-Path $OutDir "step4"
    if ($DryRun) {
        Skip "Step 4" "dry-run"
    } else {
        New-Item -ItemType Directory -Force -Path $s4 | Out-Null
        # multi-function program: exercises symbol emission + relocations
        $src = "tests\photon\P1\02_simple_vars.aura"
        # the photon backend names the object file after the SOURCE stem
        # (e.g. 02_simple_vars.obj), and writes it next to the .phir output.
        # Two builds therefore need two distinct subdirectories, otherwise the
        # second build silently overwrites the first.
        $stem = [System.IO.Path]::GetFileNameWithoutExtension($src)
        $da = Join-Path $s4 "a"; New-Item -ItemType Directory -Force -Path $da | Out-Null
        $db = Join-Path $s4 "b"; New-Item -ItemType Directory -Force -Path $db | Out-Null
        $r1 = Invoke-Proc $AuraBin "build -b photon `"$Root\$src`" --output `"$da\a.phir`"" $Root $TimeoutSecs
        $r2 = Invoke-Proc $AuraBin "build -b photon `"$Root\$src`" --output `"$db\b.phir`"" $Root $TimeoutSecs
        $ha = ShaOf "$da\a.phir"; $hb = ShaOf "$db\b.phir"
        if ($ha -and $hb) {
            if ($ha -eq $hb) { Pass "Step 4a PHIR byte-identical" }
            else { Fail "Step 4a PHIR" "hashes differ" }
        } else { Fail "Step 4a PHIR" "phir not produced" }

        $ho1 = ShaOf "$da\$stem.obj"; $ho2 = ShaOf "$db\$stem.obj"
        if ($ho1 -and $ho2) {
            if ($ho1 -eq $ho2) { Pass "Step 4b OBJ byte-identical" }
            else { Fail "Step 4b OBJ" "hashes differ" }
        } else { Fail "Step 4b OBJ" "obj not produced ($stem.obj)" }

        $bootExe = Join-Path $OutDir "step3\Main.exe"
        if (Test-Path $bootExe) {
            $ds = Join-Path $s4 "self"; New-Item -ItemType Directory -Force -Path $ds | Out-Null
            Invoke-Proc $bootExe "build -b photon `"$Root\$src`" --output `"$ds\self.phir`"" $Root $TimeoutSecs | Out-Null
            $hs = ShaOf "$ds\self.phir"
            if ($hs -and $ha) {
                if ($hs -eq $ha) { Pass "Step 4c self-bootstrap byte-identical" }
                else { Fail "Step 4c self-bootstrap" "seed and self outputs differ" }
            } else { Fail "Step 4c self-bootstrap" "self.phir not produced by boot exe" }
        } else {
            Skip "Step 4c self-bootstrap" "requires Step 3 compiler exe"
        }
    }
}

# ============================================================
# Step 5: COFF determinism
# ============================================================
if ($steps -contains "5") {
    Log ""
    Log "-------- Step 5: COFF determinism --------"
    $s5 = Join-Path $OutDir "step5"
    if ($DryRun) {
        Skip "Step 5" "dry-run"
    } else {
        New-Item -ItemType Directory -Force -Path $s5 | Out-Null
        $src = "tests\photon\P1\02_simple_vars.aura"
        $stem = [System.IO.Path]::GetFileNameWithoutExtension($src)
        # separate dirs: the backend writes <stem>.obj next to the .phir output
        $da = Join-Path $s5 "a"; New-Item -ItemType Directory -Force -Path $da | Out-Null
        $db = Join-Path $s5 "b"; New-Item -ItemType Directory -Force -Path $db | Out-Null
        Invoke-Proc $AuraBin "build -b photon `"$Root\$src`" --output `"$da\a.phir`"" $Root $TimeoutSecs | Out-Null
        Start-Sleep -Seconds 2
        Invoke-Proc $AuraBin "build -b photon `"$Root\$src`" --output `"$db\b.phir`"" $Root $TimeoutSecs | Out-Null

        $ha = ShaOf "$da\$stem.obj"; $hb = ShaOf "$db\$stem.obj"
        if (-not $ha -or -not $hb) {
            Fail "Step 5 COFF" "obj missing ($stem.obj)"
        } elseif ($ha -ne $hb) {
            Fail "Step 5 COFF" "obj hashes differ across runs"
        } else {
            Pass "Step 5 COFF deterministic (no timestamps/random fields)"
        }

        $objPath = "$da\$stem.obj"
        # .NET static calls are blocked by the sandbox's ConstrainedLanguage
        # policy, so read bytes via Get-Content instead of [IO.File]::ReadAllBytes.
        if (Test-Path $objPath) {
            $bytes = Get-Content $objPath -Encoding Byte -TotalCount 64
            if ($bytes -ne $null -and $bytes.Count -gt 0x08) {
                # COFF header: 0x00 machine(2) 0x02 numSections(2) 0x04 timeDateStamp(4)
                # (0x18 is inside the first section's 8-byte Name field, not the stamp.)
                $ts = [uint32]($bytes[0x04] -bor ($bytes[0x05] -shl 8) -bor ($bytes[0x06] -shl 16) -bor ($bytes[0x07] -shl 24))
                if ($ts -eq 0) { Pass "Step 5 TimeDateStamp = 0" }
                else { Fail "Step 5 TimeDateStamp" "0x$($ts.ToString('X8'))" }
            }
        }
    }
}

# ---- summary ----
Log ""
Log "============================================================"
Log " Photon bootstrap: PASS=$pass  FAIL=$fail  SKIP=$skip"
Log "============================================================"
$reportPath = Join-Path $OutDir "bootstrap-report.txt"
$report | Out-File $reportPath -Encoding utf8
Log "Report: $reportPath"

if ($fail -gt 0) {
    Write-Host ""
    Write-Host "Failures:" -ForegroundColor Red
}
exit $fail
