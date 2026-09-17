# Rust vs Aura Compiler Performance Benchmark
# Simple version without Chinese characters

Write-Host "==================================================="
Write-Host "  Rust vs Aura Compiler Benchmark"
Write-Host "==================================================="
Write-Host ""

# Test file
$testFile = "D:\Code\AuraLang\bench_test.aura"

# Compiler paths
$rustExe = "D:\Code\AuraLang\target\release\aura.exe"
$auraExe = "D:\Code\AuraLang\build\bin\aura-compiler-native2.exe"

# Output dir
$outDir = "D:\Code\AuraLang\build\bench_output"
if (-not (Test-Path $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}
Get-ChildItem -Path $outDir -Force -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

Write-Host "Test file: $testFile"
Write-Host "Rust compiler: $rustExe"
Write-Host "Aura compiler: $auraExe"
Write-Host ""

# Check if executables exist
if (-not (Test-Path $rustExe)) {
    Write-Host "ERROR: Rust compiler not found!"
    exit 1
}
if (-not (Test-Path $auraExe)) {
    Write-Host "ERROR: Aura compiler not found!"
    exit 1
}

Write-Host "Both compilers found. Running benchmarks..."
Write-Host ""

# Benchmark 1: VM compile time
Write-Host "--- Benchmark 1: VM Compile Time ---"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut1 = "$outDir\test_rust_vm.auc"
$p1 = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--output", $rustOut1 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_vm_out.txt" -RedirectStandardError "$outDir\rust_vm_err.txt"
$sw.Stop()
$rustTime1 = $sw.ElapsedMilliseconds
Write-Host "  Rust VM: $($rustTime1)ms (exit: $($p1.ExitCode))"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut1 = "$outDir\test_aura_vm.auc"
$p2 = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--output", $auraOut1 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_vm_out.txt" -RedirectStandardError "$outDir\aura_vm_err.txt"
$sw.Stop()
$auraTime1 = $sw.ElapsedMilliseconds
Write-Host "  Aura VM: $($auraTime1)ms (exit: $($p2.ExitCode))"

Write-Host ""

# Benchmark 2: AOT compile time
Write-Host "--- Benchmark 2: AOT Compile Time ---"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut2 = "$outDir\test_rust_aot.exe"
$p3 = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--aot", "--output", $rustOut2 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_aot_out.txt" -RedirectStandardError "$outDir\rust_aot_err.txt"
$sw.Stop()
$rustTime2 = $sw.ElapsedMilliseconds
Write-Host "  Rust AOT: $($rustTime2)ms (exit: $($p3.ExitCode))"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut2 = "$outDir\test_aura_aot.exe"
$p4 = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--aot", "--output", $auraOut2 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_aot_out.txt" -RedirectStandardError "$outDir\aura_aot_err.txt"
$sw.Stop()
$auraTime2 = $sw.ElapsedMilliseconds
Write-Host "  Aura AOT: $($auraTime2)ms (exit: $($p4.ExitCode))"

Write-Host ""

# Benchmark 3: LLVM IR generation
Write-Host "--- Benchmark 3: LLVM IR Generation ---"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$rustOut3 = "$outDir\test_rust.ll"
$p5 = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--emit-llvm", "--output", $rustOut3 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\rust_ll_out.txt" -RedirectStandardError "$outDir\rust_ll_err.txt"
$sw.Stop()
$rustTime3 = $sw.ElapsedMilliseconds
Write-Host "  Rust LLVM IR: $($rustTime3)ms (exit: $($p5.ExitCode))"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$auraOut3 = "$outDir\test_aura.ll"
$p6 = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--emit-llvm", "--output", $auraOut3 -NoNewWindow -Wait -PassThru -RedirectStandardOutput "$outDir\aura_ll_out.txt" -RedirectStandardError "$outDir\aura_ll_err.txt"
$sw.Stop()
$auraTime3 = $sw.ElapsedMilliseconds
Write-Host "  Aura LLVM IR: $($auraTime3)ms (exit: $($p6.ExitCode))"

Write-Host ""

# Benchmark 4: Output sizes
Write-Host "--- Benchmark 4: Output Sizes ---"

if (Test-Path $rustOut1) {
    Write-Host "  Rust VM (.auc): $((Get-Item $rustOut1).Length) bytes"
} else {
    Write-Host "  Rust VM (.auc): N/A"
}

if (Test-Path $auraOut1) {
    Write-Host "  Aura VM (.auc): $((Get-Item $auraOut1).Length) bytes"
} else {
    Write-Host "  Aura VM (.auc): N/A"
}

if (Test-Path $rustOut2) {
    Write-Host "  Rust AOT (.exe): $((Get-Item $rustOut2).Length) bytes"
} else {
    Write-Host "  Rust AOT (.exe): N/A"
}

if (Test-Path $auraOut2) {
    Write-Host "  Aura AOT (.exe): $((Get-Item $auraOut2).Length) bytes"
} else {
    Write-Host "  Aura AOT (.exe): N/A"
}

if (Test-Path $rustOut3) {
    Write-Host "  Rust LLVM IR: $((Get-Item $rustOut3).Length) bytes"
} else {
    Write-Host "  Rust LLVM IR: N/A"
}

if (Test-Path $auraOut3) {
    Write-Host "  Aura LLVM IR: $((Get-Item $auraOut3).Length) bytes"
} else {
    Write-Host "  Aura LLVM IR: N/A"
}

Write-Host ""

# Benchmark 5: Memory usage
Write-Host "--- Benchmark 5: Memory Usage ---"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p7 = Start-Process -FilePath $rustExe -ArgumentList "build", $testFile, "--output", "$outDir\mem_rust.auc" -NoNewWindow -Wait -PassThru
$sw.Stop()
$rustMem = $p7.PeakedMemorySize64
Write-Host "  Rust peak memory: $([math]::Round($rustMem / 1MB, 2)) MB"

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p8 = Start-Process -FilePath $auraExe -ArgumentList "build", $testFile, "--output", "$outDir\mem_aura.auc" -NoNewWindow -Wait -PassThru
$sw.Stop()
$auraMem = $p8.PeakedMemorySize64
Write-Host "  Aura peak memory: $([math]::Round($auraMem / 1MB, 2)) MB"

Write-Host ""

# Summary
Write-Host "==================================================="
Write-Host "  Summary"
Write-Host "==================================================="
Write-Host ""

if ($auraTime1 -gt 0) {
    Write-Host "  VM ratio (Rust:Aura): $([math]::Round($rustTime1 / $auraTime1, 2))"
}
if ($auraTime2 -gt 0) {
    Write-Host "  AOT ratio (Rust:Aura): $([math]::Round($rustTime2 / $auraTime2, 2))"
}
if ($auraTime3 -gt 0) {
    Write-Host "  LLVM IR ratio (Rust:Aura): $([math]::Round($rustTime3 / $auraTime3, 2))"
}
if ($auraMem -gt 0) {
    Write-Host "  Memory ratio (Rust:Aura): $([math]::Round($rustMem / $auraMem, 2))"
}

Write-Host ""
Write-Host "  Compile times (ms):"
Write-Host "    VM:      Rust=$rustTime1  Aura=$auraTime1"
Write-Host "    AOT:     Rust=$rustTime2  Aura=$auraTime2"
Write-Host "    LLVM IR: Rust=$rustTime3  Aura=$auraTime3"
Write-Host ""
Write-Host "  Peak memory (MB):"
Write-Host "    Rust: $([math]::Round($rustMem / 1MB, 2)) MB"
Write-Host "    Aura: $([math]::Round($auraMem / 1MB, 2)) MB"

Write-Host ""
Write-Host "==================================================="
Write-Host "  Benchmark Complete"
Write-Host "==================================================="
