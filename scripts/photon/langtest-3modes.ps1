# langtest-3modes.ps1 -- tests/language-test/*.aura × 三种执行模式矩阵
#
#   模式 A：Aura VM     —— 种子 VM 编译并解释（aura.exe run）
#   模式 B：Photon JIT  —— 编译成 .auc，交 Photon 原生宿主 jit_vmhost.exe 执行
#                          （宿主内建 JitBackend；AURA_VM_AUC 指定字节码）
#   模式 C：Photon AOT  —— HAT 驱动 PhotonHatCompile.exe 编译成原生 exe 后执行
#
# 判据（与仓库既有口径一致）：退出码 0 且未超时 = PASS。
# 附加：记录每个模式的「最后一行非空输出」，三方不一致时标 DIFF 供 triage
#       （一致性不作判据——JIT 宿主的 [XX] 调试行与 AOT/VM 的输出面本来就不同）。
#
# 用法:
#   powershell -File build\langtest-3modes.ps1
#   powershell -File build\langtest-3modes.ps1 -Only 01,05 -Tag probe
param(
  [string]$Only = '',
  [string]$Tag  = 'lt3',
  [int]$TimeoutSecs = 180
)
$ErrorActionPreference = 'Continue'
$Root   = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
Set-Location $Root
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"

$Seed   = Join-Path $Root 'rust\target\release\aura.exe'
$Driver = Join-Path $Root 'build\hat-native\PhotonHatCompile.exe'
$JitHst = Join-Path $Root 'build\jit-verify\jit_vmhost.exe'
$Work   = Join-Path $Root ('build\langtest-3modes\' + $Tag)
New-Item -ItemType Directory -Force -Path $Work | Out-Null

foreach ($p in @($Seed, $Driver, $JitHst)) {
  if (-not (Test-Path $p)) { Write-Host "missing: $p" -ForegroundColor Red; exit 1 }
}

# ── 跑一个进程：收集 exit / 耗时 / stdout / stderr，超时整树收割 ──
function Run-Probe([string]$Exe, [string[]]$ArgList, [hashtable]$EnvMap, [int]$TimeoutMs) {
  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $Exe
  $psi.Arguments = ($ArgList -join ' ')
  $psi.UseShellExecute = $false
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.WorkingDirectory = $Root
  # ⚠️ 环境变量一律进程级赋值（PS 5.1 下 $psi.EnvironmentVariables 为 $null）。
  # 保存/恢复用 .Add() 而非索引赋值：索引赋值在本脚本里实测会抛
  # 「无法对 Null 数组进行索引」（同族坑见 scripts/photon/bench-jit-aot-vm.ps1）。
  $saved = @{}
  if ($EnvMap) {
    foreach ($k in $EnvMap.Keys) {
      $saved.Add($k, [Environment]::GetEnvironmentVariable($k))
      [Environment]::SetEnvironmentVariable($k, $EnvMap[$k])
    }
  }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = [System.Diagnostics.Process]::Start($psi)
  $soT = $p.StandardOutput.ReadToEndAsync()
  $seT = $p.StandardError.ReadToEndAsync()
  $tmo = $false
  while (-not $p.HasExited) {
    if ($sw.ElapsedMilliseconds -gt $TimeoutMs) {
      $tmo = $true
      try { & taskkill.exe /T /F /PID $p.Id 2>&1 | Out-Null } catch { try { $p.Kill() } catch {} }
      break
    }
    Start-Sleep -Milliseconds 10
  }
  try { $p.WaitForExit(5000) | Out-Null } catch {}
  $sw.Stop()
  if ($EnvMap) {
    foreach ($kv in $saved.GetEnumerator()) { [Environment]::SetEnvironmentVariable($kv.Key, $kv.Value) }
  }
  $so = ''; $se = ''
  try { $so = $soT.Result } catch {}
  try { $se = $seT.Result } catch {}
  $code = -1
  try { $code = $p.ExitCode } catch {}
  return @{ Ms = [math]::Round($sw.Elapsed.TotalMilliseconds, 0); Exit = $code; Out = $so; Err = $se; Tmo = $tmo }
}

# ── 取「最后一行非空、且不像宿主调试行」的输出 ──
function LastLine([string]$text) {
  if (-not $text) { return '' }
  $lines = $text -split "`r?`n"
  $best = ''
  foreach ($l in $lines) {
    $t = $l.Trim()
    if ($t -eq '') { continue }
    if ($t -match '^\[(VEH|ST|CP|FLAT|AS|OC|CL|BLC|winC|MAIN|BENCH)') { continue }
    if ($t -match '^(aura|Error|error):') { continue }
    $best = $t
  }
  return $best
}

$tests = Get-ChildItem (Join-Path $Root 'tests\language-test') -Filter '*.aura' -File | Sort-Object Name
if ($Only -ne '') {
  $keys = $Only -split ','
  $tests = $tests | Where-Object { $n = $_.BaseName; ($keys | Where-Object { $n -like ('*' + $_ + '*') }).Count -gt 0 }
}
Write-Host ("[lt3] tests=" + $tests.Count + "  work=" + $Work) -ForegroundColor Cyan
Write-Host ("[lt3] seed=" + $Seed)
Write-Host ("[lt3] driver=" + $Driver + "  jit=" + $JitHst)

$rows = New-Object System.Collections.Generic.List[object]
$idx = 0
foreach ($t in $tests) {
  $idx++
  $stem = $t.BaseName
  $src  = $t.FullName
  $ms   = $TimeoutSecs * 1000

  # ── A: Aura VM（种子 VM 解释）──
  $rVm = Run-Probe $Seed @('run', ('"' + $src + '"')) $null $ms
  $vmOk = ($rVm.Exit -eq 0) -and (-not $rVm.Tmo)

  # ── B: Photon JIT（.auc → 原生宿主）──
  $auc = Join-Path $Work ($stem + '.auc')
  if (Test-Path $auc) { Remove-Item $auc -Force -ErrorAction SilentlyContinue }
  $rB = Run-Probe $Seed @('build', ('"' + $src + '"'), '--output', ('"' + $auc + '"')) $null $ms
  $jitOk = $false; $jitNote = ''
  if ((Test-Path $auc) -and $rB.Exit -eq 0) {
    $rJ = Run-Probe $JitHst @() @{ 'AURA_VM_AUC' = $auc } $ms
    $jitOk = ($rJ.Exit -eq 0) -and (-not $rJ.Tmo)
    if (-not $jitOk) {
      $m = [regex]::Match(($rJ.Out + $rJ.Err), 'VM-ERROR=([^\r\n]+)')
      if ($m.Success) { $jitNote = $m.Groups[1].Value } else { $jitNote = 'exit=' + $rJ.Exit }
      if ($jitNote.Length -gt 70) { $jitNote = $jitNote.Substring(0, 70) }
    }
    $jitLast = LastLine $rJ.Out
  } else {
    $jitNote = 'auc-build-fail'
    $jitLast = ''
  }

  # ── C: Photon AOT（HAT 驱动 → 原生 exe）──
  $aotDir = Join-Path $Work ('aot_' + $stem)
  New-Item -ItemType Directory -Force -Path $aotDir | Out-Null
  $envMap = @{
    'AURA_HAT_AURA'   = $src
    'AURA_HAT_SRC'    = (Join-Path $aotDir ($stem + '.hat'))
    'AURA_HAT_OUT'    = $aotDir
    'AURA_HAT_MODULE' = $stem
  }
  $rC = Run-Probe $Driver @() $envMap $ms
  $exe = Join-Path $aotDir ($stem + '.exe')
  $aotOk = $false; $aotNote = ''
  if (Test-Path $exe) {
    $rX = Run-Probe $exe @() $null $ms
    $aotOk = ($rX.Exit -eq 0) -and (-not $rX.Tmo)
    if (-not $aotOk) { $aotNote = 'run-exit=' + $rX.Exit }
    $aotLast = LastLine $rX.Out
  } else {
    $m = [regex]::Match($rC.Out, '===ERR===([^\r\n]+)')
    if ($m.Success) { $aotNote = 'compile: ' + $m.Groups[1].Value } else { $aotNote = 'no-exe (driver exit=' + $rC.Exit + ')' }
    if ($aotNote.Length -gt 70) { $aotNote = $aotNote.Substring(0, 70) }
    $aotLast = ''
  }

  $vmLast = LastLine $rVm.Out
  $diff = ''
  if ($vmLast -ne '' -and $aotLast -ne '' -and $vmLast -ne $aotLast) { $diff = 'VM/AOT' }
  if ($vmLast -ne '' -and $jitLast -ne '' -and $vmLast -ne $jitLast) {
    if ($diff -eq '') { $diff = 'VM/JIT' } else { $diff = $diff + '+JIT' }
  }
  $tag = 'PASS'
  if (-not ($vmOk -and $jitOk -and $aotOk)) {
    $tag = ''
    if (-not $vmOk)  { $tag = $tag + 'vm ' }
    if (-not $jitOk) { $tag = $tag + 'jit ' }
    if (-not $aotOk) { $tag = $tag + 'aot ' }
    $tag = 'FAIL(' + $tag.Trim() + ')'
  }

  $rows.Add([pscustomobject]@{
    Test = $stem; VM = $vmOk; JIT = $jitOk; AOT = $aotOk; Verdict = $tag
    Diff = $diff; Note = (($jitNote + ' ' + $aotNote).Trim()); Last = $vmLast
  })
  $color = if ($tag -eq 'PASS') { 'Gray' } else { 'Yellow' }
  Write-Host ("[" + $idx + "/" + $tests.Count + "] " + $tag.PadRight(16) + " " + $stem.PadRight(28) +
              " diff=" + $diff.PadRight(8) + " " + (($jitNote + ' ' + $aotNote).Trim())) -ForegroundColor $color
}

$csv = Join-Path $Work 'results.csv'
$rows | Export-Csv $csv -NoTypeInformation
$sum = @{}
$sum['VM']  = ($rows | Where-Object { $_.VM }).Count
$sum['JIT'] = ($rows | Where-Object { $_.JIT }).Count
$sum['AOT'] = ($rows | Where-Object { $_.AOT }).Count
$all3 = ($rows | Where-Object { $_.VM -and $_.JIT -and $_.AOT }).Count
Write-Host ''
Write-Host ("=== 汇总 (" + $rows.Count + " 用例) ===") -ForegroundColor Cyan
Write-Host ("  Aura VM      PASS " + $sum['VM']  + " / " + $rows.Count)
Write-Host ("  Photon JIT   PASS " + $sum['JIT'] + " / " + $rows.Count)
Write-Host ("  Photon AOT   PASS " + $sum['AOT'] + " / " + $rows.Count)
Write-Host ("  三模式全过   " + $all3 + " / " + $rows.Count)
$hard = $rows | Where-Object { -not ($_.VM -and $_.JIT -and $_.AOT) }
if ($hard.Count -gt 0) {
  Write-Host ''
  Write-Host "--- 未全过的用例 ---" -ForegroundColor Yellow
  foreach ($r in $hard) {
    Write-Host ("  " + $r.Test.PadRight(28) + " vm=" + $r.VM + " jit=" + $r.JIT + " aot=" + $r.AOT + "  " + $r.Note)
  }
}
$diffs = $rows | Where-Object { $_.Diff -ne '' }
if ($diffs.Count -gt 0) {
  Write-Host ''
  Write-Host "--- 输出末行不一致（triage 参考，不作判据）---" -ForegroundColor DarkYellow
  foreach ($r in $diffs) { Write-Host ("  " + $r.Test.PadRight(28) + " " + $r.Diff + "  VM末行=" + $r.Last) }
}
Write-Host ''
Write-Host ("CSV: " + $csv)
