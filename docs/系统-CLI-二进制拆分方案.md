# AuraLang CLI 二进制拆分方案

> **状态**：方案已确认 → 待实施
> **日期**：2026-07-02
> **基线**：`多进程与CLI架构分析报告.md` §3 方案B

---

## 1. 目标

将当前 Rust `rust/cli/` crate 中**大而全的 `aura.exe`** 拆分为 **四个职责单一的二进制**，同时保持 `loom.exe`、`auralsp.exe`、`aurad.exe` 三个独立工具。Photon 原生编译后端从 `aura build -b photon` 中剥离，独立为 `photon.exe`。

同一套命名和职责拆分也适用于 Aura 自举 / 纯 Aura 实现后的工具链产物：最终对外工具名保持一致，只是编译路径从 Rust 产物切换为 Aura 源码经 `photon.exe` / `aurac.exe` 生成的产物。无过渡期，所有命令直接迁移到位。

| 现状 | 拆分后 |
|------|--------|
| `aura.exe`（~21 子命令） | `aurac.exe`（编译器） + `aura.exe`（运行时） + `aurap.exe`（包管理器） |
| `aura build -b photon` | `photon.exe`（Photon 原生编译器） |
| `aura-lsp.exe` | `auralsp.exe`（重命名） |
| `aura-debug.exe` | `aurad.exe`（重命名） |
| `loom.exe` | `loom.exe`（不变） |

---

## 2. 最终二进制矩阵

### 2.1 Rust 实现路径（当前实施基线）

```
AuraLang
├── rust/compiler/           (lib only)
├── rust/cli/
│   ├── src/main.rs           → aura.exe      (运行时)
│   ├── src/compiler_main.rs  → aurac.exe     (编译器)   [新增]
│   ├── src/package_main.rs   → aurap.exe     (包管理)   [新增]
│   ├── src/photon_main.rs    → photon.exe    (Photon 原生编译器)   [新增]
│   ├── src/lsp_main.rs       → auralsp.exe   (LSP，原 aura-lsp)
│   └── src/debugger_main.rs  → aurad.exe     (调试器，原 aura-debug)
└── rust/loom/
    └── src/main.rs           → loom.exe      (构建系统)
```

### 2.2 Aura 自举实现路径（目标形态）

```
AuraLang
├── compiler/                (核心库或标准库模块)
├── toolchain/cli/
│   ├── runtime/Main.aura     → aura.exe      (运行时)
│   ├── compiler/Main.aura    → aurac.exe     (编译器)
│   ├── package/Main.aura     → aurap.exe     (包管理)
│   ├── photon/Main.aura      → photon.exe    (Photon 原生编译器)
│   ├── lsp/Main.aura         → auralsp.exe   (LSP)
│   └── debugger/Main.aura    → aurad.exe     (调试器)
└── toolchain/loom/
    └── Main.aura             → loom.exe      (构建系统)
```

> Aura 自举路径中的二进制由 `aurac.exe` / `photon.exe` 编译产出；编译命令、命名、CLI 参数、退出码与 Rust 路径保持一致。

| 二进制 | 中文名 | 定位 | 依赖的 compiler 模块 |
|--------|--------|------|----------------------|
| `aurac.exe` | Aura 编译器 | 源码 → 字节码/AOT/IR/库 | lexer, parser, sema, codegen, vm(debug), lsp(fmt), docgen, auz, hir, mir, arc, lsp |
| `aura.exe` | Aura 运行时 | 字节码/源码 → 执行 | vm, codegen, compiler(lib), lsp |
| `aurap.exe` | Aura 包管理器 | 包安装/发布/验证 | package, auz, compiler(lib) |
| `photon.exe` | Photon 原生编译器 | 源码 → 原生可执行文件（不经 LLVM） | lexer, parser, sema, hir, mir, photon(HAT), compiler(lib) |
| `auralsp.exe` | Aura LSP | 编辑器协议服务 | lsp, compiler(lib) |
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
| `aura lsp` | 转发到 `auralsp.exe` | ✅（目标改名） |

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

### 3.4 `photon.exe` — Photon 原生编译器

> 职责：源码 → HAT 管线 → 原生可执行文件（不经 LLVM，纯 Aura 实现后端）
>
> 对标：`gcc` / `rustc`（但走完全独立的 HAT 后端路径）
>
> 后端管线：HIR → SSA → LIR → DAG → 寄存器分配 → x86_64 编码 → COFF → exe

| 命令 | 说明 | 原属 aura.exe |
|------|------|:-------------:|
| `photon build <file.aura> [--output <out.exe>]` | 完整管线编译为原生 exe | ✅（原 `aura build -b photon`） |
| `photon build <file.aura> [--output <out.exe>] [--debug]` | 生成 DWARF 调试信息 | ✅ |
| `photon build <file.aura> [--output <out.exe>] [--emit-llvm]` | 仅生成 LLVM IR（调试用） | — |
| `photon check <file.aura>` | 语法/语义检查（仅前端） | — |
| `photon run <file.aura>` | 编译并执行（等价 build + 执行） | — |

**photon.exe 不需要 VM 执行**，所以不链接 VM 运行时、JIT、Actor 运行时、Channel 运行时，也不包含 AOT LLVM 后端、包管理器、文档生成等模块，只链接 photon(HAT) 后端所需的 compiler(lib) 模块。

### 3.5 `auralsp.exe` — Aura LSP 服务器（原 `aura-lsp.exe` 重命名）

| 命令 | 说明 |
|------|------|
| `auralsp` | stdio 模式（默认） |
| `auralsp --port <port>` | TCP 模式（预留） |
| `auralsp --help, -h` | 帮助 |
| `auralsp --version, -V` | 版本 |

### 3.6 `aurad.exe` — Aura 调试器（原 `aura-debug.exe` 重命名）

| 命令 | 说明 |
|------|------|
| `aurad <file.aura>` | VM 模式调试（默认） |
| `aurad --mode jit <file.aura>` | JIT 模式调试 |
| `aurad --mode aot <file.aura>` | AOT 模式调试 |
| `aurad --mode aot --launch <file.aura>` | AOT + 外部调试器 |
| `aurad --help, -h` | 帮助 |
| `aurad --version, -V` | 版本 |

**交互命令**（在 `aurad` 内）：`break`、`continue`、`step/next/out`、`backtrace`、`list`、`print`、`locals/stack`、`info`、`quit`

### 3.7 `loom.exe` — Aura 构建系统（不变）

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

## 4. 各二进制产物配置

### 4.1 Rust 实现路径：`rust/cli/Cargo.toml`

```toml
[package]
name = "cli"
version.workspace = true
edition.workspace = true

# ── 拆分后的 6 个二进制 ──

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

# Photon 原生编译器：HAT 管线（HIR → SSA → LIR → DAG → Encode → COFF → exe）
[[bin]]
name = "photon"
path = "src/photon_main.rs"

# LSP：语言服务器（独立进程）
[[bin]]
name = "auralsp"
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

### 4.2 Aura 自举实现路径

Aura 路径不复用 Cargo `[[bin]]`，但应保持等价产物映射：

```toml
# aura.toml 建议配置（示例）
[toolchain.cli]
runtime = "toolchain/cli/runtime/Main.aura"
compiler = "toolchain/cli/compiler/Main.aura"
package = "toolchain/cli/package/Main.aura"
photon = "toolchain/cli/photon/Main.aura"
lsp = "toolchain/cli/lsp/Main.aura"
debugger = "toolchain/cli/debugger/Main.aura"
```

| Rust 入口 | Aura 入口 | 输出二进制 |
|-----------|-----------|------------|
| `rust/cli/src/compiler_main.rs` | `toolchain/cli/compiler/Main.aura` | `aurac.exe` |
| `rust/cli/src/main.rs` | `toolchain/cli/runtime/Main.aura` | `aura.exe` |
| `rust/cli/src/package_main.rs` | `toolchain/cli/package/Main.aura` | `aurap.exe` |
| `rust/cli/src/photon_main.rs` | `toolchain/cli/photon/Main.aura` | `photon.exe` |
| `rust/cli/src/lsp_main.rs` | `toolchain/cli/lsp/Main.aura` | `auralsp.exe` |
| `rust/cli/src/debugger_main.rs` | `toolchain/cli/debugger/Main.aura` | `aurad.exe` |

### 4.3 各二进制编译 feature 建议

| 二进制 | Rust 编译命令 | Aura 路径说明 |
|--------|---------------|---------------|
| `aurac` | `cargo build --features llvm` | 由 `photon.exe` 或 `aurac.exe` 编译，需启用 AOT/LLVM 工具链探测 |
| `aurac` | `cargo build --no-default-features` | 纯字节码编译器（最小体积） |
| `aura` | `cargo build` | 默认（VM + JIT） |
| `aura` | `cargo build --features llvm` | VM + JIT + AOT（完整） |
| `aurap` | `cargo build` | 无需 LLVM |
| `photon` | `cargo build` | 无需 LLVM（纯 HAT 管线） |
| `auralsp` | `cargo build` | 无需 LLVM |
| `aurad` | `cargo build --features llvm` | AOT 调试需要 LLVM |
| `aurad` | `cargo build` | VM/JIT 调试（无需 LLVM） |

---

## 5. 跨二进制调用关系

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                                用户直接调用                                   │
│                                                                              │
│  aurac.exe ← 源码编译    aura.exe ← 执行    aurap.exe ← 包管理              │
│  photon.exe ← Photon 原生编译                                                  │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│                                内部转发关系                                   │
│                                                                              │
│  aura.exe  ──→ 调用 aurad.exe     (debug 命令转发)                            │
│  aura.exe  ──→ 调用 auralsp.exe   (lsp 命令转发)                             │
│  loom.exe  ──→ 构建时调用 aurac.exe / aura.exe / photon.exe                  │
│  aurad.exe ──→ 编译时调用 AOT 后端（如需 AOT 调试）                            │
└──────────────────────────────────────────────────────────────────────────────┘
```

**转发搜索顺序**（与现有 `find_lsp_binary` / `find_aurad_binary` 一致）：

1. 当前可执行文件所在目录（`aurad.exe` / `auralsp.exe` / `aurac.exe` / `photon.exe`）
2. `PATH` 环境变量

---

## 6. `aura.exe` 中的转发命令更新

### 6.1 `aura debug` → `aurad.exe`

```rust
// 原 cmd_debug 中 find_aura_debug_binary()
// 改为 find_aurad_binary()，搜索 "aurad.exe" / "aurad"
```

### 6.2 `aura lsp` → `auralsp.exe`

```rust
// 原 cmd_lsp 中 find_lsp_binary()
// 改为 find_auralsp_binary()，搜索 "auralsp.exe" / "auralsp"
```

### 6.3 `aura build -b photon` → `photon.exe`

```rust
// 原 cmd_build 中 backend == "photon" 的分支
// 改为 find_photon_binary()，搜索 "photon.exe" / "photon"，并转发原始参数。
// 若 photon.exe 不存在，可直接执行内置 cmd_build_photon 作为兼容回退。
```

---

## 7. 实施步骤

### Step 1：创建新入口文件

```
rust/cli/src/compiler_main.rs   → aurac.exe 入口
rust/cli/src/package_main.rs    → aurap.exe 入口
rust/cli/src/photon_main.rs     → photon.exe 入口
```

从 `rust/cli/src/main.rs` 中提取对应命令的函数到三个新文件中，共享的辅助函数（`extract_opt`、`first_positional`、`default_output`、`default_output_base`）提取到 `rust/cli/src/util.rs`。

### Step 2：更新 Cargo.toml

在 `rust/cli/Cargo.toml` 中添加 / 调整 `[[bin]]` 条目：

- `name = "aurac"` → `src/compiler_main.rs`
- `name = "aura"` → `src/main.rs`
- `name = "aurap"` → `src/package_main.rs`
- `name = "photon"` → `src/photon_main.rs`
- `name = "auralsp"` → `src/lsp_main.rs`（原 `aura-lsp`）
- `name = "aurad"` → `src/debugger_main.rs`（原 `aura-debug`）

### Step 3：重命名二进制

| 原名 | 新名 | 改动 |
|------|------|------|
| `aura-debug` | `aurad` | `[[bin]] name = "aurad"` |
| `aura-lsp` | `auralsp` | `[[bin]] name = "auralsp"` |
| `aura.exe` 中的 `find_aura_debug_binary()` | `find_aurad_binary()` | 搜索 `aurad.exe` |
| `aura.exe` 中的 `find_lsp_binary()` | `find_auralsp_binary()` | 搜索 `auralsp.exe` |
| `aura build -b photon` | `photon` 或 `find_photon_binary()` 转发 | 搜索 `photon.exe` |

### Step 4：拆分 `aura.exe`

从 `rust/cli/src/main.rs` 中删除已迁移到 `aurac`、`aurap`、`photon` 的命令实现，只保留运行时命令：

- `run` / `eval` / `repl` / `debug` / `lsp` / `help`

`build -b photon` 建议保留薄转发：

```
aura build -b photon <args...> → photon build <args...>
```

### Step 5：更新 `print_usage`

更新 `aurac.exe`、`aura.exe`、`aurap.exe`、`photon.exe`、`auralsp.exe`、`aurad.exe` 各自的 `--help` 输出。

### Step 6：更新文档和引用

需要更新的文档（`docs/` 下）：

| 文档 | 更新内容 |
|------|---------|
| `多进程与CLI架构分析报告.md` | 更新二进制结构表格（§1.1）、方案对比（§3）、实施计划（§4） |
| `Aura构建系统设计.md` | `aura` → `aurac` 构建命令引用；新增 `photon` 原生编译路径 |
| `Aura调试器设计方案.md` | `aura-debug` → `aurad` |
| `LSP设计方案.md` | `aura-lsp` → `auralsp` |
| 其他含命令示例的文档 | 检查并更新命令名称 |

### Step 7：更新 VS Code 扩展和其他客户端

- `ide-extension/vscode-extension/` 中的 LSP 路径引用
- `bin/aura-lsp.exe` → `bin/auralsp.exe`
- Sublime / Vim / Emacs 等客户端默认 `lsp_binary` 改为 `auralsp` 或 `auralsp.exe`

---

## 8. 预期收益

| 指标 | 现状（1 个大二进制 + 附属工具） | 拆分后（4 个专职二进制 + 3 个独立工具） |
|------|-------------------------------|----------------------------------------|
| 最小编译 | `aura.exe` 需全量编译（~70 模块） | `aurap.exe` 只需链接 package+auz，体积最小 |
| 编译速度 | `aura check` 需编译全部 AOT/JIT/VM | `aurac check` 不含 AOT 后端；`photon` 不走 LLVM |
| 运行时体积 | VM + AOT + 包管理 + 文档全打包 | 各自最小化依赖集 |
| 错误隔离 | AOT/Photon 崩溃影响整个工具 | 崩溃隔离到单个二进制 |
| 并行编译 | 单二进制无法并行构建 | 多个二进制可并行构建 |
| 自举边界 | Photon 逻辑混在 CLI 中 | Photon 后端有独立 CLI、测试入口、发布产物 |

---

## 9. 命令速查表（最终版）

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                            AuraLang 工具链                                   │
├───────────────┬──────────────────────────────────────────────────────────────┤
│               │ build <file.aura> [--output <out>]                          │
│               │ build <file.aura> --aot [--output <exe>] [--target]         │
│               │ build <file.aura> --aot [--opt] [--emit-llvm] [--debug]     │
│               │ build <file.aura> --aot [--shared] [--cabi]                 │
│               │ build <file.aura> --aot-embed                              │
│               │ build <file.aura> --lib [--output <out>]                    │
│               │ check <file.aura>                                           │
│  aurac.exe    │ disasm <file.auc> [--source <f.aura>]                        │
│  (编译器)      │ tokens <file.aura>                                          │
│               │ ast <file.aura>                                             │
│               │ fmt <file.aura> [--check]                                   │
│               │ leak-check <file.aura>                                      │
│               │ stdlib-compile <core-dir> [--output <dir>]                  │
│               │ export-header <file.aura> --out <name>.h                    │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ run <file.aura> [--jit] [--stdlib-dir <dir>]                 │
│  aura.exe     │ run <file.auc>                                               │
│  (运行时)      │ eval [--expr <code>]                                        │
│               │ repl                                                         │
│               │ debug <file.aura>       (→ aurad.exe)                        │
│               │ lsp                  (→ auralsp.exe)                         │
│               │ build -b photon ...      (→ photon.exe)                      │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ install [--offline]                                          │
│               │ update [--all]                                               │
│               │ publish [--dir <path>]                                       │
│               │ deps [--dir <path>] [--outdated]                             │
│               │ new <name> [--dir <path>]                                    │
│  aurap.exe    │ package <file.aura> [--output <out>] [--sources]             │
│  (包管理器)    │ inspect <file.auz> [--verbose]                               │
│               │ verify <file.auz>                                            │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ build <file.aura> [--output <out.exe>]                       │
│               │ build <file.aura> [--output <out.exe>] [--debug]             │
│               │ build <file.aura> [--output <out.exe>] [--emit-llvm]         │
│  photon.exe   │ check <file.aura>                                            │
│  (原生编译器)   │ run <file.aura>                                             │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ stdio 模式（默认）                                            │
│  auralsp.exe  │ --port <port>（TCP 预留）                                     │
│  (LSP 服务器)  │ --help / --version                                          │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ <file.aura>              (VM 模式，默认)                      │
│               │ --mode jit <file.aura>  (JIT 模式)                          │
│               │ --mode aot <file.aura>  (AOT 模式)                          │
│  aurad.exe    │ --mode aot --launch <file.aura>                              │
│  (调试器)      │ 交互：break/continue/step/next/out/backtrace/list/print     │
│               │        locals/stack/info/quit                               │
├───────────────┼──────────────────────────────────────────────────────────────┤
│               │ build [--profile/--target/--parallel/--opt/--debug]          │
│               │ compile / test / run / check / clean / resolve / ci          │
│  loom.exe     │ watch / check-config / new / wrapper / ide / doc / fmt       │
│  (构建系统)    │ version                                                      │
└───────────────┴──────────────────────────────────────────────────────────────┘
```
