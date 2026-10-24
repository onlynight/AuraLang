param([string]$Exe, [string]$ArgsStr = "", [int]$Runs = 5)
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = (Resolve-Path $Exe).Path
$psi.Arguments = $ArgsStr
$psi.WorkingDirectory = (Get-Location).Path
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$times = @()
$last = ""
for ($i = 0; $i -lt $Runs; $i++) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $p = [System.Diagnostics.Process]::Start($psi)
    $soT = $p.StandardOutput.ReadToEndAsync()
    $seT = $p.StandardError.ReadToEndAsync()
    $p.WaitForExit() | Out-Null
    $sw.Stop()
    $times += [math]::Round($sw.Elapsed.TotalMilliseconds, 0)
    try { $last = ($soT.Result -split "`r?`n" | Where-Object { $_.Trim() -ne "" } | Select-Object -Last 1) } catch {}
}
$sorted = $times | Sort-Object
$med = $sorted[[int][Math]::Floor($sorted.Count / 2)]
Write-Host ("times=[" + ($times -join ",") + "] median=" + $med + " last=[" + $last + "]")
