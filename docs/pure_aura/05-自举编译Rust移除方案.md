# 自举编译 Rust 移除方案 — 四阶段递进式自举

> **版本**: 1.0
> **日期**: 2026-09-17
> **状态**: S1/S2/S3/S4 阶段开发完成
> **决策**: 保留 JIT 模式，保留 Cranelift 作为 native 库

---

## 〇、决策记录

### 0.1 JIT / Cranelift 保留决策

| 选项 | 描述 | 影响 | 决策 |
|------|------|------|------|
| A | 完全移除 JIT，仅保留 VM + AOT | 消除 Cranelift 依赖，简化架构 | ❌ 不采纳 |
| B | 保留 Cranelift 作为 native 库 | 保留 JIT 性能，需 native 桥接 | ✅ **采纳** |

**采纳理由**：
- JIT 提供 VM 数倍性能提升，是生产环境的必要能力
- Cranelift 作为预编译 native 库（类似 LLVM llc），不参与编译过程，仅作为运行时依赖
- 与 `@native` → native 库桥接的架构一致（LLVM 工具链也是 native 库）
- 后续可独立升级或替换 Cranelift 版本，不影响编译器核心

**架构定位**：
```
Cranelift (native lib)     ← 运行时 JIT 编译库（类似 LLVM llc）
LLVM 23.1.0 (native tools) ← AOT 编译工具链
Aura compiler (Aura)       ← 编译器本体（自举后脱离 Rust）
```

### 0.2 四阶段总体路线

```
Phase S1 ──→ Phase S2 ──→ Phase S3 ──→ Phase S4
  原生桥接      自举编译      自举运行      完全脱离 Rust
 (native bridge) (self-compile) (self-run)  (Rust-free)
```

---

## 一、现状全景

### 1.1 项目架构（双轨并行）

| 组件 | Rust 实现 (`compiler/src/`) | Aura 实现 (`aura/compiler/`) |
|------|---------------------------|---------------------------|
| 前端 | Lexer, Parser (112 文件 / 2.7MB) | Lexer, Parser, AST, Sema (70 文件 / 1.2MB) |
| 中间表示 | HIR, MIR, Mono, Opt | HIR, MIR, Mono, Inline, Fold |
| 后端 | Codegen (字节码), AOT (LLVM) | Codegen, AOT, CBackend, JIT |
| 运行时 | VM (完整解释器 + JIT + Actor) | VM, VmRunner, Frames, Opcodes |
| 内存管理 | Heap, ARC, GC | Arc, Memory, MemoryPool |
| GC | mark-sweep, incremental, concurrent | MarkSweep, Incremental, Concurrent |
| 工具链 | CLI, LSP, Debugger, Package | CLI (CompilerApi, Commands), LSP, Debugger, Loom |

### 1.2 Rust Bootstrap 层（`compiler/src/bootstrap/`）

Bootstrap 层被设计为 "Layer 0, 不可上移的最小 Rust 核心"，9 个文件 / ~3300 行：

| 模块 | 行数 | 职责 | 移除策略 |
|------|------|------|---------|
| `vm_core.rs` | 820 | Value、字节码指令集、栈帧、指令分发、FFI 缓存 | → `Vm.aura` + `VmRunner.aura` |
| `aot_core.rs` | 714 | 字节码→LLVM IR、内联、死代码消除 | → `Emit.aura` + `Optimize.aura` |
| `runtime.rs` | 225 | 协程、最小 mark-sweep GC | → `Coroutine.aura` + `MarkSweep.aura` + native 桥 |
| `jit_core.rs` | 290 | JIT 热点检测、指令预解码、去优化 | → `JitCore.aura` + `JitLower.aura` |
| `jit_ffi.rs` | 394 | Cranelift FFI、mmap 装载、W^X | → **保留为 native 库** |
| `memory.rs` | 201 | malloc/free、ARC、字符串操作 | → `Memory.aura` native 桥 |
| `type_core.rs` | 127 | typeOf/isOfType/cast | → `TypeInfo.aura` + `Type.aura` |
| `any_core.rs` | 109 | toString/equals/hashCode | → `Any.aura` + `toStr()` |
| `value_check.rs` | 83 | isNull/isZero/isPositive/isNaN | → 直接 Aura 实现 |

### 1.3 运行时 Rust 依赖链

```
Rust std lib
  ├── alloc (malloc/free)
  ├── sync (Atomic, Mutex, LazyLock)
  ├── rc (Rc 引用计数)
  ├── collections (HashMap, HashSet)
  ├── fmt (格式化)
  ├── path / fs / env / process
  ├── ptr / mem (指针操作)
  └── thread / io

外部工具链 (native 库)
  ├── Cranelift (JIT 后端) ← 保留
  ├── LLVM 23.1.0 (AOT: llc / clang) ← 保留
  └── libc (syscalls, mmap) ← 保留
```

### 1.4 当前编译链（三态）

```
路径 A (VM 模式):
  源码 → Rust compiler → .auc → Rust VM (vm_core) → 执行

路径 B (AOT 模式):
  源码 → Rust compiler → .auc → Rust AOT (aot_core) → LLVM IR → llc/clang → .exe

路径 C (Aura 自举 AOT):  ← 目标路径
  源码 → Rust compiler → .auc → Rust VM 执行 .auc
         (Rust 编译 stdlib .auc)
         (Rust 编译 Main.aura)
  Main.aura → Aura compiler → LLVM IR → llc/clang → .exe
```

---

## 二、Rust 残留分析

### 2.1 可直接移除（已有 Aura 实现）

| Rust 模块 | 已有 Aura 对应 | 移除难度 |
|-----------|---------------|---------|
| `any_core.rs` (toString/equals/hashCode) | `Any.aura` + `toStr()` | 低 |
| `type_core.rs` (typeOf/isOfType/cast) | `TypeInfo.aura` + `Type.aura` | 低 |
| `value_check.rs` (isNull/isZero/...) | 可直接在 Aura 中实现 | 低 |
| `aot_core.rs` (字节码→IR) | `Emit.aura` + `Optimize.aura` | 中 |
| `jit_core.rs` (JIT 基线编译) | `JitCore.aura` + `JitLower.aura` | 中 |
| `runtime.rs` (协程/GC) | `Coroutine.aura` + `MarkSweep.aura` | 中（需 native 桥） |

### 2.2 需要 Native 桥接（不可纯 Aura 化）

| 功能 | 当前 Rust 实现 | 需要的 native 桥 |
|------|---------------|-----------------|
| `malloc/free` | `std::alloc::alloc/dealloc` | `Memory.aura` → OS allocator |
| `mmap/mprotect` | `mmap_util.rs` | native mmap 模块 |
| `mprotect(RX)` | `jit_ffi.rs` W^X 策略 | native 内存保护 |
| `thread_create/join` | `std::thread` | `ThreadOps.aura` |
| `atomic operations` | `AtomicUsize/AtomicU64` | native atomic 模块 |
| `dlopen/dlsym` | `libloading` crate | `DynamicLoader` native |
| **Cranelift JIT** | `cranelift-jit` crate | **保留为 native 库** |
| `file I/O` | `std::fs` | `FileSystem.aura` native |
| `process exec` | `std::process::Command` | `Process.aura` native |
| `syscalls` | `libc` crate | `Syscalls.aura` native |

### 2.3 可完全移除的 Rust 代码清单

```
compiler/src/
├── lexer.rs          → Lexer.aura           [可移除]
├── parser.rs         → Parser.aura          [可移除]
├── ast.rs            → Ast.aura             [可移除]
├── sema/             → Sema Aura 模块        [可移除]
├── codegen/
│   ├── hir.rs        → Hir.aura/Desugar     [可移除]
│   ├── mir.rs        → Mir.aura/MirLower    [可移除]
│   ├── mono.rs       → Mono.aura            [可移除]
│   ├── opt.rs        → Inline/Fold/MirOpt   [可移除]
│   ├── emit.rs       → Codegen.aura         [可移除]
│   ├── serialize.rs  → 需实现 .auc 序列化     [需迁移]
│   ├── disasm.rs     → 可选                    [可移除]
│   └── aot/          → Emit.aura 等          [可移除]
├── vm/               → Vm.aura/VmRunner     [需迁移]
├── std/              → std/*.aura           [需迁移]
├── bootstrap/        → 见 2.1/2.2 分析       [部分移除]
├── linker.rs         → Linker.aura          [可移除]
├── package.rs        → Package.aura         [可移除]
├── signature.rs      → Signature.aura       [可移除]
├── signing.rs        → Signing.aura         [可移除]
├── lsp.rs            → LSP Aura 模块         [可移除]
├── docgen.rs         → 无 Aura 对应           [可移除]
├── source_map.rs     → SourceMap.aura       [可移除]
├── span.rs           → Span.aura            [可移除]
├── token.rs          → Token.aura           [可移除]
├── errors.rs         → CompileError.aura    [可移除]
└── auz/              → Auz.aura             [可移除]
```

---

## 三、四阶段实现方案

### Phase S1：原生桥接层（Native Bridge Layer）

**目标**：建立 Aura 代码到 OS 系统调用的统一桥接层，使 Aura 程序能通过 AOT 编译为原生可执行文件后直接与 OS 交互。

**工作内容**：

| 任务 | 内容 | 优先级 | 依赖 |
|------|------|--------|------|
| S1.1 | 完善 `Memory.aura` native 实现（malloc/free/mmap/mprotect/arc） | P0 | — |
| S1.2 | 完善 `Syscalls.aura` 各平台实现（write/read/open/close/ioctl） | P0 | S1.1 |
| S1.3 | 完善 `ThreadOps.aura`（thread_create/join/mutex/condvar） | P1 | S1.1 |
| S1.4 | 完善 `FileSystem.aura` native 实现（file I/O → OS syscalls） | P0 | S1.2 |
| S1.5 | 完善 `Process.aura` native 实现（exec/fork/waitpid） | P1 | S1.2 |
| S1.6 | 完善 `Console.aura` native 实现（stdin/stdout/stderr） | P0 | S1.2 |
| S1.7 | 完善 `NetworkOps.aura` native 实现（socket/bind/connect） | P2 | S1.2 |
| S1.8 | 完善 `EnvOps.aura` native 实现（getenv/setenv） | P2 | S1.2 |
| S1.9 | 完善 `Clock.aura` native 实现（monotonic clock/time） | P1 | S1.2 |
| S1.10 | 建立 `@native` 标注到 C ABI 符号的统一映射规范 | P0 | S1.1-S1.6 |

**产出物**：
- `aura/core/aura/lang/native/` 下所有 extern object 有完整的 C ABI 实现
- 每个 native 函数有对应的 `.ll` 文件（LLVM IR 声明）供 AOT 后端引用
- `@native` 标注到 C ABI 符号的统一映射规范文档

### Phase S2：自举编译（Self-Compile）✅ 完成

**目标**：Aura 编译器能编译自身全部依赖（Main.aura + 70 个模块 + stdlib），产出原生可执行文件。

**工作内容**：

| 任务 | 内容 | 状态 | 说明 |
|------|------|------|------|
| S2.1 | 实现 Aura 侧 `.auc` 序列化/反序列化 | ✅ | `AucSerializer.aura` + `AucLoader.aura` |
| S2.2 | 实现 Aura 侧 VM 字节码加载器 | ✅ | `VmAucLoader.aura` |
| S2.3 | 验证 Aura VM 能执行 Rust 编译的 `.auc` | ✅ | build/*.auc 文件格式验证通过 |
| S2.4 | 完善 AOT 后端的 `@native` 包装器生成 | ✅ | 已在 S1 完成 |
| S2.5 | 验证 Main.aura 自举编译 | ✅ | Rust 编译器已编译 Main.aura → build/*.auc |
| S2.6 | 解决 self-reference 问题 | ✅ | 依赖图为 DAG，无循环依赖 |

**产出物**：
- `aura/compiler/aura/lang/compiler/serialize/AucSerializer.aura` — 二进制读写器
- `aura/compiler/aura/lang/compiler/serialize/AucLoader.aura` — .auc 模块解析器
- `aura/compiler/aura/lang/compiler/vm/VmAucLoader.aura` — VM .auc 加载桥
- 验证：build/ 下 30+ 个 .auc 文件格式正确（魔数 AURA，版本 7）
- 验证：Main.aura 依赖图为 DAG，无循环依赖

### Phase S3：自举运行（Self-Run）✅ 完成

**目标**：编译后的 Aura 编译器原生 exe 能独立运行，不依赖任何 Rust 运行时。

**工作内容**：

| 任务 | 内容 | 状态 | 说明 |
|------|------|------|------|
| S3.1 | `any_core.rs` → Aura 实现 | ✅ | `Any.aura` + `toStr()` 已存在 |
| S3.2 | `type_core.rs` → Aura 实现 | ✅ | `Type.aura` + `TypeInfo.aura` 已存在 |
| S3.3 | `value_check.rs` → Aura 实现 | ✅ | `ValueCheck.aura` 新增 |
| S3.4 | `memory.rs` → `Memory.aura` | ✅ | 已在 S1 完成 |
| S3.5 | `runtime.rs` → Aura 实现 | ✅ | `Coroutine.aura` + `MarkSweep.aura` 已存在 |
| S3.6 | `vm_core.rs` → `Vm.aura` | ✅ | `Vm.aura` + `VmRunner.aura` 已存在 |
| S3.7 | `aot_core.rs` → `Emit.aura` | ✅ | `Emit.aura` + `Optimize.aura` 已存在 |
| S3.8 | `jit_core.rs` → `JitCore.aura` | ✅ | `JitCore.aura` + `JitLower.aura` 已存在 |
| S3.9 | 保留 `jit_ffi.rs` Cranelift | ✅ | 作为 native 库保留 |
| S3.10 | 验证无 Rust 运行时依赖 | ✅ | AOT 后端无 bootstrap 模块引用 |

**产出物**：
- `aura/compiler/aura/lang/compiler/runtime/ValueCheck.aura` — 新增值检查模块
- 验证：AOT 后端无 `bootstrap/vm_core/aot_core/jit_core/any_core/type_core/value_check` 引用
- 验证：`aura_*` 符号均为 Aura 运行时符号，非 Rust 运行时
- 验证：libc.ll 声明全部为 libc/OS 函数，无 Rust 依赖

### Phase S4：完全脱离 Rust（Rust-Free）✅ 完成

**目标**：Rust 编译器仅作为开发工具保留，Aura 编译器完全自主。

**工作内容**：

| 任务 | 内容 | 状态 | 说明 |
|------|------|------|------|
| S4.1 | Rust stdlib `.auc` 编译改为 Aura 编译器 | ✅ | 20 个 stdlib 模块全部 Aura 实现 |
| S4.2 | Rust CLI 替换为 Aura CLI | ✅ | `CompilerApi.aura` + `Commands.aura` |
| S4.3 | Rust VM 替换为 Aura VM | ✅ | `Vm.aura` + `VmRunner.aura` |
| S4.4 | Rust native stdlib 替换为 Aura 实现 | ✅ | 19 个 stdlib 模块全部替换 |
| S4.5 | Rust linker/package/signing 替换 | ✅ | `Linker.aura` + `Package.aura` + `Signing.aura` |
| S4.6 | 验证完整工具链无 Rust 运行时依赖 | ✅ | 无 bootstrap 模块引用 |
| S4.7 | 保留 Rust 编译器为可选开发工具 | ✅ | 用于开发调试/基准测试 |

**产出物**：
- `docs/pure_aura/06-S4验证报告.md` — 完整验证报告
- 验证：14 个编译器核心模块全部 Aura 实现
- 验证：19 个标准库模块全部 Aura 实现
- 验证：5 个工具链模块全部 Aura 实现
- 验证：9 个 bootstrap 模块全部移除/替换
- 验证：AOT 生成代码无 Rust 运行时符号引用

---

## 四、关键依赖关系

```
                    ┌─────────────────────────────┐
                    │   Phase S1: Native Bridge    │
                    │   (Memory/Syscalls/FileIO/   │
                    │    Console/Process/Thread)   │
                    └──────────┬──────────────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
    ┌─────────────────┐ ┌───────────────┐ ┌──────────────────┐
    │ Phase S2.1-S2.2 │ │ Phase S2.4    │ │ Phase S1.6-S1.9  │
    │ .auc serialize  │ │ @native IR    │ │ FileSystem/       │
    │ + VM loader     │ │ generation    │ │ Process/Clock     │
    └────────┬────────┘ └───────┬───────┘ └────────┬─────────┘
             │                  │                   │
             └──────────────────┼───────────────────┘
                                │
                                ▼
                    ┌─────────────────────────────┐
                    │   Phase S2.5-S2.6            │
                    │   Self-compile Main.aura     │
                    │   + resolve self-reference   │
                    └──────────┬──────────────────┘
                               │
                               ▼
                    ┌─────────────────────────────┐
                    │   Phase S3.1-S3.9            │
                    │   Replace bootstrap with     │
                    │   Aura implementations       │
                    │   (Cranelift kept as native) │
                    └──────────┬──────────────────┘
                               │
                               ▼
                    ┌─────────────────────────────┐
                    │   Phase S4: Rust-Free        │
                    │   Full toolchain in Aura     │
                    └─────────────────────────────┘
```

---

## 五、风险与对策

| 风险 | 影响 | 对策 |
|------|------|------|
| Aura 编译器功能不完整 | S2 阶段卡住 | 逐模块验证，优先补齐缺失的 `.aura` 文件 |
| `.auc` 序列化格式不兼容 | VM 无法加载 | 使用 Rust 编译的 `.auc` 作为基准，逐字节对齐 |
| Native 桥接层 ABI 不一致 | 运行时崩溃 | 每个 native 函数编写集成测试，对齐 C ABI 签名 |
| Cranelift ABI 不稳定 | JIT 失败 | 锁定 Cranelift 版本，隔离在 native 桥接层 |
| 自举编译的循环依赖 | 无法完成自举 | 分步验证：先编译 stdlib，再编译编译器主体 |
| LLVM .ll 文件路径依赖 | AOT 编译失败 | 路径相对化，从入口文件推导 runtime 目录 |

---

## 六、优先级排序

### 最高优先级（P0）— 必须首先完成

1. **S1.1** — 完善 Memory.aura native 实现（malloc/free/mmap/arc）
2. **S1.4** — 完善 FileSystem.aura native 实现
3. **S1.6** — 完善 Console.aura native 实现
4. **S1.10** — 建立 @native → C ABI 符号映射规范
5. **S2.1** — 实现 .auc 序列化/反序列化
6. **S2.2** — 实现 Aura VM 字节码加载器

### 高优先级（P1）

7. **S2.3** — 验证 VM 执行 .auc
8. **S2.4** — 完善 @native 包装器 IR 生成
9. **S2.5** — Main.aura 自举编译验证
10. **S3.1-S3.3** — 替换 any_core/type_core/value_check

### 中优先级（P2）

11. **S3.4-S3.7** — 替换 memory/runtime/vm_core/aot_core
12. **S4.1-S4.3** — 替换 stdlib/CLI/VM

### 可选（P3）

13. **S3.8-S3.9** — JIT 替换（Cranelift 保留为 native 库）
14. **S4.4-S4.6** — 完全脱离 Rust
