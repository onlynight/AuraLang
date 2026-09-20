# AuraLang CLI 二进制拆分方案

> **状态**：方案已确认 → 待实施
> **日期**：2026-07-02
> **基线**：`多进程与CLI架构分析报告.md` §3 方案B

---

## 1. 目标

将当前 `cli/` crate 中**大而全的 `aura.exe`** 拆分为 **三个职责单一的二进制**，同时保持 `loom.exe`、`lsp.exe`、`aurad.exe` 三个独立工具。无过渡期，所有命令直接迁移到位。

| 现状 | 拆分后 |
|------|--------|
| `aura.exe`（~21 子命令） | `aurac.exe`（编译器） + `aura.exe`（运行时） + `aurap.exe`（包管理器） |
| `aura-lsp.exe` | `lsp.exe`（重命名） |
| `aura-debug.exe` | `aurad.exe`（重命名） |
| `loom.exe` | `loom.exe`（不变） |

---

## 2. 最终二进制矩阵

```
AuraLang
├── compiler/   (lib only)
├── cli/
│   ├── src/main.rs           → aura.exe      (运行时)
│   ├── src/compiler_main.rs  → aurac.exe     (编译器)   [新增]
│   ├── src/package_main.rs   → aurap.exe     (包管理)   [新增]
│   ├── src/lsp_main.rs       → lsp.exe       (LSP)
│   └── src/debugger_main.rs  → aurad.exe     (调试器)
└── loom/
    └── src/main.rs           → loom.exe      (构建系统)
```

| 二进制 | 中文名 | 定位 | 依赖的 compiler 模块 |
|--------|--------|------|----------------------|
| `aurac.exe` | Aura 编译器 | 源码 → 字节码/AOT/IR/库 | lexer, parser, sema, codegen, vm(debug), lsp(fmt), docgen, auz, hir, mir, arc, lsp |
| `aura.exe` | Aura 运行时 | 字节码/源码 → 执行 | vm, codegen, compiler(lib), lsp |
| `aurap.exe` | Aura 包管理器 | 包安装/发布/验证 | package, auz, compiler(lib) |
| `lsp.exe` | Aura LSP | 编辑器协议服务 | lsp, compiler(lib) |
| `aurad.exe` | Aura 调试器 | 源码级调试 | vm, codegen, compiler(lib) |
| `loom.exe` | Aura 构建系统 | 多文件构建编排 | compiler(lib) |

---

## 3. 各二进制命令分配

### 3.1 `aurac.exe` — Aura 编译器

> 职责：源码分析、字节码生成、AOT 编译、格式化工具
>
> 对标：`gcc` / `clang` / `rustc`

| 命令 | 说明 | 原属 aura.exe |
|------|------|:-------------:|
| `aurac build <file.aura> [--output <out>]` | 编译为字节码 .auc | ✅ |
| `aurac build <file.aura> --aot [--output <exe>]` | AOT 编译为原生可执行文件 | ✅ |
| `aurac build <file.aura> --aot [--target <triple>]` | 指定目标平台三元组 | ✅ |
| `aurac build <file.aura> --aot [--opt <level>]` | 优化级别 0/1/2/3/s/z | ✅ |
| `aurac build <file.aura> --aot [--emit-llvm]` | 仅生成 LLVM IR | ✅ |
| `aurac build <file.aura> --aot [--debug]` | 生成 DWARF 调试信息 | ✅ |
| `aurac build <file.aura> --aot [--shared]` | 生成动态库 (.so/.dll) | ✅ |
| `aurac build <file.aura> --aot [--cabi]` | C ABI 包装函数模式 | ✅ |
| `aurac build <file.aura> --aot-embed` | AOT 嵌入到 .auc v4 | ✅ |
| `aurac build <file.aura> --lib [--output <out>]` | 打包为 .auz 库制品 | ✅ |
| `aurac check <file.aura>` | 语法/语义检查 | ✅ |
| `aurac disasm <file.auc> [--source <f.aura>]` | 反汇编 | ✅ |
| `aurac tokens <file.aura>` | 词法分析 | ✅ |
| `aurac ast <file.aura>` | AST 输出 | ✅ |
| `aurac fmt <file.aura> [--check]` | 代码格式化 | ✅ |
| `aurac leak-check <file.aura>` | P7 内存泄漏检测 (ARC) | ✅ |
| `aurac stdlib-compile <core-dir> [--output <dir>]` | 预编译标准库 | ✅ |
| `aurac export-header <file.aura> --out <name>.h` | C ABI 头文件生成 | ✅ |

**aurac.exe 不需要 VM 执行**，所以不链接 VM 运行时、JIT、Actor 运行时、Channel 运行时，减小二进制体积。

### 3.2 `aura.exe` — Aura 运行时

> 职责：加载字节码/源码，启动 VM 执行
>
> 对标：`node` / `python` / `java`

| 命令 | 说明 | 原属 aura.exe |
|------|------|:-------------:|
| `aura run <file.aura> [--jit]` | 编译并执行（可选 JIT） | ✅ |
| `aura run <file.auc> [--stdlib-dir <dir>]` | 直接运行 .auc + 加载标准库 | ✅ |
| `aura eval [--expr <code>]` | 执行代码片段（类 node -e） | ✅ |
| `aura repl` | 交互式 REPL | ✅ |
| `aura debug <file.aura>` | 转发到 `aurad.exe` | ✅（目标改名） |
| `aura lsp` | 转发到 `lsp.exe` | ✅（目标改名） |

**aura.exe 需要编译源码为字节码**，所以仍链接 compiler 库中的 lexer/parser/sema/codegen/vm，但不包含 AOT LLVM 后端、包管理器、文档生成、ARC 分析等大模块。

### 3.3 `aurap.exe` — Aura 包管理器

> 职责：依赖管理、包发布、制品验证
>
> 对标：`npm` / `cargo`

| 命令 | 说明 | 原属 aura.exe |
|------|------|:-------------:|
| `aurap install [--offline]` | 安装依赖 | ✅ |
| `aurap update [--all]` | 更新依赖 | ✅ |
| `aurap publish [--dir <path>]` | 发布包 | ✅ |
| `aurap deps [--dir <path>] [--outdated]` | 显示依赖树 | ✅ |
| `aurap new <name> [--dir <path>]` | 创建新包项目 | ✅ |
| `aurap package <file.aura> [--output <out>] [--sources]` | 打包为 .auz | ✅ |
| `aurap inspect <file.auz> [--verbose]` | 检查 .auz 内容 | ✅ |
| `aurap verify <file.auz>` | 验证 .auz 校验和 | ✅ |

### 3.4 `lsp.exe` — Aura LSP 服务器（原 `aura-lsp.exe` 重命名）

| 命令 | 说明 |
|------|------|
| `lsp` | stdio 模式（默认） |
| `lsp --port <port>` | TCP 模式（预留） |
| `lsp --help, -h` | 帮助 |
| `lsp --version, -V` | 版本 |

### 3.5 `aurad.exe` — Aura 调试器（原 `aura-debug.exe` 重命名）

| 命令 | 说明 |
|------|------|
| `aurad <file.aura>` | VM 模式调试（默认） |
| `aurad --mode jit <file.aura>` | JIT 模式调试 |
| `aurad --mode aot <file.aura>` | AOT 模式调试 |
| `aurad --mode aot --launch <file.aura>` | AOT + 外部调试器 |
| `aurad --help, -h` | 帮助 |
| `aurad --version, -V` | 版本 |

**交互命令**（在 `aurad` 内）：`break`、`continue`、`step/next/out`、`backtrace`、`list`、`print`、`locals/stack`、`info`、`quit`

### 3.6 `loom.exe` — Aura 构建系统（不变）

| 命令 | 说明 |
|------|------|
| `loom build [profile/target/parallel/opt/debug/dir]` | 完整构建 |
| `loom compile [同上]` | 仅编译 |
| `loom test [同上]` | 编译+测试 |
| `loom check [--dir]` | 语法/语义检查 |
| `loom run [同上]` | 编译+运行 |
| `loom clean [--dir]` | 清理构建产物 |
| `loom resolve [--offline] [--dir]` | 解析依赖 |
| `loom ci [--steps]` | CI 流水线 |
| `loom watch [同上]` | 监听增量重编 |
| `loom check-config [--dir]` | 校验 aura.toml |
| `loom new <name> [--template]` | 创建新项目 |
| `loom wrapper install` | 构建包装器 |
| `loom ide [--dir] [--all-files]` | 生成 IDE 项目文件 |
| `loom doc [--output] [--module]` | 生成文档 |
| `loom fmt [--dir] [--check]` | 格式化源码 |
| `loom version` | 显示版本 |

---

## 4. 各二进制 Cargo.toml [[bin]] 配置

### 4.1 `cli/Cargo.toml`

```toml
[package]
name = "cli"
version.workspace = true
edition.workspace = true

# ── 拆分后的 5 个二进制 ──

# 编译器：源码分析 + 字节码生成 + AOT + 格式化工具
[[bin]]
name = "aurac"
path = "src/compiler_main.rs"

# 运行时：VM 执行（字节码/JIT）
[[bin]]
name = "aura"
path = "src/main.rs"

# 包管理器：依赖管理 + 包发布 + 制品验证
[[bin]]
name = "aurap"
path = "src/package_main.rs"

# LSP：语言服务器（独立进程）
[[bin]]
name = "lsp"
path = "src/lsp_main.rs"

# 调试器：源码级调试（VM/JIT/AOT 三种模式）
[[bin]]
name = "aurad"
path = "src/debugger_main.rs"

[dependencies]
compiler = { path = "../compiler" }

[features]
llvm = ["compiler/llvm", "compiler/dynamic-ffi", "compiler/std-collections"]
```

### 4.2 各二进制编译 feature 建议

| 二进制 | 编译命令 | 说明 |
|--------|---------|------|
| `aurac` | `cargo build --features llvm` | AOT 需要 LLVM 后端 |
| `aurac` | `cargo build --no-default-features` | 纯字节码编译器（最小体积） |
| `aura` | `cargo build` | 默认（VM + JIT） |
| `aura` | `cargo build --features llvm` | VM + JIT + AOT（完整） |
| `aurap` | `cargo build` | 无需 LLVM |
| `aurad` | `cargo build --features llvm` | AOT 调试需要 LLVM |
| `aurad` | `cargo build` | VM/JIT 调试（无需 LLVM） |
| `lsp` | `cargo build` | 无需 LLVM |

---

## 5. 跨二进制调用关系

```
┌───────────────────────────────────────────────────────────────────┐
│                        用户直接调用                                │
│                                                                   │
│   aurac.exe ← 源码编译     aura.exe ← 执行      aurap.exe ← 包管理 │
│                                                                   │
├───────────────────────────────────────────────────────────────────┤
│                        内部转发关系                                │
│                                                                   │
│   aura.exe ──→ 调用 aurad.exe  (debug 命令转发)                   │
│   aura.exe ──→ 调用 lsp.exe     (lsp 命令转发)                    │
│   aurad.exe ──→ 编译时调用 AOT 后端（如需 AOT 调试）               │
│   loom.exe  ──→ 构建时调用 aurac.exe / aura.exe                   │
└───────────────────────────────────────────────────────────────────┘
```

**转发搜索顺序**（与现有 `find_lsp_binary` / `find_aurad_binary` 一致）：

1. 当前可执行文件所在目录（`aurad.exe` / `lsp.exe` / `aurac.exe`）
2. `PATH` 环境变量

---

## 6. `aura.exe` 中的转发命令更新

### 6.1 `aura debug` → `aurad.exe`

```rust
// 原 cmd_debug 中 find_aura_debug_binary()
// 改为 find_aurad_binary()，搜索 "aurad.exe" / "aurad"
```

### 6.2 `aura lsp` → `lsp.exe`

```rust
// 原 cmd_lsp 中 find_lsp_binary()
// 改为 find_lsp_binary()，搜索 "lsp.exe" / "lsp"
```

---

## 7. 实施步骤

### Step 1：创建新入口文件

```
cli/src/compiler_main.rs   → aurac.exe 入口
cli/src/package_main.rs    → aurap.exe 入口
```

从 `cli/src/main.rs` 中提取对应命令的函数到两个新文件中，共享的辅助函数（`extract_opt`、`first_positional`、`default_output`、`default_output_base`）提取到 `cli/src/util.rs`。

### Step 2：更新 Cargo.toml

在 `cli/Cargo.toml` 中添加 `[[bin]]` 条目：
- `name = "aurac"` → `src/compiler_main.rs`
- `name = "aurap"` → `src/package_main.rs`
- `name = "lsp"` → `src/lsp_main.rs`（原 `aura-lsp`）
- `name = "aurad"` → `src/debugger_main.rs`（原 `aura-debug`）

### Step 3：重命名二进制

| 原名 | 新名 | 改动 |
|------|------|------|
| `aura-debug` | `aurad` | `[[bin]] name = "aurad"` |
| `aura-lsp` | `lsp` | `[[bin]] name = "lsp"` |
| `aura.exe` 中的 `find_aura_debug_binary()` | → `find_aurad_binary()` | 搜索 `aurad.exe` |
| `aura.exe` 中的 `find_lsp_binary()` | → `find_lsp_binary()` | 搜索 `lsp.exe` |

### Step 4：清理 `aura.exe`

从 `cli/src/main.rs` 中删除已迁移到 `aurac` 和 `aurap` 的命令实现，只保留运行时命令：
- `run` / `eval` / `repl` / `debug` / `lsp` / `help`

### Step 5：更新 `print_usage`

更新 `aurac.exe`、`aura.exe`、`aurap.exe` 各自的 `--help` 输出。

### Step 6：更新文档和引用

需要更新的文档（`docs/` 下）：

| 文档 | 更新内容 |
|------|---------|
| `多进程与CLI架构分析报告.md` | 更新二进制结构表格（§1.1）、方案对比（§3）、实施计划（§4） |
| `Aura构建系统设计.md` | `aura` → `aurac` 构建命令引用 |
| `Aura调试器设计方案.md` | `aura-debug` → `aurad` |
| `LSP设计方案.md` | `aura-lsp` → `lsp` |
| 其他含命令示例的文档 | 检查并更新命令名称 |

### Step 7：更新 VS Code 扩展

- `ide-extension/vscode-extension/` 中的 LSP 路径引用
- `bin/aura-lsp.exe` → `bin/lsp.exe`

---

## 8. 预期收益

| 指标 | 现状（1 个大二进制） | 拆分后（3 个专职二进制） |
|------|---------------------|-------------------------|
| 最小编译 | `aura.exe` 需全量编译（~70 模块） | `aurap.exe` 只需链接 package+auz，体积最小 |
| 编译速度 | `aura check` 需编译全部 AOT/JIT/VM | `aurac check` 不含 AOT 后端（无 LLVM 依赖时） |
| 运行时体积 | VM + AOT + 包管理 + 文档全打包 | 各自最小化依赖集 |
| 错误隔离 | AOT 崩溃影响整个工具 | 崩溃隔离到单个二进制 |
| 并行编译 | 单二进制无法并行构建 | 三个二进制可并行构建 |

---

## 9. 命令速查表（最终版）

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        AuraLang 工具链                                  │
├───────────────┬─────────────────────────────────────────────────────────┤
│               │ build <file.aura> [--output <out>]                     │
│               │ build <file.aura> --aot [--output <exe>] [--target]    │
│               │ build <file.aura> --aot [--opt] [--emit-llvm] [--debug]│
│               │ build <file.aura> --aot [--shared] [--cabi]            │
│               │ build <file.aura> --aot-embed                          │
│               │ build <file.aura> --lib [--output <out>]               │
│               │ check <file.aura>                                       │
│  aurac.exe    │ disasm <file.auc> [--source <f.aura>]                   │
│  (编译器)      │ tokens <file.aura>                                      │
│               │ ast <file.aura>                                         │
│               │ fmt <file.aura> [--check]                               │
│               │ leak-check <file.aura>                                  │
│               │ stdlib-compile <core-dir> [--output <dir>]              │
│               │ export-header <file.aura> --out <name>.h                │
├───────────────┼─────────────────────────────────────────────────────────┤
│               │ run <file.aura> [--jit] [--stdlib-dir <dir>]            │
│  aura.exe     │ run <file.auc>                                          │
│  (运行时)      │ eval [--expr <code>]                                    │
│               │ repl                                                     │
│               │ debug <file.aura>       (→ aurad.exe)                    │
│               │ lsp                  (→ lsp.exe)                         │
├───────────────┼─────────────────────────────────────────────────────────┤
│               │ install [--offline]                                      │
│               │ update [--all]                                           │
│               │ publish [--dir <path>]                                   │
│               │ deps [--dir <path>] [--outdated]                         │
│               │ new <name> [--dir <path>]                                │
│  aurap.exe    │ package <file.aura> [--output <out>] [--sources]         │
│  (包管理器)    │ inspect <file.auz> [--verbose]                           │
│               │ verify <file.auz>                                        │
├───────────────┼─────────────────────────────────────────────────────────┤
│  lsp.exe      │ (LSP 协议服务，stdio 模式)                               │
│  (LSP 服务器)  │                                                         │
├───────────────┼─────────────────────────────────────────────────────────┤
│               │ <file.aura>              (VM 模式，默认)                  │
│               │ --mode jit <file.aura>  (JIT 模式)                      │
│               │ --mode aot <file.aura>  (AOT 模式)                      │
│  aurad.exe    │ --mode aot --launch <file.aura>                          │
│  (调试器)      │ 交互：break/continue/step/next/out/backtrace/list/print│
│               │        locals/stack/info/quit                            │
├───────────────┼─────────────────────────────────────────────────────────┤
│               │ build [--profile/--target/--parallel/--opt/--debug]      │
│               │ compile / test / run / check / clean / resolve / ci      │
│  loom.exe     │ watch / check-config / new / wrapper / ide / doc / fmt  │
│  (构建系统)    │ version                                                  │
└───────────────┴─────────────────────────────────────────────────────────┘
```
