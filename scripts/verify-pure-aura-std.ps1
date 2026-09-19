#Requires -Version 5.0
# -------------------------------------------------------------
# 条件4 校验：std 加载走 Aura 源码、不依赖 Rust 预编译的 build/*.auc
#
# 做法（实证，而非静态检查）：
#   1. 把所有 build/**/*.auc（Rust `aura stdlib-compile` 的产物）临时移出工作区
#   2. 用冻结自举载体 dist/bootstrap/aura-compiler.exe 编译一个含 std 依赖的程序
#      （String 方法 / 集合 / FileSystem 等），产出原生 exe 并运行
#   3. 再用同一载体自举编译 Main.aura（编译器自身，依赖 40+ std 模块）
#   4. 断言 build 下已无 .auc（编译期间没有被重建/读取）
#   5. 无论成功失败，finally 中还原全部 .auc
#
# 通过标准：第 1~4 步全部成立 —— 说明 Aura 侧 AOT 链路的 std 完全来自
#           aura/core/**/*.aura 源码（经 AotModuleLinker 递归合并），
#           与 Rust 生成的 .auc 无任何关系。
#
# 外部依赖：LLVM（llc / clang）+ 系统 CRT。
#
# Usage:
#   scripts\verify-pure-aura-std.ps1
#   scripts\verify-pure-aura-std.ps1 -LlvmHome <dir>
# -------------------------------------------------------------
param(
    [string]$LlvmHome = "D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc",
    [switch]$Help
)

$ErrorActionPreference = 'Stop'

$RootDir  = (Split-Path -Parent $PSScriptRoot)
$Frozen   = Join-Path $RootDir 'dist/bootstrap/aura-compiler.exe'
$MainAura = Join-Path $RootDir 'aura/compiler/aura/lang/compiler/Main.aura'
$TestFile = Join-Path $RootDir 'tests/string_methods_test.aura'
$BuildDir = Join-Path $RootDir 'build'
$OutDir   = Join-Path $BuildDir 'test'

if ($Help) {
    Write-Host "Usage: scripts\verify-pure-aura-std.ps1 [-LlvmHome <dir>]"
    exit 0
}

Set-Location $RootDir
New-Item -ItemType Directory -Path $OutDir -Force | Out-Null

function Step($msg) { Write-Host ""; Write-Host "[$msg]" -ForegroundColor Cyan }

# ── 前置检查 ──
Step "Step 0: preconditions"
if (-not (Test-Path $Frozen))   { Write-Host "  FATAL: frozen carrier missing: $Frozen" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $MainAura)) { Write-Host "  FATAL: Main.aura missing" -ForegroundColor Red; exit 1 }
if (-not (Test-Path $TestFile)) { Write-Host "  FATAL: test file missing: $TestFile" -ForegroundColor Red; exit 1 }
if (-not (Test-Path (Join-Path $LlvmHome 'bin/llc.exe'))) { Write-Host "  FATAL: llc not found under $LlvmHome" -ForegroundColor Red; exit 1 }
Write-Host "  carrier : $Frozen" -ForegroundColor Green
Write-Host "  llvm    : $LlvmHome" -ForegroundColor Green

# ── 隔离 build/**/*.auc ──
$Backup = Join-Path $env:TEMP ("aura-auc-backup-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $Backup -Force | Out-Null
$MapFile = Join-Path $Backup 'mapping.txt'
$before = @(Get-ChildItem $BuildDir -Recurse -File -Filter *.auc -ErrorAction SilentlyContinue)
Write-Host ("  found {0} .auc file(s) under build/" -f $before.Count)

$exitCode = 0
try {
    Step "Step 1: temporarily move build/**/*.auc out of the workspace"
    $buildFull = (Resolve-Path $BuildDir).Path
    $lines = New-Object System.Collections.Generic.List[string]
    foreach ($f in $before) {
        $rel = $f.FullName.Substring($buildFull.Length).TrimStart('\', '/')
        $dst = Join-Path $Backup $rel
        New-Item -ItemType Directory -Path (Split-Path $dst) -Force | Out-Null
        Move-Item -LiteralPath $f.FullName -Destination $dst -Force
        $lines.Add($rel)
    }
    Set-Content -Path $MapFile -Value $lines -Encoding UTF8
    $left = @(Get-ChildItem $BuildDir -Recurse -File -Filter *.auc -ErrorAction SilentlyContinue)
    if ($left.Count -ne 0) {
        Write-Host "  FATAL: still $($left.Count) .auc file(s) under build/" -ForegroundColor Red
        $exitCode = 1
    } else {
        Write-Host "  OK: no .auc left under build/" -ForegroundColor Green
    }

    # ── Step 2: compile + run a std-dependent program ──
    if ($exitCode -eq 0) {
        Step "Step 2: compile a std-dependent program WITHOUT any .auc"
        $exe = Join-Path $OutDir 'noauc_probe.exe'
        & $Frozen $TestFile -o $exe
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) {
            Write-Host "  FATAL: compilation failed (exit=$LASTEXITCODE)" -ForegroundColor Red
            $exitCode = 1
        } else {
            Write-Host "  OK: $exe" -ForegroundColor Green
            $out = & $exe
            $txt = ($out | Out-String)
            if ($txt -match 'DONE') {
                Write-Host "  OK: program ran, std behaviour correct" -ForegroundColor Green
            } else {
                Write-Host "  FATAL: unexpected program output:" -ForegroundColor Red
                Write-Host $txt
                $exitCode = 1
            }
        }
    }

    # ── Step 3: self-bootstrap without any .auc ──
    if ($exitCode -eq 0) {
        Step "Step 3: self-bootstrap Main.aura WITHOUT any .auc"
        $n1 = Join-Path $OutDir 'noauc_boot.exe'
        & $Frozen $MainAura -o $n1
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path $n1)) {
            Write-Host "  FATAL: self-bootstrap failed (exit=$LASTEXITCODE)" -ForegroundColor Red
            $exitCode = 1
        } else {
            Write-Host "  OK: $n1" -ForegroundColor Green
        }
    }

    # ── Step 4: assert no .auc re-appeared ──
    if ($exitCode -eq 0) {
        Step "Step 4: assert no .auc was read/regenerated"
        $after = @(Get-ChildItem $BuildDir -Recurse -File -Filter *.auc -ErrorAction SilentlyContinue)
        if ($after.Count -ne 0) {
            Write-Host "  FATAL: $($after.Count) .auc appeared during compilation" -ForegroundColor Red
            $exitCode = 1
        } else {
            Write-Host "  OK: build/ still contains zero .auc" -ForegroundColor Green
        }
    }
}
finally {
    # ── 还原 .auc（无条件执行） ──
    Step "Restore: move .auc files back"
    if (Test-Path $MapFile) {
        $buildFull = (Resolve-Path $BuildDir).Path
        foreach ($rel in (Get-Content $MapFile)) {
            if ([string]::IsNullOrWhiteSpace($rel)) { continue }
            $src = Join-Path $Backup $rel
            $dst = Join-Path $buildFull $rel
            if (Test-Path $src) {
                New-Item -ItemType Directory -Path (Split-Path $dst) -Force | Out-Null
                Move-Item -LiteralPath $src -Destination $dst -Force
            }
        }
        $restored = @(Get-ChildItem $BuildDir -Recurse -File -Filter *.auc -ErrorAction SilentlyContinue).Count
        Write-Host ("  restored {0} .auc file(s)" -f $restored) -ForegroundColor Green
    }
    Remove-Item -LiteralPath $Backup -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
if ($exitCode -eq 0) {
    Write-Host "==============================================================" -ForegroundColor Cyan
    Write-Host "  PASS: std comes from Aura sources; no dependency on build/*.auc" -ForegroundColor Cyan
    Write-Host "==============================================================" -ForegroundColor Cyan
} else {
    Write-Host "==============================================================" -ForegroundColor Red
    Write-Host "  FAIL: pure-Aura std loading verification failed" -ForegroundColor Red
    Write-Host "==============================================================" -ForegroundColor Red
}
exit $exitCode
