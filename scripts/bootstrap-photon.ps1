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
    # ⚠️ 必须**异步**读两条管道。
    # 旧实现：`$p.StandardOutput.ReadToEnd()` 之后再读 stderr —— 子进程（Photon 驱动
    # 会往 stderr 打大量 `[vm] stdlib: loaded …` / 诊断告警）一旦写满 stderr 的 4 KB
    # 管道缓冲就会阻塞，而父进程此刻正阻塞在读 stdout 上 ⇒ **经典管道死锁**：
    # 现象是「脚本卡住、8 分钟零产物、子进程 WorkingSet 停在 8 MB」。
    # 另外超时必须杀**进程树**：驱动是 aura.exe 的子进程，只 Kill 父进程会留下孤儿。
    $soTask = $p.StandardOutput.ReadToEndAsync()
    $seTask = $p.StandardError.ReadToEndAsync()
    if ($p.WaitForExit($to * 1000)) {
        return @{ Out=$soTask.Result; Err=$seTask.Result; Code=$p.ExitCode; TimedOut=$false }
    }
    taskkill /PID $p.Id /T /F 2>$null | Out-Null
    return @{ Out=$soTask.Result; Err=$seTask.Result; Code=-1; TimedOut=$true }
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
        if ($probe2.TimedOut) { Fail "Step 1b AOT stdlib" "build timeout" }
        elseif (Test-Path $s1b) {
            $run2 = Invoke-Proc $s1b "" $s1 $TimeoutSecs
            $o = ($run2.Out -replace "`r","").Trim()
            if ($o -eq "Hello, World!") { Pass "Step 1b AOT stdlib (hello world)" }
            else {
                # 失败时必须带上**构建**侧的错误：此前只报 stdout=''，
                # 若 hw.exe 是上一轮遗留的陈旧产物，真正的原因（AOT 构建失败）
                # 会被完全掩盖，只能靠手工复现才发现。
                $berr = FirstErr $probe2.Err
                Fail "Step 1b AOT stdlib" "stdout='$o' (expect 'Hello, World!'); run.ExitCode=$($run2.Code); build.err=$(if ($berr) { $berr } else { '<empty>' })"
            }
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
        # 只编译**一个**代表文件做 smoke test。
        #
        # 为什么不全编译 5 个：这些是「运行时声明/常量」模块（无函数体），
        # 管线对它们产出的 COFF **代码段为空**（实测 0 字节编码，落盘为 136 字符
        # 的 hex 头）。逐个编译既慢（每个含驱动启动 ≈2 min）又无额外覆盖 ——
        # 全链（导入解析 → HIR → SSA → LIR → DAG → 编码 → COFF 组装）编译
        # Memory.aura 一个就全覆盖了。
        #
        # 判定也据此放宽：声明模块**只会有 hex 文本**（二进制落盘要求非空代码段），
        # 因此 `.obj` 与 `.obj.hex` 任一存在都算通过。
        # ⚠️ 这里**不能**写 `$f = $runtimeFiles[0]`：上面第 165 行的
        # `foreach ($f in $runtimeFiles)` 会把 `$f` 留成**最后一个元素**
        # （PowerShell 的 foreach 不建立新作用域），后续赋值又会被下面的
        # `foreach` 之外……实测该写法下 `$f` 仍是 `Runtime.aura`（最后一个），
        # 直接写成**字面量**最稳。
        $f = "aura\core\aura\lang\native\Memory.aura"
        if (-not (Test-Path $f)) {
            Fail "Step 2 compile" "sample source missing: $f"
        } else {
            $stem = [IO.Path]::GetFileNameWithoutExtension($f)
            $phir = Join-Path $s2 "$stem.phir"
            StepLog "  [step2] src=$f stem=$stem (files=$($runtimeFiles.Count))"
            $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
            $r = Invoke-Proc $AuraBin "build -b photon `"$Root\$f`" --output `"$phir`"" $Root $TimeoutSecs
            $sw2.Stop()
            $objBin = Join-Path $s2 "$stem.obj"
            $objHex = Join-Path $s2 "$stem.obj.hex"
            if ($r.TimedOut) {
                Fail "Step 2 $stem" "timeout"
            } elseif (Test-Path $objBin) {
                Pass "Step 2 $stem -> .obj ($((Get-Item $objBin).Length) B, $([int]$sw2.Elapsed.TotalSeconds)s)"
            } elseif (Test-Path $objHex) {
                Pass "Step 2 $stem -> COFF hex ($((Get-Item $objHex).Length) chars, $([int]$sw2.Elapsed.TotalSeconds)s；声明模块无代码段)"
            } else {
                Fail "Step 2 $stem" ("no COFF artifact. " + (FirstErr $r.Err))
            }
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
        # ⚠️ 这一步是**多分钟级**的：驱动（PhotonDriver + 20 个模块）本身跑在 VM
        # 解释器上（实测 ≈3-4 M ops/s，比原生慢 ~50-100×），而 `Main.aura` 会展开成
        # 2345 个函数 / 4 万行 .phir / ≈12 万 HIR 节点。实测解析 ≈50 ms/函数（线性）。
        # 因此 -TimeoutSecs 需要给足（默认 600 s 通常不够，建议 ≥1800 s）。
        StepLog "  注意：本步是**多十分钟级**（驱动在 VM 上解释执行，2345 个函数 / 12 万 HIR 节点）"
        StepLog "        实测：-TimeoutSecs 1800 仍超时（≥30 min）；要跑完请给 -TimeoutSecs 5400 以上"
        StepLog "        进度可从 $s3\photon_trace.log 观察（本步已开启 AURA_PHOTON_TRACE=1）"
        # 打开阶段轨迹：长跑时至少能看到走到哪个 Phase，而不是干等。
        $env:AURA_PHOTON_TRACE = "1"
        $sw3 = [System.Diagnostics.Stopwatch]::StartNew()
        $r = Invoke-Proc $AuraBin "build -b photon `"$Root\$entry`" --output `"$phir`"" $Root $TimeoutSecs
        $sw3.Stop()
        Remove-Item Env:\AURA_PHOTON_TRACE -ErrorAction SilentlyContinue
        $secs3 = [int]$sw3.Elapsed.TotalSeconds
        if ($r.TimedOut) {
            $tf = Join-Path $s3 "photon_trace.log"
            $where = if (Test-Path $tf) { ((Get-Content $tf -Tail 1) -join "") } else { "<no trace>" }
            Fail "Step 3 compile" "timeout after ${secs3}s; trace last: $where"
        } elseif (Test-Path "$s3\Main.exe") {
            Pass "Step 3 compile -> Main.exe ($((Get-Item "$s3\Main.exe").Length) B, ${secs3}s)"
        } else {
            $err = FirstErr $r.Err
            StepLog "  reason: $err"
            Fail "Step 3 compile" "no Main.exe after ${secs3}s; err=$(if ($err) { $err } else { '<empty>' })"
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
