# Rust vs Aura 编译器性能基准测试
# 对比编译时间、内存使用、输出大小

Write-Host "==================================================="
Write-Host "  Rust vs Aura Compiler Performance Benchmark"
Write-Host "==================================================="
Write-Host ""

# 测试文件
$testFile = "D:\Code\AuraLang\bench_test.aura"

# 编译器路径
$rustExe = "D:\Code\AuraLang\target\release\aura.exe"
$auraExe = "D:\Code\AuraLang\build\bin\aura-compiler-native2.exe"

# 输出目录
$outDir = "D:\Code\AuraLang\build\bench_output"
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}

# 清理旧输出
Get-ChildItem -Path $outDir -Force -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

Write-Host "测试文件: $testFile"
Write-Host "Rust 编译器: $rustExe"
Write-Host "Aura 编译器: $auraExe"
Write-Host ""

# ═══════════════════════════════════════════════════════════
# 基准 1: 编译时间 (VM 模式)
# ═══════════════════════════════════════════════════════════

Write-Host "━━━ 基准 1: 编译时间 (VM 模式) ━━━"

# Rust 编译器
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut1 = "$outDir\test_rust_vm.auc"
$proc = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--output", $rustOut1 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_vm_out.txt" -RedirectStandardError "$outDir\rust_vm_err.txt"
$sw.Stop()
$rustTime1 = $sw.ElapsedMilliseconds
Write-Host "  Rust 编译器 (VM): $($rustTime1)ms (exit: $($proc.ExitCode))"

# Aura 编译器
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut1 = "$outDir\test_aura_vm.auc"
$proc = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--output", $auraOut1 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_vm_out.txt" -RedirectStandardError "$outDir\aura_vm_err.txt"
$sw.Stop()
$auraTime1 = $sw.ElapsedMilliseconds
Write-Host "  Aura 编译器 (VM): $($auraTime1)ms (exit: $($proc.ExitCode))"

Write-Host ""

# ═══════════════════════════════════════════════════════════
# 基准 2: 编译时间 (AOT 模式)
# ═══════════════════════════════════════════════════════════

Write-Host "━━━ 基准 2: 编译时间 (AOT 模式) ━━━"

# Rust 编译器
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut2 = "$outDir\test_rust_aot.exe"
$proc = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--aot", "--output", $rustOut2 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_aot_out.txt" -RedirectStandardError "$outDir\rust_aot_err.txt"
$sw.Stop()
$rustTime2 = $sw.ElapsedMilliseconds
Write-Host "  Rust 编译器 (AOT): $($rustTime2)ms (exit: $($proc.ExitCode))"

# Aura 编译器 (AOT 可能不支持，跳过或记录错误)
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut2 = "$outDir\test_aura_aot.exe"
$proc = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--aot", "--output", $auraOut2 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_aot_out.txt" -RedirectStandardError "$outDir\aura_aot_err.txt"
$sw.Stop()
$auraTime2 = $sw.ElapsedMilliseconds
Write-Host "  Aura 编译器 (AOT): $($auraTime2)ms (exit: $($proc.ExitCode))"

Write-Host ""

# ═══════════════════════════════════════════════════════════
# 基准 3: 编译时间 (LLVM IR 生成)
# ═══════════════════════════════════════════════════════════

Write-Host "━━━ 基准 3: 编译时间 (LLVM IR) ━━━"

# Rust 编译器
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut3 = "$outDir\test_rust.ll"
$proc = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--emit-llvm", "--output", $rustOut3 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_ll_out.txt" -RedirectStandardError "$outDir\rust_ll_err.txt"
$sw.Stop()
$rustTime3 = $sw.ElapsedMilliseconds
Write-Host "  Rust 编译器 (LLVM IR): $($rustTime3)ms (exit: $($proc.ExitCode))"

# Aura 编译器
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut3 = "$outDir\test_aura.ll"
$proc = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--emit-llvm", "--output", $auraOut3 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_ll_out.txt" -RedirectStandardError "$outDir\aura_ll_err.txt"
$sw.Stop()
$auraTime3 = $sw.ElapsedMilliseconds
Write-Host "  Aura 编译器 (LLVM IR): $($auraTime3)ms (exit: $($proc.ExitCode))"

Write-Host ""

# ═══════════════════════════════════════════════════════════
# 基准 4: 输出文件大小
# ═══════════════════════════════════════════════════════════

Write-Host "━━━ 基准 4: 输出文件大小 ━━━"

if (Test-Path $rustOut1) {
    $rustSize1 = (Get-Item $rustOut1).Length
    Write-Host "  Rust VM (.auc): $rustSize1 bytes"
} else {
    Write-Host "  Rust VM (.auc): 未生成"
}

if (Test-Path $auraOut1) {
    $auraSize1 = (Get-Item $auraOut1).Length
    Write-Host "  Aura VM (.auc): $auraSize1 bytes"
} else {
    Write-Host "  Aura VM (.auc): 未生成"
}

if (Test-Path $rustOut2) {
    $rustSize2 = (Get-Item $rustOut2).Length
    Write-Host "  Rust AOT (.exe): $rustSize2 bytes"
} else {
    Write-Host "  Rust AOT (.exe): 未生成"
}

if (Test-Path $auraOut2) {
    $auraSize2 = (Get-Item $auraOut2).Length
    Write-Host "  Aura AOT (.exe): $auraSize2 bytes"
} else {
    Write-Host "  Aura AOT (.exe): 未生成"
}

if (Test-Path $rustOut3) {
    $rustSize3 = (Get-Item $rustOut3).Length
    Write-Host "  Rust LLVM IR (.ll): $rustSize3 bytes"
} else {
    Write-Host "  Rust LLVM IR (.ll): 未生成"
}

if (Test-Path $auraOut3) {
    $auraSize3 = (Get-Item $auraOut3).Length
    Write-Host "  Aura LLVM IR (.ll): $auraSize3 bytes"
} else {
    Write-Host "  Aura LLVM IR (.ll): 未生成"
}

Write-Host ""

# ═══════════════════════════════════════════════════════════
# 基准 5: 内存使用 (峰值)
# ═══════════════════════════════════════════════════════════

Write-Host "━━━ 基准 5: 内存使用 (峰值) ━━━"

# Rust 编译器内存
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--output", "$outDir\mem_rust.auc" -NoNewWindow -Wait -PassThru
$sw.Stop()
$rustMem = $proc.PeakedMemorySize64
Write-Host "  Rust 编译器峰值内存: $([math]::Round($rustMem / 1MB, 2)) MB"

# Aura 编译器内存
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--output", "$outDir\mem_aura.auc" -NoNewWindow -Wait -PassThru
$sw.Stop()
$auraMem = $proc.PeakedMemorySize64
Write-Host "  Aura 编译器峰值内存: $([math]::Round($auraMem / 1MB, 2)) MB"

Write-Host ""

# ═══════════════════════════════════════════════════════════
# 汇总
# ═══════════════════════════════════════════════════════════

Write-Host "==================================================="
Write-Host "  性能对比汇总"
Write-Host "==================================================="
Write-Host ""

# 计算比率
if ($auraTime1 -gt 0) {
    $ratio1 = [math]::Round($rustTime1 / $auraTime1, 2)
    Write-Host "  VM 编译时间比率 (Rust:Aura): $ratio1"
}
if ($auraTime2 -gt 0) {
    $ratio2 = [math]::Round($rustTime2 / $auraTime2, 2)
    Write-Host "  AOT 编译时间比率 (Rust:Aura): $ratio2"
}
if ($auraTime3 -gt 0) {
    $ratio3 = [math]::Round($rustTime3 / $auraTime3, 2)
    Write-Host "  LLVM IR 时间比率 (Rust:Aura): $ratio3"
}
if ($auraMem -gt 0) {
    $ratioMem = [math]::Round($rustMem / $auraMem, 2)
    Write-Host "  内存比率 (Rust:Aura): $ratioMem"
}

Write-Host ""
Write-Host "==================================================="
Write-Host "  详细数据"
Write-Host "==================================================="
Write-Host ""
Write-Host "  编译时间 (ms):"
Write-Host "    VM 模式:     Rust=$rustTime1  Aura=$auraTime1"
Write-Host "    AOT 模式:    Rust=$rustTime2  Aura=$auraTime2"
Write-Host "    LLVM IR:     Rust=$rustTime3  Aura=$auraTime3"
Write-Host ""
Write-Host "  输出大小 (bytes):"
if (Test-Path $rustOut1) { Write-Host "    VM (.auc):    Rust=$(if (Test-Path $rustOut1) { (Get-Item $rustOut1).Length } else { 'N/A' })  Aura=$(if (Test-Path $auraOut1) { (Get-Item $auraOut1).Length } else { 'N/A' })" }
if (Test-Path $rustOut2) { Write-Host "    AOT (.exe):   Rust=$(if (Test-Path $rustOut2) { (Get-Item $rustOut2).Length } else { 'N/A' })  Aura=$(if (Test-Path $auraOut2) { (Get-Item $auraOut2).Length } else { 'N/A' })" }
if (Test-Path $rustOut3) { Write-Host "    LLVM IR:      Rust=$(if (Test-Path $rustOut3) { (Get-Item $rustOut3).Length } else { 'N/A' })  Aura=$(if (Test-Path $auraOut3) { (Get-Item $auraOut3).Length } else { 'N/A' })" }
Write-Host ""
Write-Host "  峰值内存 (MB):"
Write-Host "    Rust: $([math]::Round($rustMem / 1MB, 2)) MB"
Write-Host "    Aura: $([math]::Round($auraMem / 1MB, 2)) MB"
