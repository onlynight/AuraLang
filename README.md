# Aura Programming Language

> A system-level scripting language for NovaOS ‚Ä?Kotlin-style syntax, Rust implementation, AOT + JIT hybrid compilation, zero-cost FFI, ARC memory management, plus a **direct-to-COFF native backend (HAT / Photon)** that emits a native executable without LLVM IR.
>
> **‰∏≠ÊñáÊñáÊ°£** ‚Ü?[README.zh-CN.md](README.zh-CN.md)

Design docs: [Á≥ªÁªü-AuraÊûÑÂª∫Á≥ªÁªüËÆæËÆ°.md](docs/Á≥ªÁªü-AuraÊûÑÂª∫Á≥ªÁªüËÆæËÆ°.md) ¬∑ [ÁºñËØë-ÁºñËØëÂô®ÂêéÁ´ØÊñπÊ°?md](docs/ÁºñËØë-ÁºñËØëÂô®ÂêéÁ´ØÊñπÊ°?md) ¬∑ [ÈõÜÊàê-Aura-DSH-Integration-Plan.md](docs/ÈõÜÊàê-Aura-DSH-Integration-Plan.md)

---

## Current Status

The **full toolchain** is implemented and functional ‚Ä?from lexer to AOT-compiled native binaries, with VM, JIT, debugger, LSP, and a complete build system.

| Phase | Component | Status |
|-------|-----------|--------|
| P0 | Infrastructure (workspace, SourceMap, diagnostics, CI) | ‚ú?|
| P1 | Lexer | ‚ú?|
| P2 | Parser + AST | ‚ú?|
| P3 | Semantic analysis (types, null-safety, diagnostics) | ‚ú?|
| P4 | Bytecode compiler (HIR ‚Ü?MIR ‚Ü?bytecode) | ‚ú?|
| P5 | VM (stack-based interpreter) + JIT (Cranelift) | ‚ú?|
| P6 | AOT compiler (LLVM IR ‚Ü?native) | ‚ú?|
| P7 | Memory management (ARC, leak detection) | ‚ú?|
| P8 | FFI (C interop, extern "c", dynamic loading) | ‚ú?|
| P9 | Standard library (19 modules, 320+ functions) | ‚ú?|
| P10 | Concurrency (Actors, Channels, Coroutines, ThreadPool) | ‚ú?|
| P11 | Package management (aura.toml, .auz packages) | ‚ú?|
| P12 | IPC (TCP channels, actor processes) | ‚ú?|
| P13 | Toolchain (LSP, formatter, VS Code extension) | ‚ú?|
| P14 | Examples & integration tests | ‚ú?|
| P15 | Debugger (VM / JIT / AOT modes) | ‚ú?|
| P16 | Build system (loom) | ‚ú?|
| P17 | HAT / Photon native backend (LLVM-IR-free, direct COFF) | ‚ú?‚Ä?see [below](#native-backend--hat--photon) |

---

## Native Backend ‚Ä?HAT / Photon

A third, **LLVM-IR-free** compilation path. It does not generate `.ll`; the backend lowers to a
register-allocated linear IR, emits x86-64 machine code by hand, assembles a COFF object, and hands
it to `lld-link` for the final image.

```
Source (.aura)
   ‚î?   ‚ñ?AotModuleLinker ‚îÄ‚îÄ‚ñ?HirProgram          (module merge, std prelude inlining)
   ‚î?   ‚ñ?SsaBuilder ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚ñ?SSA MIR            (SSA + phi reconstruction)
   ‚î?   ‚ñ?HatSerializer ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚ñ?.hat              (text IR, round-trippable, human-readable)
   ‚î?   ‚ñ? HATParser
   ‚îú‚îÄ Phase B: SSA MIR ‚Ü?LIR                 (Lowering.aura)
   ‚îú‚îÄ Phase C: LIR ‚Ü?Machine DAG             (InstructionSelection.aura)
   ‚îú‚îÄ Phase D: Register allocation + peephole (RegisterAllocator.aura, PeepholeOptimizer.aura)
   ‚îî‚îÄ Phase E: X86 encoding ‚Ü?COFF ‚Ü?link    (X86Encoder.aura, PhotonObjectWriter.aura, PhotonSystemLinker.aura)
                              ‚î?                              ‚ñ?                       native .exe         (COFF 64-bit, x86_64-pc-windows-msvc)
```

The HAT IR is a first-class text representation (`aura/lang/compiler/hir/hat/`), so a program can be
compiled to `.hat`, inspected or edited, and recompiled ‚Ä?HAT is the true intermediate artifact of
this path, not a debug dump.

| Item | State |
|------|-------|
| SSA builder + phi reconstruction | ‚ú?|
| HAT serializer / parser (round-trip) | ‚ú?|
| LIR lowering | ‚ú?|
| Machine DAG + instruction selection | ‚ú?|
| Register allocation (x86-64) | ‚ú?|
| Peephole + liveness | ‚ú?|
| X86-64 encoder + relocation emission | ‚ú?|
| COFF object writer | ‚ú?|
| System linker (`lld-link`, `/NODEFAULTLIB`) | ‚ú?|
| Syscall emitter (`NtCreateFile` / `NtWriteFile` / `NtReadFile` / `NtMapViewOfFile`) | ‚ú?|
| Runtime (`println`, `strcat`, `toInt`, `streq`, `heapArena`, `__list_*`, syscalls) | ‚ú?(subset) |

### Differential test suites

Every suite case is compiled **and executed twice** ‚Ä?once through HAT, once through the plain VM ‚Ä?and the outputs are compared. The only reference is VM stdout; the HAT chain never produces or reads
a `.phir`.

| Suite script | Front end | Back end | Result |
|--------------|-----------|----------|--------|
| `scripts/photon-hat-native-suite.ps1` (default) | native self-bootstrapping compiler (`build/hat-native/PhotonHatCompile.exe`) | VM-side HAT consumer (`PhotonHatBuild.aura`) | **15/15** |
| `scripts/photon-hat-native-suite.ps1 -NativeAll` | same | native Photon backend | **13/15** ‚Ä?`05_functions`, `02_fibonacci` lose a user-function call's return value (`add(3,4) = 0`) |
| `scripts/photon-hat-suite.ps1` | seed VM (PHIR ‚Ü?HIR ‚Ü?SSA ‚Ü?`.hat`) | HAT backend | **15/15** |

Cases:

```
P1  hello / vars / arithmetic / control flow / functions          5 cases
P2  nested loop / fibonacci / array ops / string ops               4 cases
P3  syscall: exit, NtCreateFile, NtWriteFile, mmap, read, write   6 cases
```

`tests/photon/P1..P4` hold the end-to-end differential cases; `tests/photon/S1..S4` hold the backend
unit/integration tests (x86 encoder, relocations, instruction selection, register allocator,
emitter, object writer, pipeline integration).

> `-OutRoot` must be a **relative** path ‚Ä?the scripts join it against the repository root.

### Self-bootstrap of the Aura compiler (in progress)

`scripts/photon-hat-bootstrap.ps1` drives the HAT chain against `aura/compiler/aura/lang/compiler/Main.aura`
(the Aura-written compiler itself). Current state:

```
[hat-front] modules=111 hirNodes=155887
[hat-front] ssa functions=18 values=239 blocks=51
  .hat                 8,994 chars      (HatParser round-trip confirmed)
  LIR functions=18     DAG nodes=152 instrs=234
  spillSlots=7         machine bytes=1,483     COFF object=2,919 B
  wall clock 79 s      peak working set 324.8 MB      (lld-link peak 17.8 MB)
  lld-link ‚Ü?9 unresolved symbols
```

Everything up to the link step now runs cleanly. The remaining 9 symbols fall in three buckets:

| Bucket | Symbols | Effort |
|--------|---------|--------|
| Missing runtime symbol | `charCodeAt` | small ‚Ä?add a `mapStdlibFuncName` entry + export `movzx eax, byte ptr [rcx+rdx]` |
| `object` methods not lowered | `cliUsage`, `runSelfTest`, `runCli` | medium ‚Ä?`lowerTypeDecl` puts `object` methods inside a `HirBlock` under `HirObject`; `SsaBuilder` must descend `HirObject ‚Ü?HirBlock ‚Ü?HirFunction` (as `Emit.aura` already does by scanning the whole arena by kind) |
| `class` instances / vtables | `VmRunner`, `loadAucAndRun`, `getReturnValue`, `getError`, `getInstructionCount` | large ‚Ä?the HAT runtime has no object header, no vtable, no `HashMap`/`ArrayList` semantics |

Full breakdown: [`build/hat-bootstrap/REPORT-HAT-bootstrap.md`](build/hat-bootstrap/REPORT-HAT-bootstrap.md).

> **Root cause that unblocked this** (fixed): `SsaBuilder.changedVarsCsv` scanned a string backward
> with `while (start >= 0 && after.charCodeAt(start) != 10)`. The AOT backend lowers
> `String.charCodeAt(i)` to an **unchecked inline load** (`getelementptr i8, i8*, i64 i` + `load i8`,
> `aot/Emit.aura:6922`), so it bypasses the `index < 0` guard in `core/aura/lang/String.aura`.
> When `start` reached -1 the scan read before the buffer: either garbage (scan never terminated,
> looked like a hang) or `0xC0000005`. The scan now only ever reads indices `>= 0`, structurally.
> **Rule for this backend:** never rely on `&&` short-circuit to protect a `charCodeAt` call ‚Ä?make
> the invalid-index path structurally unreachable.

---

## Pure Aura Compiler Migration (Á∫?Aura ÂåñËøÅÁß?

In parallel with the Rust compiler, a **compiler written entirely in Aura** is built under `aura/compiler/aura/lang/compiler/`. The Rust compiler (`rust/compiler/`) is preserved unmodified as the fallback and reference implementation.

### Self-Bootstrap Process (Ëá™‰∏æÊµÅÁ®ã)

The Aura compiler is bootstrapped from a pre-built seed binary:

```
aura/seed/aura.exe          <- Rust bootstrap (seed, no rebuild needed)
        ‚î?        ‚ñ? aura build aura/compiler/.../Main.aura
        ‚î?        ‚ñ?build/auc/compiler/aura-compiler.auc   <- Aura-written compiler bytecode
        ‚î?        ‚ñ? aura run aura-compiler.auc <file.aura>
        ‚î?        ‚ñ?build/output/<file>.exe               <- AOT-compiled native executable
```

A second, independent bootstrap path drives the HAT chain directly and skips the VM entirely:

```
rust/target/release/aura.exe
        ‚î? aura build --aot aura/photon/.../photon/PhotonHatCompile.aura
        ‚ñ?build/hat-native/PhotonHatCompile.exe        <- AOT-compiled HAT chain driver
        ‚î? <driver>  Main.aura
        ‚ñ?Main.aura ‚îÄ‚îÄ‚ñ?HirProgram ‚îÄ‚îÄ‚ñ?SSA MIR ‚îÄ‚îÄ‚ñ?.hat ‚îÄ‚îÄ‚ñ?COFF ‚îÄ‚îÄ‚ñ?lld-link ‚îÄ‚îÄ‚ñ?.exe
```

**Bootstrap resolution order** (in `build-aura-compiler.ps1`):

1. `rust/target/release/aura.exe` ‚Ä?cargo-built, preferred
2. `rust/target/debug/aura.exe`
3. `aura/seed/aura.exe` ‚Ä?frozen git-LFS seed, only with `-FrozenSeed` or if no target build exists

**Rebuilding the seed** (when `rust/compiler` is updated):

```powershell
cd rust
cargo build --release -p cli --features llvm
copy target\release\aura.exe ..\aura\seed\aura.exe

# or, from the repository root:
scripts\build-aura-compiler.ps1 -RebuildSeed
```

| Phase | Component | Status |
|-------|-----------|--------|
| P0 | Infrastructure (TestRunner, Main skeleton) | ‚ú?|
| P1 | Lexer + Parser + AST | ‚ú?|
| P2 | Sema (Type, SymbolTable, TypeInfo, TypeChecker) + HIR (Lower, Desugar, Mono, Inline, Fold) | ‚ú?|
| P3 | MIR (IR types, HIR‚ÜíMIR lowering, DCE/CSE/const-prop optimizations) | ‚ú?|
| P4 | Bytecode Codegen (MIR‚Ü?auc) + VM interpreter | ‚ú?|
| P5 | VM enhancements (closures, tail-call, stack frames) | ‚ú?|
| P6 | AOT backend (LLVM IR generation) | ‚ú?|
| P6.5 | AOT hardening: classes / std signature table / collections / multi-module link | üöß In progress |
| P7 | JIT (Cranelift) | ‚ú?|
| P8 | Core & standard library in Aura | ‚ú?|
| P9 | End-to-end compile pipeline (VM / JIT / AOT) | ‚ú?|
| P10 | MIR ‚Ü?SSA (SsaBuilder, SsaMir, TypeRegistry, Linearizer) | ‚ú?|
| P11 | HAT IR (text format, `HatSerializer` / `HatParser`) | ‚ú?|
| P12 | HAT / Photon native backend (LIR ‚Ü?DAG ‚Ü?RegAlloc ‚Ü?X86 ‚Ü?COFF ‚Ü?link) | ‚ú?(P1‚ÄìP3 differential 15/15) |
| P13 | HAT self-bootstrap of the Aura compiler | üöß link stage ‚Ä?see [Native Backend](#native-backend--hat--photon) |

### Test Results

```
tests/phase0_tests.aura           ‚Ü?RESULT: PASS
tests/phase1_lexer_tests.aura     ‚Ü?RESULT: PASS
tests/phase2_sema_hir_tests.aura  ‚Ü?RESULT: PASS  (20 test groups, 0 failures)
tests/phase3_mir_tests.aura       ‚Ü?RESULT: PASS  (11 test groups, 0 failures)
tests/phase5_vm_tests.aura        ‚Ü?RESULT: PASS
tests/phase6_aot_tests.aura       ‚Ü?RESULT: PASS
tests/phase6_5_aot_tests.aura     ‚Ü?RESULT: PASS
tests/phase7_jit_tests.aura       ‚Ü?RESULT: PASS
tests/phase8_stdlib_tests.aura    ‚Ü?RESULT: PASS
tests/phase9_compiler_tests.aura  ‚Ü?RESULT: PASS

scripts/photon-hat-native-suite.ps1  P1+P2+P3  ‚Ü?PASS=15 FAIL=0
scripts/photon-hat-suite.ps1         P1+P2+P3  ‚Ü?PASS=15 FAIL=0
```

Key modules under `aura/compiler/aura/lang/compiler/`:

```
test/        TestRunner                        ‚Ä?Test framework (self-contained)
lexer/      Span, Token, Lexer                 ‚Ä?Tokenizer with string interpolation
parser/     Parser                             ‚Ä?Recursive-descent + Pratt parser
ast/        Ast                                ‚Ä?Flat arena AST (kinds/texts/tys/spans/kids)
sema/       Type, SymbolTable, TypeInfo, TypeChecker ‚Ä?Type system & semantic analysis
hir/        Hir, Desugar, Mono, Inline, Fold, HirSerializer ‚Ä?HIR lowering & optimization
hir/hat/    HatSerializer, HatParser           ‚Ä?HAT text IR (round-trippable)
mir/        Mir, MirLower, MirOpt              ‚Ä?MIR IR, HIR‚ÜíMIR lowering, optimization
mir/        SsaBuilder, SsaMir, TypeRegistry, Linearizer ‚Ä?SSA builder + SSA MIR
codegen/    Codegen                            ‚Ä?MIR ‚Ü?bytecode emission
vm/         VmRunner, Closures, TailCall, FrameManager, Frames, Opcodes ‚Ä?VM interpreter
aot/        Emit, StdSigs, Runtime, ModuleLink ‚Ä?LLVM IR emitter, std signature table, multi-module linker
jit/        JitCore, JitState, JitOpt, JitLower, DispatchTable ‚Ä?JIT backend
gc/         Gc, MarkSweep, Incremental, Concurrent      ‚Ä?GC implementations
memory/     Memory, MemoryPool, Arc                    ‚Ä?Memory management & ARC
photon/  PhotonPipeline, Lowering, InstructionSelection, RegisterAllocator,
                 PeepholeOptimizer, MachineDag, Lir, X86Emitter, PhotonObjectWriter,
                 PhotonSystemLinker, PhotonRuntime, SyscallEmitter, JitBackend,
                 PhotonHatCompile (HAT-chain driver)
errors/     CompileError                       ‚Ä?Diagnostic model
serialize/  AucSerializer, AucLoader, PlatformFileIO, WinFileIO ‚Ä?.auc format
linker/     Linker                             ‚Ä?Module linker
signature/  Signature                          ‚Ä?Signature table
Main.aura   ‚Ä?Compiler entry skeleton
```

---

## Repository Structure

```text
AuraLang/
‚îú‚îÄ‚îÄ Cargo.toml              Workspace root
‚îú‚îÄ‚îÄ LICENSE                 Apache-2.0
‚îú‚îÄ‚îÄ README.md               ‚Ü?You are here
‚îú‚îÄ‚îÄ README.zh-CN.md         ‰∏≠ÊñáÊñáÊ°£
‚îú‚îÄ‚îÄ aura.toml               Loom package manifest
‚î?‚îú‚îÄ‚îÄ aura/                   Aura language sources (self-bootstrapping)
‚î?  ‚îú‚îÄ‚îÄ core/               Core language types (Any, Int, String, List, Map, ...)
‚î?  ‚îú‚îÄ‚îÄ compiler/           Compiler written in Aura
‚î?  ‚î?  ‚îî‚îÄ‚îÄ aura/lang/compiler/
‚î?  ‚î?      ‚îú‚îÄ‚îÄ lexer/ parser/ ast/ sema/ errors/ sourcemap/   Front end
‚î?  ‚î?      ‚îú‚îÄ‚îÄ hir/                        HIR + optimizer passes
‚î?  ‚î?      ‚î?  ‚îî‚îÄ‚îÄ hat/                    HAT text IR (serializer + parser)
‚î?  ‚î?      ‚îú‚îÄ‚îÄ mir/                        MIR + SSA (SsaBuilder, SsaMir, Linearizer)
‚î?  ‚î?      ‚îú‚îÄ‚îÄ codegen/ vm/                Bytecode emission + VM interpreter
‚î?  ‚î?      ‚îú‚îÄ‚îÄ aot/ jit/                   LLVM IR AOT + Cranelift JIT
‚î?  ‚î?      ‚îú‚îÄ‚îÄ photon/             HAT native backend (LIR ‚Ü?COFF ‚Ü?exe)
‚î?  ‚î?      ‚îú‚îÄ‚îÄ gc/ memory/ runtime/        GC, ARC, coroutine runtime
‚î?  ‚î?      ‚îú‚îÄ‚îÄ serialize/ linker/ signature/ auz/ package/
‚î?  ‚î?      ‚îî‚îÄ‚îÄ Main.aura                   Compiler entry
‚î?  ‚îú‚îÄ‚îÄ runtime/            Runtime support sources
‚î?  ‚îú‚îÄ‚îÄ toolchain/          LSP, debugger, docgen, cli
‚î?  ‚îî‚îÄ‚îÄ seed/
‚î?      ‚îî‚îÄ‚îÄ aura.exe        Rust bootstrap seed (pre-built, no rebuild needed)
‚î?‚îú‚îÄ‚îÄ rust/                   Rust toolchain (build from here)
‚î?  ‚îú‚îÄ‚îÄ compiler/           Compiler crate (lexer, sema, codegen, vm, std, lsp, ...)
‚î?  ‚îú‚îÄ‚îÄ cli/                Binaries: `aura`, `aura-lsp`, `aura-debug`
‚î?  ‚îú‚îÄ‚îÄ loom/               Gradle/Bazel-style build system
‚î?  ‚îî‚îÄ‚îÄ target/             Rust build artifacts (generated)
‚î?‚îú‚îÄ‚îÄ book/                   User documentation (tutorials, API, migration)
‚îú‚îÄ‚îÄ docs/                   Technical design documents (Á≥ªÁªü/ÁºñËØë/ËØ≠Ë®Ä/ÈõÜÊàê/ËßÑÂàí)
‚îú‚îÄ‚îÄ examples/               Aura source examples
‚îú‚îÄ‚îÄ tests/                  Test files
‚î?  ‚îú‚îÄ‚îÄ phase{0..9}_tests.aura      Pure-Aura compiler migration tests
‚î?  ‚îú‚îÄ‚îÄ photon/
‚î?  ‚î?  ‚îú‚îÄ‚îÄ P1/ P2/ P3/ P4/         End-to-end differential cases
‚î?  ‚î?  ‚îî‚îÄ‚îÄ S1/ S2/ S3/ S4/         Backend unit/integration tests
‚î?  ‚îú‚îÄ‚îÄ aot/ self_bootstrap/ pure_aura/ pure_aura_cffi/ compiler/ ...
‚î?  ‚îî‚îÄ‚îÄ snapshots/                  Snapshot tests
‚îú‚îÄ‚îÄ scripts/                PowerShell build, bootstrap & suite scripts
‚î?  ‚îú‚îÄ‚îÄ build-aura-compiler.ps1        Seed ‚Ü?Aura-compiler build
‚î?  ‚îú‚îÄ‚îÄ bootstrap-photon.ps1           Photon bootstrap
‚î?  ‚îú‚îÄ‚îÄ photon-hat-bootstrap.ps1       HAT self-bootstrap of Main.aura
‚î?  ‚îú‚îÄ‚îÄ photon-hat-native-suite.ps1    HAT chain differential suite
‚î?  ‚îú‚îÄ‚îÄ photon-hat-suite.ps1           VM‚ÜíHAT chain differential suite
‚î?  ‚îî‚îÄ‚îÄ run-hat-on-main.ps1            HAT driver metrics harness
‚îú‚îÄ‚îÄ tools/                  Editor/IDE tooling
‚î?  ‚îú‚îÄ‚îÄ dsh-plugins/        DSH syntax highlighting (aura, hat, phir)
‚î?  ‚îú‚îÄ‚îÄ ide-extension/      IDE extension sources
‚î?  ‚îî‚îÄ‚îÄ skills/
‚îî‚îÄ‚îÄ build/                  Generated artifacts (incl. build/hat-native/, build/hat-bootstrap/)
```

> **Layout note:** the Rust compiler, CLI and `loom` build system live under `rust/`, not at the
> repository root. `aura/` holds the Aura-written language sources; the compiler is bootstrapped
> from the pre-built seed at `aura/seed/aura.exe`.

---

## Quick Start

### Prerequisites

- Rust 1.75+ (edition 2024)
- LLVM ‚â?23 (tested with `clang+llvm-23.1.0-x86_64-pc-windows-msvc`), including `lld-link.exe` for the
  HAT/Photon native backend; `llc` for the LLVM-IR AOT path
- Windows / PowerShell 5.1+ for the build and bootstrap scripts

### Build

```powershell
# Build the Aura compiler (self-bootstrap, no Rust toolchain needed)
scripts\build-aura-compiler.ps1

# Build with AOT (native executable, needs LLVM)
scripts\build-aura-compiler.ps1 -Aot

# Rebuild the seed binary from Rust source (when rust/compiler is updated)
scripts\build-aura-compiler.ps1 -RebuildSeed

# Rebuild the native HAT driver (AOT-compiled by the Rust `aura` binary, ~12 s)
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
.\rust\target\release\aura.exe build --aot `
    aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura `
    --output build\hat-native\PhotonHatCompile.exe

# HAT differential suites (each rebuilds the driver first with -Rebuild)
.\scripts\photon-hat-native-suite.ps1 -Phase P1,P2,P3 -OutRoot build\hat-native-suite
.\scripts\photon-hat-suite.ps1        -Phase P1,P2,P3 -OutRoot build\hat-suite
```

> **Note:** The frozen seed at `aura/seed/aura.exe` (git-LFS tracked) eliminates the need for a Rust
> toolchain entirely ‚Ä?`build-aura-compiler.ps1 -FrozenSeed` uses it directly. Otherwise the script
> prefers a cargo build at `rust/target/{release,debug}/aura.exe`. Only `-RebuildSeed` needs `cargo`.

### Run

```bash
# Compile and execute
aura run examples/compiler/showcase.aura

# AOT compile to native executable (LLVM IR path)
aura build --aot examples/games/game_2d_demo.aura --target x86_64-pc-windows-msvc

# Interactive REPL
aura repl

# Evaluate a snippet
aura eval --expr "println('Hello, Aura!')"

# HAT / Photon native path ‚Ä?.hat IR + direct COFF, no LLVM IR involved
$env:AURA_HAT_AURA  = "examples\compiler\showcase.aura"
$env:AURA_HAT_OUT   = "build\hat-demo"
$env:AURA_HAT_MODULE = "showcase"
.\build\hat-native\PhotonHatCompile.exe
```

---

## Command-Line Reference

```text
aura build <file.aura>                          Compile to bytecode (.auc)
aura build <file.aura> --aot [--target <triple>] AOT compile to native
aura build <file.aura> --lib                    Package as .auz library
aura run <file.aura> [--jit]                    Compile + execute (VM or JIT)
aura check <file.aura>                          Syntax/semantic check only
aura disasm <file.auc>                          Disassemble bytecode
aura tokens <file.aura>                         Print token stream
aura ast <file.aura>                            Print AST
aura fmt <file.aura> [--check]                  Code formatter
aura leak-check <file.aura>                     ARC memory leak analysis
aura doc [--output <dir>]                       Generate std API docs
aura eval [--expr <code>]                       Execute snippet (like node -e)
aura repl                                       Interactive REPL
aura install                                    Install dependencies
aura update [--all]                             Update dependencies
aura publish [--dir <path>]                     Publish package
aura deps                                       Show dependency tree
aura new <name>                                 Create new project
aura package <file.aura>                        Package as .auz artifact
aura inspect <file.auz>                         Inspect .auz contents
aura verify <file.auz>                          Verify .auz checksum
aura lsp                                        Start LSP server (stdio)
aura debug <file.aura>                          Start debugger
```

Standalone binaries:

```text
aura-lsp              LSP server (independent process, no VM overhead)
aura-debug <file.aura> [--mode vm|jit|aot]  Source-level debugger
```

---

## Language Features

### Variables & Types

```aura
val x: Int = 42                    // Immutable
var y: Int = 0                    // Mutable
lateinit var cache: String         // Lazy init
val lazyVal by lazy { compute() }  // Lazy evaluation

// Type system
val list: List<Int> = listOf(1, 2, 3)
val map: Map<String, Int> = mapOf("a" to 1)
val opt: Int? = null               // Nullable
typealias Vec2 = Pair<Int, Int>    // Type alias
```

### Functions

```aura
fun add(a: Int, b: Int): Int = a + b        // Expression body
fun power(base: Int, exp: Int = 2): Int { }  // Default params
fun join(vararg parts: String): String { }   // Variadic
fun <T: Number> first(items: List<T>): T? { }// Generic + constraint

// Lambdas
val f = { x: Int -> x + 1 }
val mapped = listOf(1,2,3).map { x -> x * 2 }
```

### Classes & Interfaces

```aura
data struct Player(val id: Int, var name: String = "unknown", var health: Int = 100)
struct Point(val x: Int, val y: Int) { fun manhattan(): Int = x + y }
sealed class Shape { fun area(): Float = 0.0f }
enum Color { RED, GREEN, CUSTOM(val r: Int, val g: Int, val b: Int) }
interface Drawable { fun draw(): Unit }
class Circle : Drawable { override fun draw() {} }
class Dog : Animal() { override fun name(): String = "dog" }
actor Scheduler { var tick: Int = 0; fun step() { tick += 1 } }
```

### Control Flow

```aura
val status = if (hp > 0) "alive" else "dead"

val band = when (score) {
    in 90..100 -> "A"
    in 80..89  -> "B"
    else       -> "F"
}

val len = when (val) {
    is String -> val.length
    is Int    -> 1
    else      -> 0
}

for (i in 0..10) { ... }
while (cond) { ... }
do { ... } while (cond)
break@outer / continue@outer
```

### Null Safety

```aura
var n: Int? = null
val safe: Int = n ?: 0          // Elvis operator
val tl = p.tag?.length          // Safe call
val forced: Int = n!!           // Force unwrap
```

### Concurrency

```aura
import aura.concurrent.*

// Actors
val worker = spawnActor("Worker")
send(worker, "task")
val reply = ask(worker, "request")

// Coroutines
val result = spawn(42)
val computed = await(100)

// Channels
val ch = channel<Int>()
ch.send(42)
val msg = ch.receive()
```

### FFI

```aura
extern "c" fun puts(msg: String): Int
val ret = puts("Hello from C!")
```

### String Interpolation

```aura
val name = "world"
println("Hello, $name!")
println("Level: ${hp * 2}")
val raw = """No $interpolation or \n here"""
```

---

## Standard Library

All standard library modules are under the `aura.lang.std` package.

| Module | Description |
|--------|-------------|
| `aura.lang.std.math` | sin, cos, tan, sqrt, pow, abs, min, max, round, floor, ceil, log, exp, PI, E |
| `aura.lang.std.io` | readFile, writeFile, readLine, writeLine, println, print |
| `aura.lang.std.collections` | List, Map, Set operations (filter, map, reduce, sort, zip, ...) |
| `aura.lang.std.concurrent` | Actor, Channel, Coroutine, spawn, send, ask, supervise, threadPool |
| `aura.lang.std.json` | JSON parse, stringify, pretty-print |
| `aura.lang.std.string` | String operations (split, join, replace, trim, toUpperCase, ...) |
| `aura.lang.std.fs` | File system (exists, remove, mkdir, readDir, copy, move) |
| `aura.lang.std.env` | Environment variables (get, set, remove) |
| `aura.lang.std.process` | Process management (exec, spawn, exit, arguments) |
| `aura.lang.std.time` | Time & dates (now, millis, timestamp, date formatting) |
| `aura.lang.std.path` | Path operations (join, normalize, resolve, base, dir, ext) |
| `aura.lang.std.console` | Terminal control (clear, cursor, colors, width, height) |
| `aura.lang.std.assert` | Assertions (assert, assertEquals, assertThrows) |
| `aura.lang.std.test` | Test framework (describe, it, before, after) |
| `aura.lang.std.net` | Network (HTTP client, URL, WebSocket) |
| `aura.lang.std.random` | Random numbers (nextInt, nextFloat, shuffle) |
| `aura.lang.std.encoding` | Encoding (base64, hex, URL encoding) |
| `aura.lang.std.ascii` | ASCII operations (isAlpha, isDigit, toUpper, toLower) |
| `aura.lang.std.iter` | Iterator operations |
| `aura.lang.std.builtin` | Built-in utilities (typeof, typeOf, isNull, isNotNull, ...) |

**Prelude** (always available, no import needed): `println`, `print`, `puts`, `abs`, `sqrt`, `pow`, `toInt`, `toFloat`, `toStr`, `toString`, `clock`, `strlen`, `CString`, `CStr`, `ptrIsNull`, `ptrToInt`, `intToPtr`, `makeCallback`, `listOf`, `assertTrue`, `assertFalse`, `assertEq`, `assertNotEq`, `assertNotNull`, `assertNull`, `assertContains`, `assertNotContains`, `assertGt`, `assertGte`, `assertLt`, `assertLte`, `assertApprox`, `assertArrayEq`, `assertMapEq`, `pass`, `fail`

**Core Source** (core/aura/lang/): IDE-facing type declarations for `Any`, `Int`, `String`, `List`, `Map`, `Actor`, `Channel`, `Coroutine`, `Box`, `Weak`, `DeathStrategy`, `ProcessActor`, `IntRange`, etc.

---

## Build System (loom)

The `loom` build system is a Gradle/Bazel-style build tool for Aura projects:

```bash
loom new my-app          # Create project
loom build               # Build
loom test                # Run tests
loom run                 # Run application
loom watch               # Watch mode (incremental rebuild)
loom ci                  # CI/CD integration
```

Configuration via `aura.toml`:

```toml
name = "my-app"
version = "0.1.0"
entry = "main.aura"

[dependencies]
"aura-math" = { version = "1.0", rev = "main" }
```

---

## Editor Integration

VS Code and Sublime Text 4 packages live under `tools/ide-extension/`:

| Package | Contents |
|---------|----------|
| `tools/ide-extension/aura-vscode-extension` | Aura ‚Ä?LSP integration, syntax highlighting, snippets, formatting |
| `tools/ide-extension/aura-st4` | Aura ‚Ä?Sublime Text 4 syntax + build + keymap + snippets |
| `tools/ide-extension/hat-vscode-extension`, `hat-st4` | HAT IR syntax highlighting |
| `tools/ide-extension/phir-vscode-extension`, `phir-st4` | PHIR syntax highlighting |

DSH syntax highlighting plugins are built and packaged under `tools/dsh-plugins/`
(`aura-dsh-highlight`, `hat-dsh-highlight`, `phir-dsh-highlight`).

Install: `aura-language` from the VS Code Marketplace, or build from `tools/ide-extension/aura-vscode-extension/`.

---

## Development

The Rust workspace lives in `rust/`:

```bash
# Format
cd rust && cargo fmt --all

# Lint
cd rust && cargo clippy --all-features -- -D warnings

# Test
cd rust && cargo test --workspace
cd rust && cargo test --release --test perf_lexer -- --nocapture

# Update snapshots
INSTA_UPDATE=always cargo test
```

CI: `rust/.github/workflows/ci.yml` ‚Ä?`cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`
(debug + release), coverage.

### Debugging the HAT / Photon backend

Environment variables understood by `PhotonHatCompile.exe`:

| Variable | Effect |
|----------|--------|
| `AURA_PHOTON_TRACE=1` | Per-function `[ssab]` / `[ssae]` markers with value/expr/type counts ‚Ä?used to localize a crash to a specific function |
| `AURA_SSA_PERFN=1` | `[ssa-fn] n=<ordinal> <name>` per function, in definition order |
| `AURA_PHOTON_DEBUG_HIR=1` | Dump SSA MIR before HAT serialization |
| `AURA_PHOTON_STOP=B\|C` | Halt after Phase B (LIR) or Phase C (DAG) ‚Ä?narrows which stage owns a bad artifact |
| `AURA_HAT_AURA` / `AURA_HAT_OUT` / `AURA_HAT_MODULE` | Driver inputs (source path, output dir, module name) |

The driver always emits machine-readable markers regardless of verbosity:

```
===COFF-MAIN===<hex>     hex of the main COFF object
===COFF-RUNTIME===<hex>  hex of the runtime COFF object
===LINK===<command>      the exact lld-link invocation
===RESULT===success|fail
```

> **Known AOT hazard when writing Aura compiler code:** `String.charCodeAt(i)` is lowered to an
> **unchecked inline load** (`aot/Emit.aura`). A loop of the form
> `while (i >= 0 && s.charCodeAt(i) != 10)` will read out of bounds once `i` reaches -1 ‚Ä?> `&&` short-circuit does not protect you, because the bounds check lives in the Aura source of
> `charCodeAt` and is dropped by the AOT inlining. Structure the loop so only valid indices are
> ever passed in.


---

## Architecture

```
Source (.aura)
  ‚î?  ‚ñ?Lexer ‚îÄ‚îÄ‚îÄ‚îÄ‚ñ?Token stream
  ‚î?  ‚ñ?Parser ‚îÄ‚îÄ‚îÄ‚ñ?AST
  ‚î?  ‚ñ?Sema ‚îÄ‚îÄ‚îÄ‚îÄ‚îÄ‚ñ?Typed AST (type inference, null-safety)
  ‚î?  ‚ñ?HIR (desugar / mono / inline / fold)
  ‚î?  ‚îú‚îÄ‚îÄ‚ñ?MIR ‚îÄ‚îÄ‚ñ?Bytecode (.auc) ‚îÄ‚îÄ‚ñ?VM (interpreter) ‚îÄ‚îÄ‚ñ?JIT (Cranelift) ‚îÄ‚îÄ‚ñ?Native code
  ‚î?  ‚îú‚îÄ‚îÄ‚ñ?LLVM IR (.ll) ‚îÄ‚îÄ‚ñ?llc ‚îÄ‚îÄ‚ñ?.o ‚îÄ‚îÄ‚ñ?linker ‚îÄ‚îÄ‚ñ?Native executable
  ‚î?  ‚îî‚îÄ‚îÄ‚ñ?SSA MIR ‚îÄ‚îÄ‚ñ?.hat ‚îÄ‚îÄ‚ñ?LIR ‚îÄ‚îÄ‚ñ?Machine DAG ‚îÄ‚îÄ‚ñ?RegAlloc ‚îÄ‚îÄ‚ñ?X86 ‚îÄ‚îÄ‚ñ?COFF ‚îÄ‚îÄ‚ñ?lld-link
                  (text IR, round-trippable)
                  HAT / Photon native backend ‚Ä?no LLVM IR, no llc, direct COFF emission
```

Three independent native paths from the same HIR: the classic LLVM-IR AOT path, the JIT path, and
the HAT/Photon path that owns its own IR, register allocator, x86-64 encoder and COFF object writer.

---

## License

[Apache-2.0](LICENSE)
