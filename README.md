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

## Repository Structure

```text
AuraLang/
├── Cargo.toml              Workspace root (compiler, cli, loom)
├── LICENSE                 Apache-2.0
├── README.md               ← You are here
├── README.zh-CN.md         中文文档
│
├── compiler/               Compiler library (Rust crate)
│   ├── src/
│   │   ├── lexer.rs            Lexer (hand-written, string interpolation, raw strings)
│   │   ├── parser.rs           Recursive-descent + Pratt parser
│   │   ├── ast.rs              AST node definitions
│   │   ├── sema/               Semantic analysis (ty, symbol, checker)
│   │   ├── codegen/
│   │   │   ├── hir.rs              HIR desugaring
│   │   │   ├── mir.rs              MIR lowering
│   │   │   ├── emit.rs             Bytecode emitter
│   │   │   ├── opt.rs              Optimization passes
│   │   │   ├── arc.rs              ARC analysis & insertion
│   │   │   ├── serialize.rs        .auc binary format
│   │   │   └── aot/                AOT (LLVM) backend
│   │   │       ├── emit.rs         LLVM IR generation
│   │   │       ├── linker.rs       llc/clang linking
│   │   │       ├── target.rs       Cross-platform triples
│   │   │       ├── dwarf.rs        DWARF debug info
│   │   │       └── c_backend.rs    C code fallback
│   │   ├── vm/
│   │   │   ├── interp.rs           Stack-based interpreter
│   │   │   ├── jit.rs              Cranelift JIT
│   │   │   ├── ffi.rs              C FFI (extern "c")
│   │   │   ├── heap.rs             GC heap + ARC
│   │   │   ├── value.rs            Runtime values
│   │   │   ├── actor.rs            Actor runtime
│   │   │   ├── channel.rs          Message channels
│   │   │   ├── coroutine.rs        Coroutines & suspend
│   │   │   ├── thread_pool.rs      ThreadPool
│   │   │   ├── debugger.rs         Source-level debugger
│   │   │   └── ...                 IPC, dynamic FFI, native, etc.
│   │   ├── std/                    Standard library (19 modules)
│   │   │   ├── decl.rs             Single source of truth for std functions
│   │   │   ├── std_math.rs         Math (sin, cos, sqrt, pow, ...)
│   │   │   ├── std_io.rs           I/O (readFile, writeFile, ...)
│   │   │   ├── std_collections.rs  List, Map, Set operations
│   │   │   ├── std_concurrent.rs   Actor, Channel, Coroutine APIs
│   │   │   ├── std_json.rs         JSON parsing & serialization
│   │   │   ├── std_string.rs       String operations
│   │   │   ├── std_fs.rs           File system
│   │   │   ├── std_env.rs          Environment variables
│   │   │   ├── std_process.rs      Process management
│   │   │   ├── std_time.rs         Time & dates
│   │   │   └── ...                 (+8 more modules)
│   │   ├── auz/                    .auz package format
│   │   ├── lsp.rs                  LSP server (JSON-RPC over stdio)
│   │   ├── package.rs              Package manager
│   │   ├── docgen.rs               API documentation generator
│   │   └── linker.rs               Module linking
│   ├── tests/                    Integration tests (40+ test files)
│   └── examples/                 AOT benchmarks
│
├── cli/                    Command-line tool (3 binaries)
│   └── src/
│       ├── main.rs             `aura` — 20+ subcommands
│       ├── lsp_main.rs         `aura-lsp` — standalone LSP server
│       └── debugger_main.rs    `aura-debug` — source-level debugger
│
├── loom/                   Build system (Gradle/Bazel-like)
│   ├── src/                    manifest, task DAG, cache, plugins, CI/CD
│   ├── docs/                   Design documents
│   └── examples/               Example projects
│
├── vscode-extension/       VS Code extension (LSP + syntax highlighting)
│
├── book/                   User documentation (tutorials, API, migration)
├── docs/                   Technical design documents
└── examples/               Aura source examples (36 files)
```

---

## Quick Start

### Prerequisites

- Rust 1.75+ (edition 2024)
- LLVM 17+ (for AOT; optional for VM/JIT mode)

### Build

```bash
# Full toolchain (VM + JIT + AOT + all std modules)
cargo build --release --features "llvm,jit,std-all"

# Minimal (VM only)
cargo build --release
```

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

**Phantom Source** (phantom-source/aura/lang/): IDE-facing type declarations for `Any`, `Int`, `String`, `List`, `Map`, `Actor`, `Channel`, `Coroutine`, `Box`, `Weak`, `DeathStrategy`, `ProcessActor`, `IntRange`, etc.

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
