# Aura 编程语言

> 为 NovaOS 从零构建的系统级脚本语言 —— Rust 实现、Kotlin 风格语法、AOT + JIT 混合编译、零开销 FFI、ARC 内存管理。
>
> **English** → [README.md](README.md)

详细设计见 [构建系统设计文档](docs/Aura构建系统设计.md) · [多进程与CLI架构分析报告](docs/多进程与CLI架构分析报告.md)

---

## 当前状态

**完整工具链已实现并可运行** —— 从词法分析到 AOT 原生二进制，涵盖 VM、JIT、调试器、LSP 以及完整构建系统。

| 阶段 | 组件 | 状态 |
|------|------|------|
| P0 | 基础设施（workspace、SourceMap、诊断、CI） | ✅ |
| P1 | 词法分析器 | ✅ |
| P2 | 语法分析器 + AST | ✅ |
| P3 | 语义分析（类型推断、空安全、诊断） | ✅ |
| P4 | 字节码编译器（HIR → MIR → 字节码） | ✅ |
| P5 | VM（栈式解释器）+ JIT（Cranelift） | ✅ |
| P6 | AOT 编译器（LLVM IR → 原生码） | ✅ |
| P7 | 内存管理（ARC、泄漏检测） | ✅ |
| P8 | FFI（C 互操作、extern "c"、动态加载） | ✅ |
| P9 | 标准库（19 个模块，320+ 函数） | ✅ |
| P10 | 并发（Actor、Channel、协程、线程池） | ✅ |
| P11 | 包管理（aura.toml、.auz 制品） | ✅ |
| P12 | 进程间通信（TCP 通道、Actor 进程） | ✅ |
| P13 | 工具链（LSP、格式化器、VS Code 扩展） | ✅ |
| P14 | 示例与集成测试 | ✅ |
| P15 | 调试器（VM / JIT / AOT 模式） | ✅ |
| P16 | 构建系统（loom） | ✅ |

---

## 纯 Aura 编译器迁移进展

与 Rust 编译器并行，正在构建一个**完全用 Aura 语言编写的编译器**（位于 `aura/compiler/aura/lang/compiler/`）。Rust 编译器（`compiler/`）完全保留不修改，作为 fallback 与参考实现。

| 阶段 | 组件 | 状态 |
|------|------|------|
| P0 | 基础设施（TestRunner、Main 骨架） | ✅ |
| P1 | Lexer + Parser + AST | ✅ |
| P2 | Sema（Type、SymbolTable、TypeInfo、TypeChecker）+ HIR（Lower、Desugar、Mono、Inline、Fold） | ✅ |
| P3 | MIR（IR 类型、HIR→MIR 降级、DCE/CSE/常量传播优化） | ✅ |
| P4 | 字节码 Codegen（MIR→.auc）+ VM 解释器 | 🔲 进行中 |
| P5 | VM 增强（闭包、尾调用、栈帧） | 🔲 未开始 |
| P6 | AOT 后端（LLVM IR 生成） | 🔲 未开始 |

### 测试结果（Phase 0–2）

```
tests/phase0_tests.aura           → RESULT: PASS
tests/phase1_lexer_tests.aura     → RESULT: PASS
tests/phase2_sema_hir_tests.aura  → RESULT: PASS  (20 个测试组，0 失败)
tests/phase3_mir_tests.aura       → RESULT: PASS  (11 个测试组，0 失败)
```

### 主要模块

```
test/       TestRunner.aura        — 测试框架（自包含）
lexer/      Span, Token, Lexer     — 词法分析器（支持字符串插值）
parser/     Parser                 — 递归下降 + Pratt 优先级解析器
ast/        Ast                    — 扁平 arena AST（kinds/texts/tys/spans/kids）
sema/       Type, SymbolTable, TypeInfo, TypeChecker — 类型系统与语义分析
hir/        Hir, Desugar, Mono, Inline, Fold — HIR 降级与优化 passes
mir/        Mir, MirLower, MirOpt  — MIR IR、HIR→MIR 降级、优化
codegen/    Codegen                — MIR → 字节码发射
errors/     CompileError           — 诊断模型
Main.aura                       — 编译器入口骨架
```

---

## 仓库结构

```text
AuraLang/
├── Cargo.toml              Workspace 根（compiler, cli, loom）
├── LICENSE                 Apache-2.0
├── README.md               English
├── README.zh-CN.md         ← 中文文档
│
├── compiler/               编译器库（Rust crate）
│   ├── src/
│   │   ├── lexer.rs            词法分析器（手写，支持插值、原始字符串）
│   │   ├── parser.rs           递归下降 + Pratt 优先级解析器
│   │   ├── ast.rs              AST 节点定义
│   │   ├── sema/               语义分析（ty / symbol / checker）
│   │   ├── codegen/
│   │   │   ├── hir.rs              HIR 脱糖
│   │   │   ├── mir.rs              MIR 降级
│   │   │   ├── emit.rs             字节码发射
│   │   │   ├── opt.rs              优化 passes
│   │   │   ├── arc.rs              ARC 分析与插入
│   │   │   ├── serialize.rs        .auc 二进制格式
│   │   │   └── aot/                AOT（LLVM）后端
│   │   │       ├── emit.rs         LLVM IR 生成
│   │   │       ├── linker.rs       llc/clang 链接
│   │   │       ├── target.rs       跨平台三元组
│   │   │       ├── dwarf.rs        DWARF 调试信息
│   │   │       └── c_backend.rs    C 代码回退
│   │   ├── vm/
│   │   │   ├── interp.rs           栈式解释器
│   │   │   ├── jit.rs              Cranelift JIT
│   │   │   ├── ffi.rs              C FFI（extern "c"）
│   │   │   ├── heap.rs             GC 堆 + ARC
│   │   │   ├── value.rs            运行时值
│   │   │   ├── actor.rs            Actor 运行时
│   │   │   ├── channel.rs          消息通道
│   │   │   ├── coroutine.rs        协程与 suspend
│   │   │   ├── thread_pool.rs      线程池
│   │   │   ├── debugger.rs         源码级调试器
│   │   │   └── ...                 IPC、动态 FFI、native 等
│   │   ├── std/                    标准库（19 个模块）
│   │   │   ├── decl.rs             标准库函数单一真相源
│   │   │   ├── std_math.rs         数学函数
│   │   │   ├── std_io.rs           输入输出
│   │   │   ├── std_collections.rs  集合操作
│   │   │   ├── std_concurrent.rs   并发 API
│   │   │   ├── std_json.rs         JSON 解析与序列化
│   │   │   ├── std_string.rs       字符串操作
│   │   │   ├── std_fs.rs           文件系统
│   │   │   ├── std_env.rs          环境变量
│   │   │   ├── std_process.rs      进程管理
│   │   │   ├── std_time.rs         时间日期
│   │   │   └── ...                 （+8 个模块）
│   │   ├── auz/                    .auz 制品格式
│   │   ├── lsp.rs                  LSP 服务器（stdio JSON-RPC）
│   │   ├── package.rs              包管理器
│   │   ├── docgen.rs               API 文档生成器
│   │   └── linker.rs               模块链接
│   ├── tests/                    集成测试（40+ 测试文件）
│   └── examples/                 AOT 基准测试
│
├── cli/                    命令行工具（3 个二进制）
│   └── src/
│       ├── main.rs             `aura` — 20+ 子命令
│       ├── lsp_main.rs         `aura-lsp` — 独立 LSP 进程
│       └── debugger_main.rs    `aura-debug` — 源码级调试器
│
├── loom/                   构建系统（Gradle/Bazel 风格）
│   ├── src/                    manifest、任务 DAG、缓存、插件、CI/CD
│   ├── docs/                   设计文档
│   └── examples/               示例项目
│
├── vscode-extension/       VS Code 扩展（LSP + 语法高亮）
│
├── book/                   用户文档（教程、API、迁移指南）
├── docs/                   技术设计文档
└── examples/               Aura 源码示例（36 个文件）
```

---

## 快速开始

### 前置要求

- Rust 1.75+（edition 2024）
- LLVM 17+（AOT 编译需要；VM/JIT 模式可选）

### 构建

```bash
# 完整工具链（VM + JIT + AOT + 全部标准库）
cargo build --release --features "llvm,jit,std-all"

# 最小构建（仅 VM）
cargo build --release
```

### 运行

```bash
# 编译并执行
aura run examples/compiler/showcase.aura

# AOT 编译为原生可执行文件
aura build --aot examples/games/game_2d_demo.aura --target x86_64-pc-windows-msvc

# 交互式 REPL
aura repl

# 执行代码片段
aura eval --expr "println('Hello, Aura!')"
```

---

## 命令行参考

```text
aura build <file.aura>                          编译为字节码 (.auc)
aura build <file.aura> --aot [--target <triple>] AOT 编译为原生可执行文件
aura build <file.aura> --lib                    打包为 .auz 库制品
aura run <file.aura> [--jit]                    编译并执行（VM 或 JIT）
aura check <file.aura>                          仅做语法/语义检查
aura disasm <file.auc>                          反汇编字节码
aura tokens <file.aura>                         输出词法分析结果
aura ast <file.aura>                            输出 AST
aura fmt <file.aura> [--check]                  代码格式化
aura leak-check <file.aura>                     ARC 内存泄漏分析
aura doc [--output <dir>]                       生成标准库 API 文档
aura eval [--expr <code>]                       执行代码片段（类 node -e）
aura repl                                       交互式 REPL
aura install                                    安装依赖
aura update [--all]                             更新依赖
aura publish [--dir <path>]                     发布包
aura deps                                       显示依赖树
aura new <name>                                 创建新项目
aura package <file.aura>                        打包为 .auz 制品
aura inspect <file.auz>                         检查 .auz 内容
aura verify <file.auz>                          验证 .auz 校验和
aura lsp                                        启动 LSP 服务器（stdio）
aura debug <file.aura>                          启动调试器
```

独立二进制：

```text
aura-lsp                    独立 LSP 服务器进程（不加载 VM，零开销）
aura-debug <file.aura> [--mode vm|jit|aot]  源码级调试器
```

---

## 语言特性

### 变量与类型

```aura
val x: Int = 42                    // 不可变
var y: Int = 0                    // 可变
lateinit var cache: String         // 延迟初始化
val lazyVal by lazy { compute() }  // 惰性求值

// 类型系统
val list: List<Int> = listOf(1, 2, 3)
val map: Map<String, Int> = mapOf("a" to 1)
val opt: Int? = null               // 可空类型
typealias Vec2 = Pair<Int, Int>    // 类型别名
```

### 函数

```aura
fun add(a: Int, b: Int): Int = a + b        // 表达式体
fun power(base: Int, exp: Int = 2): Int { }  // 默认参数
fun join(vararg parts: String): String { }   // 可变参数
fun <T: Number> first(items: List<T>): T? { }// 泛型 + 约束

// Lambda
val f = { x: Int -> x + 1 }
val mapped = listOf(1,2,3).map { x -> x * 2 }
```

### 类与接口

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

### 控制流

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

### 空安全

```aura
var n: Int? = null
val safe: Int = n ?: 0          // Elvis 运算符
val tl = p.tag?.length          // 安全调用
val forced: Int = n!!           // 强制解包
```

### 并发

```aura
import aura.concurrent.*

// Actor
val worker = spawnActor("Worker")
send(worker, "task")
val reply = ask(worker, "request")

// 协程
val result = spawn(42)
val computed = await(100)

// 消息通道
val ch = channel<Int>()
ch.send(42)
val msg = ch.receive()
```

### FFI

```aura
extern "c" fun puts(msg: String): Int
val ret = puts("Hello from C!")
```

### 字符串插值

```aura
val name = "world"
println("Hello, $name!")
println("Level: ${hp * 2}")
val raw = """不做 $interpolation，不做 \n 转义"""
```

---

## 标准库

所有标准库模块位于 `aura.lang.std` 包下。

| 模块 | 说明 |
|------|------|
| `aura.lang.std.math` | 数学函数（sin, cos, tan, sqrt, pow, abs, min, max, round, floor, ceil, log, exp, PI, E） |
| `aura.lang.std.io` | 输入输出（readFile, writeFile, readLine, writeLine, println, print） |
| `aura.lang.std.collections` | 集合操作（List, Map, Set — filter, map, reduce, sort, zip 等） |
| `aura.lang.std.concurrent` | 并发（Actor, Channel, Coroutine, spawn, send, ask, supervise, threadPool） |
| `aura.lang.std.json` | JSON 解析与序列化 |
| `aura.lang.std.string` | 字符串操作（split, join, replace, trim, toUpperCase 等） |
| `aura.lang.std.fs` | 文件系统（exists, remove, mkdir, readDir, copy, move） |
| `aura.lang.std.env` | 环境变量（get, set, remove） |
| `aura.lang.std.process` | 进程管理（exec, spawn, exit, arguments） |
| `aura.lang.std.time` | 时间日期（now, millis, timestamp, date formatting） |
| `aura.lang.std.path` | 路径操作（join, normalize, resolve, base, dir, ext） |
| `aura.lang.std.console` | 终端控制（clear, cursor, colors, width, height） |
| `aura.lang.std.assert` | 断言（assert, assertEquals, assertThrows） |
| `aura.lang.std.test` | 测试框架（describe, it, before, after） |
| `aura.lang.std.net` | 网络（HTTP 客户端、URL、WebSocket） |
| `aura.lang.std.random` | 随机数（nextInt, nextFloat, shuffle） |
| `aura.lang.std.encoding` | 编码（base64, hex, URL 编码） |
| `aura.lang.std.ascii` | ASCII 操作（isAlpha, isDigit, toUpper, toLower） |
| `aura.lang.std.iter` | 迭代器操作 |
| `aura.lang.std.builtin` | 内置工具（typeof, typeOf, isNull, isNotNull, ...） |

**Prelude**（免 import，始终可用）：`println`, `print`, `puts`, `abs`, `sqrt`, `pow`, `toInt`, `toFloat`, `toStr`, `toString`, `clock`, `strlen`, `CString`, `CStr`, `ptrIsNull`, `ptrToInt`, `intToPtr`, `makeCallback`, `listOf`, `assertTrue`, `assertFalse`, `assertEq`, `assertNotEq`, `assertNotNull`, `assertNull`, `assertContains`, `assertNotContains`, `assertGt`, `assertGte`, `assertLt`, `assertLte`, `assertApprox`, `assertArrayEq`, `assertMapEq`, `pass`, `fail`

**Core Source**（core/aura/lang/）：IDE 可见的类型声明，包括 `Any`, `Int`, `String`, `List`, `Map`, `Actor`, `Channel`, `Coroutine`, `Box`, `Weak`, `DeathStrategy`, `ProcessActor`, `IntRange` 等。

---

## 构建系统（loom）

`loom` 是 Aura 项目的 Gradle/Bazel 风格构建系统：

```bash
loom new my-app          # 创建项目
loom build               # 构建
loom test                # 运行测试
loom run                 # 运行应用
loom watch               # 监听模式（增量重编）
loom ci                  # CI/CD 集成
```

通过 `aura.toml` 配置：

```toml
name = "my-app"
version = "0.1.0"
entry = "main.aura"

[dependencies]
"aura-math" = { version = "1.0", rev = "main" }
```

---

## VS Code 扩展

Aura 语言的 VS Code 支持：LSP 集成、语法高亮、代码片段、格式化。

安装：从 VS Code Marketplace 搜索 `aura-language`，或从 `vscode-extension/` 源码构建。

---

## 开发

```bash
# 格式化
cargo fmt --all

# Lint
cargo clippy --all-features -- -D warnings

# 测试
cargo test --workspace
cargo test --release --test perf_lexer -- --nocapture

# 更新快照
INSTA_UPDATE=always cargo test
```

CI 配置在 `.github/workflows/ci.yml`：包含 `cargo fmt --check`、`cargo clippy -D warnings`、`cargo test`（debug + release）与覆盖率采集。

---

## 架构

```
源码 (.aura)
  │
  ▼
Lexer ────► Token 流
  │
  ▼
Parser ───► AST
  │
  ▼
Sema ─────► 带类型 AST（类型推断、空安全）
  │
  ▼
HIR（脱糖）───► MIR（降级）
  │
  ├──► 字节码 (.auc) ──► VM（解释器）───► JIT（Cranelift）───► 原生码
  │
  └──► LLVM IR (.ll) ──► llc ──► .o ──► 链接器 ──► 原生可执行文件
```

---

## 许可证

[Apache-2.0](LICENSE)
