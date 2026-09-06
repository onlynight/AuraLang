# 编译外部插件
#
# 用法：
#   pwsh build-plugin.ps1
#
# 输出：
#   Windows: plugins/target/release/custom_plugin.dll
#   Linux:   plugins/target/release/libcustom_plugin.so
#   macOS:   plugins/target/release/libcustom_plugin.dylib

param(
    [string]$Profile = "release"
)

$pluginDir = Join-Path $PSScriptRoot "plugins"

Write-Host "=== 编译外部插件 ===" -ForegroundColor Cyan
Write-Host "目录: $pluginDir"
Write-Host "配置: $Profile"
Write-Host ""

Push-Location $pluginDir
try {
    $cargoOutput = & cargo build --$Profile 2>&1
    $cargoOutput | ForEach-Object { $_.ToString() }
    
    if ($LASTEXITCODE -ne 0) {
        Write-Host "编译失败" -ForegroundColor Red
        exit 1
    }
    
    Write-Host ""
    Write-Host "=== 编译成功 ===" -ForegroundColor Green
    
    # 显示输出文件
    $releaseDir = Join-Path $pluginDir "target\$Profile"
    $dllFiles = Get-ChildItem $releaseDir -Filter "*.dll" -ErrorAction SilentlyContinue
    $soFiles = Get-ChildItem $releaseDir -Filter "*.so" -ErrorAction SilentlyContinue
    $dylibFiles = Get-ChildItem $releaseDir -Filter "*.dylib" -ErrorAction SilentlyContinue
    
    foreach ($f in @($dllFiles, $soFiles, $dylibFiles)) {
        foreach ($file in $f) {
            Write-Host "  ✓ $($file.FullName)" -ForegroundColor Green
        }
    }
    
    Write-Host ""
    Write-Host "现在可以运行:" -ForegroundColor Cyan
    Write-Host "  loom build --dir .."
}
finally {
    Pop-Location
}
