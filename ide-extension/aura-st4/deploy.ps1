# Aura ST4 Plugin Deploy Script
# Usage: .\deploy.ps1 [-Clean]

param(
    [switch]$Clean
)

$src = $PSScriptRoot
$dst = Join-Path $env:APPDATA "Sublime Text\Packages\AuraLanguage"

if ($Clean) {
    if (Test-Path $dst) {
        Remove-Item $dst -Recurse -Force
        Write-Host "Cleaned: $dst"
    }
}

# Create destination
New-Item -ItemType Directory -Path $dst -Force | Out-Null

# Copy files, excluding test/ and __pycache__
$excludeDirs = @("test", "__pycache__")
$items = Get-ChildItem -Path $src -Force | Where-Object { $_.Name -notin $excludeDirs }
foreach ($item in $items) {
    Copy-Item -Path $item.FullName -Destination $dst -Recurse -Force
}

# Report
Write-Host "`nDeployed Aura Language plugin to: $dst"
Write-Host "Files deployed:"
Get-ChildItem $dst -Recurse -File | Select-Object @{N="Relative";E={$_.FullName.Substring($dst.Length+1)}}, Length | Format-Table -AutoSize
