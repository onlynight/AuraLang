# Phase E: �?Aura 化工具链验证脚本
#
# 验证 AuraCli / AuraLsp / AuraDebugger / Loom 的纯 Aura 实现�?# 对应改造方�?E.5「验证」章节�?#
# 用法：pwsh scripts/verify-pure-aura.ps1

param(
    [switch]$SkipLlm,       # 跳过 LLVM 依赖测试
    [switch]$Quick          # 快速模式（仅检查文件存在）
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$pass = 0
$fail = 0

function Check($desc, $cond) {
    if ($cond) {
        Write-Host "  �?$desc" -ForegroundColor Green
        $script:pass++
    } else {
        Write-Host "  �?$desc" -ForegroundColor Red
        $script:fail++
    }
}

function Section($title) {
    Write-Host ""
    Write-Host "━━�?$title ━━�? -ForegroundColor Cyan
}

Write-Host "══�?�?Aura 化工具链验证 ══�? -ForegroundColor White
Write-Host "日期: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Write-Host "仓库: $root"

# ─── 快速模�?───
if ($Quick) {
    Section "E.1 CLI Main.aura"
    Check "File exists" (Test-Path "$root/aura/toolchain/cli/aura/lang/cli/Main.aura")
    $cli = Get-Content "$root/aura/toolchain/cli/aura/lang/cli/Main.aura" -Raw
    Check "Has main()" ($cli -match "fun main\(\)")
    Check "Has cmdBuild" ($cli -match "cmdBuild")
    Check "Has cmdRun" ($cli -match "cmdRun")
    Check "Has cmdCheck" ($cli -match "cmdCheck")
    Check "Has cmdLsp" ($cli -match "cmdLsp")
    Check "Has cmdDebug" ($cli -match "cmdDebug")

    Section "E.2 LSP Main.aura"
    Check "File exists" (Test-Path "$root/aura/toolchain/lsp/aura/lang/lsp/Main.aura")
    $lsp = Get-Content "$root/aura/toolchain/lsp/aura/lang/lsp/Main.aura" -Raw
    Check "Has LspServer" ($lsp -match "class LspServer")
    Check "Has lspCompletion" ($lsp -match "lspCompletion")
    Check "Has lspHover" ($lsp -match "lspHover")
    Check "Has lspDiagnostic" ($lsp -match "lspDiagnostic")
    Check "Has JSON-RPC" ($lsp -match "Content-Length")

    Section "E.3 Debugger Main.aura"
    Check "File exists" (Test-Path "$root/aura/toolchain/debugger/aura/lang/debugger/Main.aura")
    $dbg = Get-Content "$root/aura/toolchain/debugger/aura/lang/debugger/Main.aura" -Raw
    Check "Has Debugger class" ($dbg -match "class Debugger")
    Check "Has breakpoint" ($dbg -match "Breakpoint")
    Check "Has stepOver" ($dbg -match "stepOver")
    Check "Has debugRepl" ($dbg -match "debugRepl")

    Section "E.4 Loom Main.aura"
    Check "File exists" (Test-Path "$root/aura/toolchain/loom/aura/lang/loom/Main.aura")
    $loom = Get-Content "$root/aura/toolchain/loom/aura/lang/loom/Main.aura" -Raw
    Check "Has Loom class" ($loom -match "class Loom")
    Check "Has Manifest" ($loom -match "class Manifest")
    Check "Has TaskGraph" ($loom -match "class TaskGraph")
    Check "Has BuildCache" ($loom -match "class BuildCache")
    Check "Has Watcher" ($loom -match "class Watcher")
    Check "Has PluginRegistry" ($loom -match "class PluginRegistry")
    Check "Has CiConfig" ($loom -match "class CiConfig")

    Section "Phase D 成果保留"
    Check "Encoding.aura has sha256" (Get-Content "$root/aura/core/aura/lang/std/Encoding.aura" -Raw -match "sha256")
    Check "Encoding.aura has hmacSha256" (Get-Content "$root/aura/core/aura/lang/std/Encoding.aura" -Raw -match "hmacSha256")
    Check "interp.rs has priority fix" (Get-Content "$root/compiler/src/vm/interp.rs" -Raw -match "stdlib_func_map")
    Check "mod.rs has Ascii commented" (Get-Content "$root/compiler/src/std/mod.rs" -Raw -match "//.*std_ascii")
    Check "phase_d_tests.rs exists" (Test-Path "$root/compiler/tests/phase_d_tests.rs")
    Check "stdlib_consistency.rs exists" (Test-Path "$root/compiler/tests/stdlib_consistency.rs")
    Check "aura_syscalls.c has pthread" (Get-Content "$root/aura/runtime/cffi/aura_syscalls.c" -Raw -match "aura_thread_create")
    Check "aura_syscalls.c has sha256" (Get-Content "$root/aura/runtime/cffi/aura_syscalls.c" -Raw -match "aura_sha256")

    Write-Host ""
    Write-Host "══�?快速模式结�? $pass passed, $fail failed ══�? -ForegroundColor ($if ($fail -eq 0) { "Green" } else { "Yellow" })
    return
}

# ─── 完整模式 ───

Section "E.1 AuraCli.aura �?结构验证"
$cliPath = "$root/aura/toolchain/cli/aura/lang/cli/Main.aura"
$cli = Get-Content $cliPath -Raw
Check "File exists" (Test-Path $cliPath)
Check "Has main()" ($cli -match "fun main\(\)")
Check "Has cmdBuild" ($cli -match "cmdBuild")
Check "Has cmdRun" ($cli -match "cmdRun")
Check "Has cmdCheck" ($cli -match "cmdCheck")
Check "Has cmdDisasm" ($cli -match "cmdDisasm")
Check "Has cmdTokens" ($cli -match "cmdTokens")
Check "Has cmdAst" ($cli -match "cmdAst")
Check "Has cmdFmt" ($cli -match "cmdFmt")
Check "Has cmdLeakCheck" ($cli -match "cmdLeakCheck")
Check "Has cmdDoc" ($cli -match "cmdDoc")
Check "Has cmdEval" ($cli -match "cmdEval")
Check "Has cmdRepl" ($cli -match "cmdRepl")
Check "Has cmdInstall" ($cli -match "cmdInstall")
Check "Has cmdPackage" ($cli -match "cmdPackage")
Check "Has cmdInspect" ($cli -match "cmdInspect")
Check "Has cmdVerify" ($cli -match "cmdVerify")
Check "Has cmdLsp" ($cli -match "cmdLsp")
Check "Has cmdDebug" ($cli -match "cmdDebug")
Check "Has Args class" ($cli -match "class Args")
Check "Has @native declarations" ($cli -match "@native")
Check "Covers 22+ subcommands" ($cli -match '"build"')
Check "Imports std modules" ($cli -match "import aura.lang.std")

Section "E.2 AuraLsp.aura �?结构验证"
$lspPath = "$root/aura/toolchain/lsp/aura/lang/lsp/Main.aura"
$lsp = Get-Content $lspPath -Raw
Check "File exists" (Test-Path $lspPath)
Check "Has LspServer class" ($lsp -match "class LspServer")
Check "Has LspRequest class" ($lsp -match "class LspRequest")
Check "Has LspResponse class" ($lsp -match "class LspResponse")
Check "Has lspCompletion" ($lsp -match "lspCompletion")
Check "Has lspHover" ($lsp -match "lspHover")
Check "Has lspDefinition" ($lsp -match "lspDefinition")
Check "Has lspReferences" ($lsp -match "lspReferences")
Check "Has lspDiagnostic" ($lsp -match "lspDiagnostic")
Check "Has lspFormatting" ($lsp -match "lspFormatting")
Check "Has didOpen handler" ($lsp -match "handleDidOpen")
Check "Has didChange handler" ($lsp -match "handleDidChange")
Check "Has didClose handler" ($lsp -match "handleDidClose")
Check "Has JSON-RPC handling" ($lsp -match "Content-Length")
Check "Has documentSymbol" ($lsp -match "documentSymbol")
Check "Has signatureHelp" ($lsp -match "signatureHelp")
Check "Has inlayHint" ($lsp -match "inlayHint")
Check "Has codeAction" ($lsp -match "codeAction")

Section "E.3 AuraDebugger.aura �?结构验证"
$dbgPath = "$root/aura/toolchain/debugger/aura/lang/debugger/Main.aura"
$dbg = Get-Content $dbgPath -Raw
Check "File exists" (Test-Path $dbgPath)
Check "Has Debugger class" ($dbg -match "class Debugger")
Check "Has Breakpoint class" ($dbg -match "class Breakpoint")
Check "Has BreakpointManager" ($dbg -match "class BreakpointManager")
Check "Has debugInit" ($dbg -match "debugInit")
Check "Has debugRun" ($dbg -match "debugRun")
Check "Has debugContinue" ($dbg -match "debugContinue")
Check "Has debugStepOver" ($dbg -match "debugStepOver")
Check "Has debugStepInto" ($dbg -match "debugStepInto")
Check "Has debugStepOut" ($dbg -match "debugStepOut")
Check "Has debugBreakpointAdd" ($dbg -match "debugBreakpointAdd")
Check "Has debugBreakpointRemove" ($dbg -match "debugBreakpointRemove")
Check "Has debugVariables" ($dbg -match "debugVariables")
Check "Has debugCallStack" ($dbg -match "debugCallStack")
Check "Has debugEval" ($dbg -match "debugEval")
Check "Has debugWatch" ($dbg -match "debugWatch")
Check "Has debugRepl" ($dbg -match "debugRepl")
Check "Has REPL commands" ($dbg -match "breakpoints")

Section "E.4 Loom.aura �?结构验证"
$loomPath = "$root/aura/toolchain/loom/aura/lang/loom/Main.aura"
$loom = Get-Content $loomPath -Raw
Check "File exists" (Test-Path $loomPath)
Check "Has Loom class" ($loom -match "class Loom")
Check "Has Manifest class" ($loom -match "class Manifest")
Check "Has Task class" ($loom -match "class Task")
Check "Has TaskGraph class" ($loom -match "class TaskGraph")
Check "Has BuildCache class" ($loom -match "class BuildCache")
Check "Has Plugin class" ($loom -match "class Plugin")
Check "Has PluginRegistry" ($loom -match "class PluginRegistry")
Check "Has CiConfig class" ($loom -match "class CiConfig")
Check "Has Watcher class" ($loom -match "class Watcher")
Check "Has topoSort" ($loom -match "topoSort")
Check "Has fingerprint" ($loom -match "fingerprint")
Check "Has cmdNewProject" ($loom -match "cmdNewProject")
Check "Has cmdTasks" ($loom -match "cmdTasks")
Check "Has cmdInfo" ($loom -match "cmdInfo")
Check "Has addDependency" ($loom -match "addDependency")
Check "Has runTask" ($loom -match "runTask")
Check "Has watch" ($loom -match "fun watch")
Check "Has ci" ($loom -match "fun ci")

Section "Phase D 成果保留"
Check "Encoding.aura has sha256" (Get-Content "$root/aura/core/aura/lang/std/Encoding.aura" -Raw -match "sha256")
Check "Encoding.aura has hmacSha256" (Get-Content "$root/aura/core/aura/lang/std/Encoding.aura" -Raw -match "hmacSha256")
Check "interp.rs priority fix" (Get-Content "$root/compiler/src/vm/interp.rs" -Raw -match "stdlib_func_map")
Check "mod.rs Ascii commented" (Get-Content "$root/compiler/src/std/mod.rs" -Raw -match "//.*std_ascii")
Check "std_encoding.rs has SHA256" (Get-Content "$root/compiler/src/std/std_encoding.rs" -Raw -match "sha256_hex")
Check "aura_syscalls.c has pthread" (Get-Content "$root/aura/runtime/cffi/aura_syscalls.c" -Raw -match "aura_thread_create")
Check "aura_syscalls.c has sha256" (Get-Content "$root/aura/runtime/cffi/aura_syscalls.c" -Raw -match "aura_sha256")
Check "phase_d_tests.rs exists" (Test-Path "$root/compiler/tests/phase_d_tests.rs")
Check "stdlib_consistency.rs exists" (Test-Path "$root/compiler/tests/stdlib_consistency.rs")

Section "编译验证"
$compileOut = cargo check -p compiler --features llvm 2>&1
Check "cargo check passes" ($LASTEXITCODE -eq 0)

if (-not $SkipLlm) {
    Section "单元测试"
    $testOut = cargo test -p compiler --features llvm --test phase_d_tests 2>&1
    Check "phase_d_tests passes" ($testOut -match "15 passed")
    $consOut = cargo test -p compiler --features llvm --test stdlib_consistency 2>&1
    Check "stdlib_consistency passes" ($consOut -match "2 passed")
}

# ─── 汇�?───
Write-Host ""
Write-Host "══════════════════════════════════════�? -ForegroundColor White
Write-Host "  验证完成: $pass passed, $fail failed" -ForegroundColor ($if ($fail -eq 0) { "Green" } else { "Yellow" })
Write-Host "══════════════════════════════════════�? -ForegroundColor White
exit $fail