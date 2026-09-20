# Aura Programming Language

> A system-level scripting language for NovaOS — Kotlin-style syntax, Rust implementation, AOT + JIT hybrid compilation, zero-cost FFI, ARC memory management.
>
> **中文文档** → [README.zh-CN.md](README.zh-CN.md)

Design docs: [技术方案.md](docs/Aura构建系统设计.md) · [开发规划与实现进度.md](docs/多进程与CLI架构分析报告.md)

---

## Current Status

The **full toolchain** is implemented and functional — from lexer to AOT-compiled native binaries, with VM, JIT, debugger, LSP, and a complete build system.

| Phase | Component | Status |
|-------|-----------|--------|
| P0 | Infrastructure (workspace, SourceMap, diagnostics, CI) | ✅ |
| P1 | Lexer | ✅ |
| P2 | Parser + AST | ✅ |
| P3 | Semantic analysis (types, null-safety, diagnostics) | ✅ |
| P4 | Bytecode compiler (HIR → MIR → bytecode) | ✅ |
| P5 | VM (stack-based interpreter) + JIT (Cranelift) | ✅ |
| P6 | AOT compiler (LLVM IR → native) | ✅ |
| P7 | Memory management (ARC, leak detection) | ✅ |
| P8 | FFI (C interop, extern "c", dynamic loading) | ✅ |
| P9 | Standard library (19 modules, 320+ functions) | ✅ |
| P10 | Concurrency (Actors, Channels, Coroutines, ThreadPool) | ✅ |
| P11 | Package management (aura.toml, .auz packages) | ✅ |
| P12 | IPC (TCP channels, actor processes) | ✅ |
| P13 | Toolchain (LSP, formatter, VS Code extension) | ✅ |
| P14 | Examples & integration tests | ✅ |
| P15 | Debugger (VM / JIT / AOT modes) | ✅ |
| P16 | Build system (loom) | ✅ |

---

## Pure Aura Compiler Migration (纯 Aura 化迁移)

In parallel with the Rust compiler, a **compiler written entirely in Aura** is being built under `aura/compiler/aura/lang/compiler/`. The Rust compiler (`compiler/`) is preserved unmodified as the fallback and reference implementation.

### Self-Bootstrap Process (自举流程)

The Aura compiler is bootstrapped from a pre-built seed binary:

```
aura/seed/aura.exe          <- Rust bootstrap (seed, no rebuild needed)
        │
        ▼  aura build aura/compiler/.../Main.aura
        │
        ▼
build/auc/compiler/aura-compiler.auc   <- Aura-written compiler bytecode
        │
        ▼  aura run aura-compiler.auc <file.aura>
        │
        ▼
build/output/<file>.exe               <- AOT-compiled native executable
```

**Bootstrap resolution order** (in `build-aura-compiler.ps1`):

1. `aura/seed/aura.exe` — pre-built seed (preferred, no Rust toolchain needed)
2. `target/release/aura.exe` — local Rust build (if seed missing)
3. `cargo build` — rebuild from Rust source (if neither exists)

**Rebuilding the seed** (when Rust source is updated):

```powershell
scripts\build-aura-compiler.ps1 -RebuildSeed
```

This runs `cargo build --release -p cli --features llvm` and copies the result to `aura/seed/aura.exe`.

| Phase | Component | Status |
|-------|-----------|--------|
| P0 | Infrastructure (TestRunner, Main skeleton) | ✅ |
| P1 | Lexer + Parser + AST | ✅ |
| P2 | Sema (Type, SymbolTable, TypeInfo, TypeChecker) + HIR (Lower, Desugar, Mono, Inline, Fold) | ✅ |
| P3 | MIR (IR types, HIR→MIR lowering, DCE/CSE/const-prop optimizations) | ✅ |
| P4 | Bytecode Codegen (MIR→.auc) + VM interpreter | ✅ |
| P5 | VM enhancements (closures, tail-call, stack frames) | ✅ |
| P6 | AOT backend (LLVM IR generation) | ✅ |
| P6.5 | AOT hardening: classes / std signature table / collections / multi-module link | 🚧 In progress |
| P7 | JIT (Cranelift) | ✅ |
| P8 | Core & standard library in Aura | ✅ |
| P9 | End-to-end compile pipeline (VM / JIT / AOT) | ✅ |

### Test Results (Phase 0–2)

```
tests/phase0_tests.aura           → RESULT: PASS
tests/phase1_lexer_tests.aura     → RESULT: PASS
tests/phase2_sema_hir_tests.aura  → RESULT: PASS  (20 test groups, 0 failures)
tests/phase3_mir_tests.aura       → RESULT: PASS  (11 test groups, 0 failures)
```

Key modules under `aura/compiler/aura/lang/compiler/`:

```
test/       TestRunner.aura        — Test framework (self-contained)
lexer/      Span, Token, Lexer     — Tokenizer with string interpolation
parser/     Parser                 — Recursive-descent + Pratt parser
ast/        Ast                    — Flat arena AST (kinds/texts/tys/spans/kids)
sema/       Type, SymbolTable, TypeInfo, TypeChecker — Type system & semantic analysis
hir/        Hir, Desugar, Mono, Inline, Fold — HIR lowering & optimization passes
mir/        Mir, MirLower, MirOpt  — MIR IR, HIR→MIR lowering, optimization
codegen/    Codegen                — MIR → bytecode emission
vm/         VmRunner/Closures/TailCall/FrameManager — VM interpreter
aot/        Emit/StdSigs/Runtime/ModuleLink          — LLVM IR emitter, std signature table, multi-module linker
jit/        JitCore/JitState/JitOpt                  — JIT backend
errors/     CompileError           — Diagnostic model
Main.aura                       — Compiler entry skeleton
```

---

## Repository Structure

```text
AuraLang/
├── Cargo.toml              Workspace root (architectural dependency)
├── LICENSE                 Apache-2.0
├── README.md               ← You are here
├── README.zh-CN.md         中文文档
│
├── aura/                   Aura language sources (self-bootstrapping)
│   ├── core/               Core language types (Any, Int, String, ...)
│   ├── compiler/           Compiler written in Aura
│   │   └── aura/lang/compiler/   Lexer, Parser, Sema, HIR, MIR, Codegen, VM, AOT, JIT
│   ├── toolchain/          LSP, debugger, docgen, cli
│   └── seed/               Bootstrap seed binary
│       └── aura.exe        <- Rust bootstrap (pre-built, no rebuild needed)
│
├── build/                  Compiled artifacts (.auc bytecode)
│   ├── bin/                aura.exe (bootstrap), aura-compiler.auc
│   └── auc/compiler/       Aura-written compiler output
│
├── compiler/               [ARCH] Rust compiler source (architectural dependency)
├── cli/                    [ARCH] Rust CLI source (architectural dependency)
│
├── book/                   User documentation (tutorials, API, migration)
├── docs/                   Technical design documents
├── examples/               Aura source examples
├── scripts/                Build & bootstrap scripts
├── tests/                  Test files
└── target/                 Rust build artifacts (generated)
```

> **[ARCH]** = Architectural dependency: Rust source kept for bootstrap purposes only.
> All Rust/C generated artifacts (`.exe`, `.ll`, `.obj`, `.llc.log`, etc.) have been removed.

---

## Quick Start

### Prerequisites

- Rust 1.75+ (edition 2024)
- LLVM 17+ (for AOT; optional for VM/JIT mode)

### Build

```powershell
# Build the Aura compiler (self-bootstrap, no Rust toolchain needed)
scripts\build-aura-compiler.ps1

# Build with AOT (native executable, needs LLVM)
scripts\build-aura-compiler.ps1 -Aot

# Rebuild the seed binary from Rust source (when compiler/ is updated)
scripts\build-aura-compiler.ps1 -RebuildSeed
```

> **Note:** The pre-built seed at `aura/seed/aura.exe` eliminates the need for a Rust
> toolchain during normal development. Only `-RebuildSeed` requires `cargo`.

### Run

```bash
# Compile and execute
aura run examples/compiler/showcase.aura

# AOT compile to native executable
aura build --aot examples/games/game_2d_demo.aura --target x86_64-pc-windows-msvc

# Interactive REPL
aura repl

# Evaluate a snippet
aura eval --expr "println('Hello, Aura!')"
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

## VS Code Extension

Aura language support for VS Code: LSP integration, syntax highlighting, snippets, formatting.

Install: `aura-language` from VS Code Marketplace, or build from `vscode-extension/`.

---

## Development

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --all-features -- -D warnings

# Test
cargo test --workspace
cargo test --release --test perf_lexer -- --nocapture

# Update snapshots
INSTA_UPDATE=always cargo test
```

CI: `.github/workflows/ci.yml` — `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` (debug + release), coverage.

---

## Architecture

```
Source (.aura)
  │
  ▼
Lexer ────► Token stream
  │
  ▼
Parser ───► AST
  │
  ▼
Sema ─────► Typed AST (type inference, null-safety)
  │
  ▼
HIR (desugar) ──► MIR (lowering)
  │
  ├──► Bytecode (.auc) ──► VM (interpreter) ──► JIT (Cranelift) ──► Native code
  │
  └──► LLVM IR (.ll) ──► llc ──► .o ──► linker ──► Native executable
```

---

## License

[Apache-2.0](LICENSE)
