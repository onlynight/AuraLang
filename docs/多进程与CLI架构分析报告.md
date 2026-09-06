# AuraLang 多进程与 CLI 架构分析报告

> **状态**：分析完成 → 阶段 1-3 开发中
> **日期**：2026-06-30
> **作者**：SenseNova

---

## 1. 现状分析

### 1.1 二进制结构

| 组件 | 产物 | 子命令 | 依赖关系 |
|------|------|--------|----------|
| `compiler` | Rust **库** crate | 词法/语法/语义/字节码/VM/LSP/包管理/文档/JIT/AOT | 被引用 |
| `cli` | **1 个 exe** (`aura`) | 21 个子命令 | 依赖 compiler |
| `loom` | **1 个 exe** (`loom`) | 13 个子命令 | 依赖 compiler |

**关键事实**：`cli/src/main.rs`（1328 行）通过 `match cmd { ... }` 分发 21 个子命令。编译 `aura.exe` 时，**整个 compiler crate 的全部 ~70 个模块**（含 VM、LSP、LLVM AOT、Actor 运行时、JIT 等）全部链接进一个二进制。

### 1.2 多进程隔离的实际状态

| 运行时组件 | 线程安全 | 多进程能力 |
|-----------|----------|-----------|
| VM (`Vm`) | ❌ 非线程安全 | ✅ 每次 `aura run` 是独立 OS 进程 |
| 原生函数调度 | ⚠️ thread_local 单 VM 指针 | ✅ 进程间天然隔离 |
| FFI 回调派发 | ⚠️ thread_local 单 dispatcher | ✅ 进程间天然隔离 |
| Actor 运行时 | ❌ 进程内内存 | ❌ **无法跨进程** |
| Channel 运行时 | ❌ 进程内缓冲区 | ❌ **无法跨进程** |
| 协程调度器 | ❌ 进程内快照 | ❌ **无法跨进程** |
| ThreadPool | ✅ Arc<Mutex<VecDeque>> | ✅ 但未被 VM 使用 |
| AOT 链接器 | ✅ 通过 Command 调外部 llc/clang | ✅ 天然多进程 |

---

## 2. 问题诊断

### 2.1 问题 A：单二进制膨胀

`aura.exe` 当前包含全部代码：前端 + 字节码 + VM + 运行时 + JIT + AOT + LSP + 包管理 + 文档 + ARC + 格式化。仅 `aura check` 也要加载全部代码。

### 2.2 问题 B：「多进程无法访问」的根因

**不是单二进制导致的**。每次调用 `aura.exe` 已经是独立 OS 进程。问题出在 **Aura 语言运行时本身**：

1. `ActorRuntime.actors: Vec<Option<Actor>>` — 普通 Rust Vec，无共享内存/IPC
2. `ChannelRuntime.channels: Vec<Option<Channel>>` — VecDeque<Value> 缓冲区，无 IPC
3. 无进程间通信机制（无 socket、管道、共享内存）
4. `set_vm_ref` 使用 `thread_local!`，同线程只能有一个活跃 VM

### 2.3 问题 C：`loom` 与 `aura` 的功能重叠

两套工具都有 `build`, `run`, `package`, `verify`, `install`, `publish`, `deps`, `new` 命令。

---

## 3. 方案对比

### 方案一：拆分为多个独立二进制（不推荐）

10+ 个独立二进制，维护成本高，用户入口不统一。

### 方案二：保留单二进制 + 修复运行时（推荐 ⭐）

不做二进制拆分，修复运行时层面的多进程问题。

### 方案三：混合方案（最终推荐 ⭐⭐）

只拆分 LSP（长驻服务器），其余保留。运行时改造为支持跨进程 IPC。

---

## 4. 分阶段实施计划

### 阶段 1：修复 VM thread-local（短期，1-2 天）

将 `thread_local!` 改为全局 `Mutex<Vec<...>>` 栈式注册表，支持嵌套 VM 执行和跨线程回调。

**改动文件**：
- `compiler/src/vm/native.rs`：VM_REF → 全局栈
- `compiler/src/vm/ffi.rs`：CURRENT_DISPATCHER → 全局栈

### 阶段 2：LSP 拆分为独立二进制（短期，1 天）

LSP 是唯一需要长驻的组件。新建 `aura-lsp` 二进制，`aura lsp` 命令改为 spawn 子进程。

**改动文件**：
- `cli/Cargo.toml`：添加 `[[bin]]` 目标
- `cli/src/lsp_main.rs`：新增 LSP 入口
- `cli/src/main.rs`：修改 `cmd_lsp`

### 阶段 3：Actor/Channel 跨进程 IPC（中期，1-2 周）

1. `Value` 实现 serde 序列化
2. 新增 `vm/ipc.rs`：基于 Unix Socket / Windows Named Pipe 的 IPC 层
3. 新增 `vm/channel_tcp.rs`：TCP/Socket 后端 Channel
4. 新增 `vm/actor_process.rs`：跨进程 Actor spawn

**新增文件**：
- `compiler/src/vm/ipc.rs`
- `compiler/src/vm/channel_tcp.rs`
- `compiler/src/vm/actor_process.rs`

### 阶段 4：职责边界收敛（长期，3-6 个月）

**核心原则**：loom 退化为纯构建系统，aura 保留语言工具 + 包生态的全部职责。对标 TypeScript 的 `tsc + npm + webpack` 三分模式，而非 Cargo 的"构建+生态合体"模式。判断依据：构建系统是"语言的上层消费者"（可第三方替换），生态是"语言的下游分发"（必须理解 .auz 编译产物），前者天然可拆、后者天然该合。

#### 4.1 loom 收缩（1-2 周）

删除 loom CLI 中的生态层命令，仅保留构建编排相关命令：

| 动作 | 命令 | 理由 |
|------|------|------|
| **删除** | `package` / `verify` / `install` / `publish` / `deps`（5 条） | 生态层职责归 aura，`.auz` 制品格式与注册表协议是 aura 产权 |
| **保留为别名** | `new` | 语义区分：`loom new` = 创建带构建配置的项目骨架；`aura new` = 创建包项目 |
| **保留** | `build` / `compile` / `test` / `run` / `clean` / `watch` / `check-config` / `resolve` / `ci` / `wrapper` / `version` | 纯构建编排职责 |

同步改动：
- 删除 `loom/src/registry/` 模块（local.rs / protocol.rs），生态层职责归 aura
- `loom/src/dep/resolver.rs` 降级为本地路径解析器，不再自带远端拉取，仅从 `~/.aura/cache/packages/` 读取已安装包路径
- `loom new` 保留为 `aura new` 的薄别名（内部 spawn `aura new` 或共享脚手架函数），帮助新用户从构建系统入口创建项目

#### 4.2 loom 与 aura 的对接协议（1 周）

- 明确 loom 调用 aura 编译的两种模式：**库链接**（当前默认，loom 依赖 compiler crate）/ **子进程调用** `aura` 二进制（备选，用于隔离或第三方构建场景）
- 定义 `.auz` 制品的读写 API 归属：aura 拥有 builder / reader，loom 仅作为调用方，不重新实现制品格式
- 约定 `~/.aura/cache/packages/` 为本地包缓存的唯一位置：loom 的 `resolve` 只读不写；aura 的 `install` / `update` 负责写入

#### 4.3 文档与示例清理（1 周）

- `docs/Aura构建系统设计.md` §20.1 删除 `mvn deploy → aura publish`、`mvn package → aura package` 等生态层映射，生命周期收缩为 clean → compile → test → run
- `loom/README.md` "特性"清单删除"仓库管理：REST API 注册表 + 本地注册表"条目
- `loom/README.md` "构建生命周期"从 clean → resolve → compile → test → package → verify → install → deploy 收缩为 clean → resolve → compile → test → run
- 附录 §20.3 的"等价"列只保留 `cargo build` / `cargo test` / `cargo run`，删除 `cargo publish`

#### 4.4 明确不做的事

- **不拆分 aura 的生态层为第三个二进制**：`.auz` 制品、aura.toml 包清单、注册表协议均属于 aura 产权；现阶段仅在库层面拆分（建议第四阶段之后做 `aura-ecosystem` crate 抽取，作为 `aura` 二进制和 loom 的公共依赖），不新增 `aupkg` 之类的独立二进制。触发条件：第三方包管理器需求 / 注册表规模化 / 二进制体积成为用户痛点 / aura.toml 字段膨胀。
- **不合并两个二进制的入口**：`aura build`（单文件 `<file.aura>`）与 `loom build`（项目级，读 aura.toml）共存，语义与参数均不同，类似 `rustc` 与 `cargo build` 的共存关系。
- **不改变 aura 的 21 条命令**：阶段 1-3 的运行时修复与 LSP 拆分不涉及 aura CLI 增删，第四阶段 aura 端保持原样。

---

## 5. 附录：关键代码位置

| 问题 | 文件 | 行号 |
|------|------|------|
| VM_REF thread_local | `compiler/src/vm/native.rs` | 360-381 |
| CURRENT_DISPATCHER thread_local | `compiler/src/vm/ffi.rs` | 74-92 |
| VM::run() 设置/清除 | `compiler/src/vm/mod.rs` | 684-730 |
| Actor 运行时 | `compiler/src/vm/actor.rs` | 52-64 |
| Channel 运行时 | `compiler/src/vm/channel.rs` | 29-36 |
| CLI 命令分发 | `cli/src/main.rs` | 26-65 |
| cmd_lsp | `cli/src/main.rs` | 591-594 |
| LSP 服务器 | `compiler/src/lsp.rs` | 全部 |
| loom 构建系统 | `loom/src/main.rs` | 全部 |
| AOT 链接器 | `compiler/src/codegen/aot/linker.rs` | 全部 |
| ThreadPool | `compiler/src/vm/thread_pool.rs` | 全部 |
