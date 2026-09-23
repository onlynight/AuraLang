# Photon IR ST4 Plugin Deploy Script
# Usage: .\deploy.ps1 [-Clean]

param(
    [switch]$Clean
)

$src = $PSScriptRoot
$dst = Join-Path $env:APPDATA "Sublime Text\Packages\PhotonLanguage"

if ($Clean) {
    if (Test-Path $dst) {
        Remove-Item $dst -Recurse -Force
        Write-Host "Cleaned: $dst"
    }
}

# Create destination
New-Item -ItemType Directory -Path $dst -Force | Out-Null

# Copy all plugin assets
Get-ChildItem -Path $src -Force | Copy-Item -Destination $dst -Recurse -Force

# Report
Write-Host "`nDeployed Photon IR plugin to: $dst"
Write-Host "Files deployed:"
Get-ChildItem $dst -Recurse -File | Select-Object @{N="Relative";E={$_.FullName.Substring($dst.Length+1)}}, Length | Format-Table -AutoSize
