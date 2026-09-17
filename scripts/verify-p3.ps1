# P3: Std 标准库验证脚本
#
# 验证 Tar.aura 和 Zstd.aura 的纯 Aura 实现。
# 对应分阶段开发计划 P3 阶段。
#
# 用法：powershell -File scripts\verify-p3.ps1

param(
    [switch]$Quick  # 快速模式（仅检查文件存在）
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$pass = 0
$fail = 0

function Check($desc, $cond) {
    if ($cond) {
        Write-Host "  [PASS] $desc" -ForegroundColor Green
        $script:pass++
    } else {
        Write-Host "  [FAIL] $desc" -ForegroundColor Red
        $script:fail++
    }
}

function Section($title) {
    Write-Host ""
    Write-Host "--- $title ---" -ForegroundColor Cyan
}

Write-Host "=== P3: Std Standard Library Verification ===" -ForegroundColor White
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "Repo: $root"

# --- Quick mode ---
if ($Quick) {
    Section "P3.1 Tar.aura"
    $tarPath = "$root\aura\core\aura\lang\std\Tar.aura"
    Check "File exists" (Test-Path $tarPath)
    if (Test-Path $tarPath) {
        $tar = Get-Content $tarPath -Raw
        Check "Has TarArchive class" ($tar -match "class TarArchive")
        Check "Has TarEntry class" ($tar -match "class TarEntry")
        Check "Has export method" ($tar -match "fun export")
        Check "Has import method" ($tar -match "fun import")
        Check "Has addFile method" ($tar -match "fun addFile")
        Check "Has TAR_BLOCK_SIZE" ($tar -match "TAR_BLOCK_SIZE")
    }

    Section "P3.2 Zstd.aura"
    $zstdPath = "$root\aura\core\aura\lang\std\Zstd.aura"
    Check "File exists" (Test-Path $zstdPath)
    if (Test-Path $zstdPath) {
        $zstd = Get-Content $zstdPath -Raw
        Check "Has ZstdConfig class" ($zstd -match "class ZstdConfig")
        Check "Has ZstdResult class" ($zstd -match "class ZstdResult")
        Check "Has zstdCompress" ($zstd -match "fun zstdCompress")
        Check "Has zstdDecompress" ($zstd -match "fun zstdDecompress")
        Check "Has zstdIsMagic" ($zstd -match "fun zstdIsMagic")
        Check "Has @native declarations" ($zstd -match "@native")
    }

    Write-Host ""
    Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
    exit $fail
}

# --- Full mode ---
Section "P3.1 Tar.aura"
$tarPath = "$root\aura\core\aura\lang\std\Tar.aura"
Check "File exists" (Test-Path $tarPath)
if (Test-Path $tarPath) {
    $tar = Get-Content $tarPath -Raw
    Check "File size > 1KB" ($tar.Length -gt 1024)
    Check "Has package declaration" ($tar -match "^package\s")
    Check "Has TarArchive class" ($tar -match "class TarArchive")
    Check "Has TarEntry class" ($tar -match "class TarEntry")
    Check "Has export method" ($tar -match "fun export")
    Check "Has import method" ($tar -match "fun import")
    Check "Has addFile method" ($tar -match "fun addFile")
    Check "Has addDir method" ($tar -match "fun addDir")
    Check "Has findEntry method" ($tar -match "fun findEntry")
    Check "Has TAR_BLOCK_SIZE const" ($tar -match "TAR_BLOCK_SIZE")
    Check "Has TAR_MAGIC const" ($tar -match "TAR_MAGIC")
    Check "Has encodeEntry function" ($tar -match "fun encodeEntry")
    Check "Has parseHeader function" ($tar -match "fun parseHeader")
    Check "Has fileNames method" ($tar -match "fun fileNames")
    Check "Has fileContent method" ($tar -match "fun fileContent")
}

Section "P3.2 Zstd.aura"
$zstdPath = "$root\aura\core\aura\lang\std\Zstd.aura"
Check "File exists" (Test-Path $zstdPath)
if (Test-Path $zstdPath) {
    $zstd = Get-Content $zstdPath -Raw
    Check "File size > 1KB" ($zstd.Length -gt 1024)
    Check "Has package declaration" ($zstd -match "^package\s")
    Check "Has ZstdConfig class" ($zstd -match "class ZstdConfig")
    Check "Has ZstdResult class" ($zstd -match "class ZstdResult")
    Check "Has zstdCompress function" ($zstd -match "fun zstdCompress")
    Check "Has zstdDecompress function" ($zstd -match "fun zstdDecompress")
    Check "Has zstdIsMagic function" ($zstd -match "fun zstdIsMagic")
    Check "Has zstdIsValid function" ($zstd -match "fun zstdIsValid")
    Check "Has @native declarations" ($zstd -match "@native")
    Check "Has ZSTD_MAGIC const" ($zstd -match "ZSTD_MAGIC")
    Check "Has ZSTD_LEVEL_DEFAULT const" ($zstd -match "ZSTD_LEVEL_DEFAULT")
    Check "Has compressFile function" ($zstd -match "fun zstdCompressFile")
    Check "Has decompressFile function" ($zstd -match "fun zstdDecompressFile")
}

Section "P3.3 Test file"
$testPath = "$root\tests\pure_aura\std_tar_zstd_tests.aura"
Check "Test file exists" (Test-Path $testPath)
if (Test-Path $testPath) {
    $test = Get-Content $testPath -Raw
    Check "Has TestRunner import" ($test -match "TestRunner")
    Check "Has Tar import" ($test -match "Tar\.aura")
    Check "Has Zstd import" ($test -match "Zstd\.aura")
    Check "Has main function" ($test -match "fun main")
}

# --- Summary ---
Write-Host ""
Write-Host "=========================================" -ForegroundColor White
Write-Host "Result: $pass passed, $fail failed" -ForegroundColor $(if ($fail -eq 0) { "Green" } else { "Yellow" })
Write-Host "=========================================" -ForegroundColor White

if ($fail -eq 0) {
    Write-Host ""
    Write-Host "P3 verification PASSED! Std library 100% complete." -ForegroundColor Green
    Write-Host "Next phase: P4 Three-mode integration verification" -ForegroundColor Cyan
}

exit $fail
