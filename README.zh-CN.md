# Aura 编程语言

> 为 NovaOS 从零构建的系统级脚本语言 —— Rust 实现、Kotlin 风格语法、AOT + JIT 混合编译、零开销 FFI、ARC 内存管理，以及一条**直接生成 COFF 的原生后端（HAT / Photon）**—— 不经 LLVM IR 即可产出原生可执行文件。
>
> **English** → [README.md](README.md)

详细设计见 [系统-Aura构建系统设计.md](docs/系统-Aura构建系统设计.md) · [编译-编译器后端方案.md](docs/编译-编译器后端方案.md) · [集成-Aura-DSH-Integration-Plan.md](docs/集成-Aura-DSH-Integration-Plan.md)

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
| P17 | HAT / Photon 原生后端（不经 LLVM IR，直接生成 COFF） | ✅ —— 见[下方](#原生后端--hat--photon) |

---

## 原生后端 — HAT / Photon

第三条、**不经过 LLVM IR** 的编译路径。它不生成 `.ll`：后端把中间码降到已分配寄存器的线性 IR，手工
编码 x86-64 机器码，组装 COFF 目标文件，交给 `lld-link` 生成最终映像。

```
源码 (.aura)
   │
   ▼
AotModuleLinker ──► HirProgram          （模块合并、std prelude 内联）
   │
   ▼
SsaBuilder ────────► SSA MIR            （SSA + phi 重建）
   │
   ▼
HatSerializer ─────► .hat              （文本 IR，可往返、可读）
   │
   ▼  HATParser
   ├─ Phase B: SSA MIR → LIR                 (Lowering.aura)
   ├─ Phase C: LIR → Machine DAG             (InstructionSelection.aura)
   ├─ Phase D: 寄存器分配 + 窥孔              (RegisterAllocator.aura, PeepholeOptimizer.aura)
   └─ Phase E: X86 编码 → COFF → 链接        (X86Encoder.aura, PhotonObjectWriter.aura, PhotonSystemLinker.aura)
                             │
                             ▼
                      原生 .exe        （COFF 64 位，x86_64-pc-windows-msvc）
```

HAT IR 是一等公民的文本表示（`aura/lang/compiler/hir/hat/`）：程序可以被编译为 `.hat`、人工检查或
修改、再重新编译 —— 在这条路径上 HAT 才是真正的中间产物，不是调试转储。

| 组件 | 状态 |
|------|------|
| SSA 构造器 + phi 重建 | ✅ |
| HAT 序列化 / 反序列化（可往返） | ✅ |
| LIR 降级 | ✅ |
| 机器 DAG + 指令选择 | ✅ |
| 寄存器分配（x86-64） | ✅ |
| 窥孔 + 活跃性分析 | ✅ |
| X86-64 编码器 + 重定位发射 | ✅ |
| COFF 目标文件写入 | ✅ |
| 系统链接（`lld-link`，`/NODEFAULTLIB`） | ✅ |
| 系统调用发射（`NtCreateFile` / `NtWriteFile` / `NtReadFile` / `NtMapViewOfFile`） | ✅ |
| 运行库（`println`、`strcat`、`toInt`、`streq`、`heapArena`、`__list_*`、syscall） | ✅（子集） |

### 差分测试套件

每个用例被**编译并执行两次** —— 一次走 HAT，一次走纯 VM —— 然后比对输出。唯一参照物是 VM 的
stdout；HAT 链路既不自产也不读取 `.phir`。

| 套件脚本 | 前端 | 后端 | 结果 |
|----------|------|------|------|
| `scripts\photon-hat-native-suite.ps1`（默认） | 原生自举编译器（`build\hat-native\PhotonHatCompile.exe`） | VM 侧 HAT 消费者（`PhotonHatBuild.aura`） | **15/15** |
| `scripts\photon-hat-native-suite.ps1 -NativeAll` | 同上 | 原生 Photon 后端 | **13/15** —— `05_functions`、`02_fibonacci` 丢失用户函数调用的返回值（`add(3,4) = 0`） |
| `scripts\photon-hat-suite.ps1` | 种子 VM（PHIR → HIR → SSA → `.hat`） | HAT 后端 | **15/15** |

用例：

```
P1  hello / 变量 / 算术 / 控制流 / 函数              5 例
P2  嵌套循环 / 斐波那契 / 数组 / 字符串              4 例
P3  系统调用: exit, NtCreateFile, NtWriteFile, mmap, read, write  6 例
```

`tests/photon/P1..P4` 存放端到端差用例；`tests/photon/S1..S4` 存放后端单元/集成测试
（x86 编码器、重定位、指令选择、寄存器分配器、发射器、目标文件写入、管线集成）。

> `-OutRoot` 必须传**相对**路径 —— 脚本内部会把它拼到仓库根目录上。

### 自举编译器（进行中）

`scripts\photon-hat-bootstrap.ps1` 用 HAT 链路直接编译 `aura/compiler/aura/lang/compiler/Main.aura`
（即 Aura 编写的编译器自身）。当前进展：

```
[hat-front] modules=111 hirNodes=155887
[hat-front] ssa functions=18 values=239 blocks=51
  .hat                 8,994 字符       （HatParser 往返一致）
  LIR functions=18     DAG nodes=152 instrs=234
  spillSlots=7         机器码字节数=1,483    COFF 目标=2,919 B
  wall clock 79 s      峰值工作集 324.8 MB     （lld-link 峰值 17.8 MB）
  lld-link → 9 个未解析符号
```

链接之前所有阶段都已跑通。剩余 9 个符号分三类：

| 类别 | 符号 | 工作量 |
|------|------|--------|
| 运行库缺符号 | `charCodeAt` | 小 —— `mapStdlibFuncName` 加一项 + 导出 `movzx eax, byte ptr [rcx+rdx]` |
| `object` 方法未降级 | `cliUsage`、`runSelfTest`、`runCli` | 中 —— `lowerTypeDecl` 把 `object` 方法塞进 `HirObject` 下的 `HirBlock`；`SsaBuilder` 需要沿 `HirObject → HirBlock → HirFunction` 下钻（`Emit.aura` 已经通过按 kind 全 arena 扫描做到） |
| `class` 实例 / 虚表 | `VmRunner`、`loadAucAndRun`、`getReturnValue`、`getError`、`getInstructionCount` | 大 —— HAT 运行库没有对象头、没有 vtable、没有 `HashMap`/`ArrayList` 语义 |

完整分析：[`build/hat-bootstrap/REPORT-HAT-bootstrap.md`](build/hat-bootstrap/REPORT-HAT-bootstrap.md)。

> **本轮解开的根因（已修复）**：`SsaBuilder.changedVarsCsv` 用
> `while (start >= 0 && after.charCodeAt(start) != 10)` 反向扫描字符串。AOT 后端把
> `String.charCodeAt(i)` 降级为**无边界检查的内联访存**（`getelementptr i8, i8*, i64 i` +
> `load i8`，见 `aot/Emit.aura:6922`），因此绕过了 `core/aura/lang/String.aura` 里的 `index < 0`
> 保护。当 `start` 走到 -1 时扫描读到缓冲区之前：要么读到垃圾导致扫描不终止（表现为「卡住」），
> 要么 `0xC0000005`。现在扫描在结构上只读 `>= 0` 的下标。
> **该后端的写码规则**：不要指望 `&&` 短路来保护 `charCodeAt` —— 必须让越界下标路径结构上不可达。

---

## 纯 Aura 编译器迁移进展

与 Rust 编译器并行，正在构建一个**完全用 Aura 语言编写的编译器**（位于 `aura/compiler/aura/lang/compiler/`）。Rust 编译器（`rust/compiler/`）完全保留不修改，作为 fallback 与参考实现。

| 阶段 | 组件 | 状态 |
|------|------|------|
| P0 | 基础设施（TestRunner、Main 骨架） | ✅ |
| P1 | Lexer + Parser + AST | ✅ |
| P2 | Sema（Type、SymbolTable、TypeInfo、TypeChecker）+ HIR（Lower、Desugar、Mono、Inline、Fold） | ✅ |
| P3 | MIR（IR 类型、HIR→MIR 降级、DCE/CSE/常量传播优化） | ✅ |
| P4 | 字节码 Codegen（MIR→.auc）+ VM 解释器 | ✅ |
| P5 | VM 增强（闭包、尾调用、栈帧） | ✅ |
| P6 | AOT 后端（LLVM IR 生成） | ✅ |
| P6.5 | AOT 加固：class / std 签名表 / 集合 / 多模块链接 | 🚧 进行中 |
| P7 | JIT（Cranelift） | ✅ |
| P8 | Core 与标准库的 Aura 化 | ✅ |
| P9 | 端到端编译管线（VM / JIT / AOT） | ✅ |
| P10 | MIR → SSA（SsaBuilder、SsaMir、TypeRegistry、Linearizer） | ✅ |
| P11 | HAT IR（文本格式、`HatSerializer` / `HatParser`） | ✅ |
| P12 | HAT / Photon 原生后端（LIR → DAG → RegAlloc → X86 → COFF → 链接） | ✅（P1–P3 差分 15/15） |
| P13 | HAT 自举编译器自身 | 🚧 链接阶段 —— 见 [原生后端](#原生后端--hat--photon) |

### 自举流程

Aura 编译器从一个预编译的种子二进制启动：

```
aura/seed/aura.exe          <- Rust 引导（种子，无需重编）
        │
        ▼  aura build aura/compiler/.../Main.aura
        │
        ▼
build/auc/compiler/aura-compiler.auc   <- Aura 编写的编译器字节码
        │
        ▼  aura run aura-compiler.auc <file.aura>
        │
        ▼
build/output/<file>.exe               <- AOT 编译出的原生可执行文件
```

另有一条完全独立的自举路径，直接驱动 HAT 链路、不经过 VM：

```
rust/target/release/aura.exe
        │  aura build --aot aura/compiler/.../backend/photon/PhotonHatCompile.aura
        ▼
build/hat-native/PhotonHatCompile.exe        <- AOT 编译出的 HAT 链路驱动
        │  <driver>  Main.aura
        ▼
Main.aura ──► HirProgram ──► SSA MIR ──► .hat ──► COFF ──► lld-link ──► .exe
```

**引导解析顺序**（`build-aura-compiler.ps1`）：

1. `rust/target/release/aura.exe` —— cargo 构建产物（首选）
2. `rust/target/debug/aura.exe`
3. `aura/seed/aura.exe` —— 冻结的 git-LFS 种子，仅在指定 `-FrozenSeed` 或前两者都不存在时使用

**重建种子**（`rust/compiler` 更新后）：

```powershell
cd rust
cargo build --release -p cli --features llvm
copy target\release\aura.exe ..\aura\seed\aura.exe

# 或在仓库根目录：
scripts\build-aura-compiler.ps1 -RebuildSeed
```

### 测试结果

```
tests/phase0_tests.aura           → RESULT: PASS
tests/phase1_lexer_tests.aura     → RESULT: PASS
tests/phase2_sema_hir_tests.aura  → RESULT: PASS  (20 个测试组，0 失败)
tests/phase3_mir_tests.aura       → RESULT: PASS  (11 个测试组，0 失败)
tests/phase5_vm_tests.aura        → RESULT: PASS
tests/phase6_aot_tests.aura       → RESULT: PASS
tests/phase6_5_aot_tests.aura     → RESULT: PASS
tests/phase7_jit_tests.aura       → RESULT: PASS
tests/phase8_stdlib_tests.aura    → RESULT: PASS
tests/phase9_compiler_tests.aura  → RESULT: PASS

scripts\photon-hat-native-suite.ps1  P1+P2+P3  → PASS=15 FAIL=0
scripts\photon-hat-suite.ps1         P1+P2+P3  → PASS=15 FAIL=0
```

### 主要模块

```
test/        TestRunner                        — 测试框架（自包含）
lexer/      Span, Token, Lexer                 — 词法分析器（支持字符串插值）
parser/     Parser                             — 递归下降 + Pratt 优先级解析器
ast/        Ast                                — 扁平 arena AST（kinds/texts/tys/spans/kids）
sema/       Type, SymbolTable, TypeInfo, TypeChecker — 类型系统与语义分析
hir/        Hir, Desugar, Mono, Inline, Fold, HirSerializer — HIR 降级与优化
hir/hat/    HatSerializer, HatParser           — HAT 文本 IR（可往返）
mir/        Mir, MirLower, MirOpt              — MIR IR、HIR→MIR 降级、优化
mir/        SsaBuilder, SsaMir, TypeRegistry, Linearizer — SSA 构造器与 SSA MIR
codegen/    Codegen                            — MIR → 字节码发射
vm/         VmRunner, Closures, TailCall, FrameManager, Frames, Opcodes — VM 解释器
aot/        Emit, StdSigs, Runtime, ModuleLink — LLVM IR 发射、std 签名表、多模块链接器
jit/        JitCore, JitState, JitOpt, JitLower, DispatchTable — JIT 后端
gc/         Gc, MarkSweep, Incremental, Concurrent      — GC 实现
memory/     Memory, MemoryPool, Arc                    — 内存管理与 ARC
backend/photon/  PhotonPipeline, Lowering, InstructionSelection, RegisterAllocator,
                 PeepholeOptimizer, MachineDag, Lir, X86Emitter, PhotonObjectWriter,
                 PhotonSystemLinker, PhotonRuntime, SyscallEmitter, JitBackend,
                 PhotonHatCompile（HAT 链路驱动）
errors/     CompileError                       — 诊断模型
serialize/  AucSerializer, AucLoader, PlatformFileIO, WinFileIO — .auc 格式
linker/     Linker                             — 模块链接器
signature/  Signature                          — 签名表
Main.aura   — 编译器入口骨架
```

---

## 仓库结构

```text
AuraLang/
├── Cargo.toml              Workspace 根
├── LICENSE                 Apache-2.0
├── README.md               English
├── README.zh-CN.md         ← 中文文档
├── aura.toml               Loom 包清单
│
├── aura/                   Aura 语言源码（自举）
│   ├── core/               核心语言类型（Any, Int, String, List, Map, ...）
│   ├── compiler/           用 Aura 编写的编译器
│   │   └── aura/lang/compiler/
│   │       ├── lexer/ parser/ ast/ sema/ errors/ sourcemap/   前端
│   │       ├── hir/                        HIR 与优化 pass
│   │       │   └── hat/                    HAT 文本 IR（序列化 + 反序列化）
│   │       ├── mir/                        MIR 与 SSA（SsaBuilder, SsaMir, Linearizer）
│   │       ├── codegen/ vm/                字节码发射 + VM 解释器
│   │       ├── aot/ jit/                   LLVM IR AOT + Cranelift JIT
│   │       ├── backend/photon/             HAT 原生后端（LIR → COFF → exe）
│   │       ├── gc/ memory/ runtime/        GC、ARC、协程运行时
│   │       ├── serialize/ linker/ signature/ auz/ package/
│   │       └── Main.aura                   编译器入口
│   ├── runtime/            运行时支持源码
│   ├── toolchain/          LSP、调试器、文档生成、cli
│   └── seed/
│       └── aura.exe        Rust 引导种子（预编译，无需重编）
│
├── rust/                   Rust 工具链（从此处构建）
│   ├── compiler/           编译器 crate（lexer, sema, codegen, vm, std, lsp, ...）
│   │   ├── src/
│   │   │   ├── lexer.rs            词法分析器（手写，支持插值、原始字符串）
│   │   │   ├── parser.rs           递归下降 + Pratt 优先级解析器
│   │   │   ├── ast.rs              AST 节点定义
│   │   │   ├── sema/               语义分析（ty / symbol / checker）
│   │   │   ├── codegen/
│   │   │   │   ├── hir.rs              HIR 脱糖
│   │   │   │   ├── mir.rs              MIR 降级
│   │   │   │   ├── emit.rs             字节码发射
│   │   │   │   ├── opt.rs              优化 pass
│   │   │   │   ├── arc.rs              ARC 分析与插入
│   │   │   │   ├── serialize.rs        .auc 二进制格式
│   │   │   │   └── aot/                AOT（LLVM）后端
│   │   │   │       ├── emit.rs         LLVM IR 生成
│   │   │   │       ├── linker.rs       llc/clang 链接
│   │   │   │       ├── target.rs       跨平台三元组
│   │   │   │       ├── dwarf.rs        DWARF 调试信息
│   │   │   │       └── c_backend.rs    C 代码回退
│   │   │   ├── vm/
│   │   │   │   ├── interp.rs           栈式解释器
│   │   │   │   ├── jit.rs              Cranelift JIT
│   │   │   │   ├── ffi.rs              C FFI（extern "c"）
│   │   │   │   ├── heap.rs             GC 堆 + ARC
│   │   │   │   ├── value.rs            运行时值
│   │   │   │   ├── actor.rs            Actor 运行时
│   │   │   │   ├── channel.rs          消息通道
│   │   │   │   ├── coroutine.rs        协程与 suspend
│   │   │   │   ├── thread_pool.rs      线程池
│   │   │   │   ├── debugger.rs         源码级调试器
│   │   │   │   └── ...                 IPC、动态 FFI、native 等
│   │   │   ├── std/                    标准库（19 个模块）
│   │   │   │   ├── decl.rs             标准库函数单一真相源
│   │   │   │   ├── std_math.rs         数学函数
│   │   │   │   ├── std_io.rs           输入输出
│   │   │   │   ├── std_collections.rs  集合操作
│   │   │   │   ├── std_concurrent.rs   并发 API
│   │   │   │   ├── std_json.rs         JSON 解析与序列化
│   │   │   │   ├── std_string.rs       字符串操作
│   │   │   │   ├── std_fs.rs           文件系统
│   │   │   │   ├── std_env.rs          环境变量
│   │   │   │   ├── std_process.rs      进程管理
│   │   │   │   ├── std_time.rs         时间日期
│   │   │   │   └── ...                 （+8 个模块）
│   │   │   ├── auz/                    .auz 制品格式
│   │   │   ├── lsp.rs                  LSP 服务器（stdio JSON-RPC）
│   │   │   ├── package.rs              包管理器
│   │   │   ├── docgen.rs               API 文档生成器
│   │   │   └── linker.rs               模块链接
│   │   ├── tests/                    集成测试（40+ 测试文件）
│   │   └── examples/                 AOT 基准测试
│   ├── cli/                    命令行工具（3 个二进制）
│   │   └── src/
│   │       ├── main.rs             `aura` — 20+ 子命令
│   │       ├── lsp_main.rs         `aura-lsp` — 独立 LSP 进程
│   │       └── debugger_main.rs    `aura-debug` — 源码级调试器
│   ├── loom/                   构建系统（Gradle/Bazel 风格）
│   └── target/                 Rust 构建产物（生成）
│
├── book/                   用户文档（教程、API、迁移指南）
├── docs/                   技术设计文档（系统/编译/语言/集成/规划）
├── examples/               Aura 源码示例
├── tests/                  测试文件
│   ├── phase{0..9}_tests.aura      纯 Aura 编译器迁移测试
│   ├── photon/
│   │   ├── P1/ P2/ P3/ P4/         端到端差用例
│   │   └── S1/ S2/ S3/ S4/         后端单元/集成测试
│   ├── aot/ self_bootstrap/ pure_aura/ pure_aura_cffi/ compiler/ ...
│   └── snapshots/                  快照测试
├── scripts/                PowerShell 构建、自举与套件脚本
│   ├── build-aura-compiler.ps1        种子 → Aura 编译器构建
│   ├── bootstrap-photon.ps1           Photon 自举
│   ├── photon-hat-bootstrap.ps1       Main.aura 的 HAT 自举
│   ├── photon-hat-native-suite.ps1    HAT 链路差分套件
│   ├── photon-hat-suite.ps1           VM→HAT 链路差分套件
│   └── run-hat-on-main.ps1            HAT 驱动度量脚本
├── tools/                  编辑器/IDE 工具
│   ├── dsh-plugins/        DSH 语法高亮（aura, hat, phir）
│   ├── ide-extension/      IDE 扩展源码
│   └── skills/
└── build/                  生成产物（含 build/hat-native/、build/hat-bootstrap/）
```

> **布局说明**：Rust 编译器、CLI 与 `loom` 构建系统位于 `rust/` 下，**不在仓库根目录**。
> `aura/` 存放 Aura 编写的语言源码；编译器从 `aura/seed/aura.exe` 的冻结种子自举。

---

## 快速开始

### 前置要求

- Rust 1.75+（edition 2024）
- LLVM ≥ 23（实测 `clang+llvm-23.1.0-x86_64-pc-windows-msvc`），其中 `lld-link.exe` 供
  HAT / Photon 原生后端使用，`llc` 供 LLVM IR 的 AOT 路径使用
- Windows / PowerShell 5.1+（构建与自举脚本）

### 构建

```powershell
# 引导 Aura 编译器（自举，不需要 Rust 工具链）
scripts\build-aura-compiler.ps1

# 带 AOT 构建（原生可执行文件，需要 LLVM）
scripts\build-aura-compiler.ps1 -Aot

# 从 Rust 源码重建种子（rust/compiler 更新后）
scripts\build-aura-compiler.ps1 -RebuildSeed

# 重建原生 HAT 驱动（由 Rust 的 aura 做 AOT 编译，约 12 秒）
$env:Path = "D:\DevTools\LLVM\clang+llvm-23.1.0-x86_64-pc-windows-msvc\bin;$env:Path"
.\rust\target\release\aura.exe build --aot `
    aura\compiler\aura\lang\compiler\backend\photon\PhotonHatCompile.aura `
    --output build\hat-native\PhotonHatCompile.exe

# HAT 差分套件（加 -Rebuild 会先重建驱动）
.\scripts\photon-hat-native-suite.ps1 -Phase P1,P2,P3 -OutRoot build\hat-native-suite
.\scripts\photon-hat-suite.ps1        -Phase P1,P2,P3 -OutRoot build\hat-suite
```

> **说明**：`aura/seed/aura.exe` 是 git-LFS 跟踪的冻结种子，可以完全免去 Rust 工具链
> —— 用 `build-aura-compiler.ps1 -FrozenSeed` 直接启用。否则脚本优先使用
> `rust/target/{release,debug}/aura.exe` 的 cargo 构建产物。只有 `-RebuildSeed` 需要 `cargo`。

### 运行

```bash
# 编译并执行
aura run examples/compiler/showcase.aura

# AOT 编译为原生可执行文件（LLVM IR 路径）
aura build --aot examples/games/game_2d_demo.aura --target x86_64-pc-windows-msvc

# 交互式 REPL
aura repl

# 执行代码片段
aura eval --expr "println('Hello, Aura!')"

# HAT / Photon 原生路径 —— .hat IR + 直接 COFF，不经 LLVM IR
$env:AURA_HAT_AURA  = "examples\compiler\showcase.aura"
$env:AURA_HAT_OUT   = "build\hat-demo"
$env:AURA_HAT_MODULE = "showcase"
.\build\hat-native\PhotonHatCompile.exe
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

## 编辑器集成

VS Code 与 Sublime Text 4 的包位于 `tools/ide-extension/`：

| 包 | 内容 |
|----|------|
| `tools/ide-extension/aura-vscode-extension` | Aura —— LSP 集成、语法高亮、代码片段、格式化 |
| `tools/ide-extension/aura-st4` | Aura —— Sublime Text 4 语法 + 构建 + 键位 + 片段 |
| `tools/ide-extension/hat-vscode-extension`、`hat-st4` | HAT IR 语法高亮 |
| `tools/ide-extension/phir-vscode-extension`、`phir-st4` | PHIR 语法高亮 |

DSH 语法高亮插件的构建与打包位于 `tools/dsh-plugins/`
（`aura-dsh-highlight`、`hat-dsh-highlight`、`phir-dsh-highlight`）。

安装：从 VS Code Marketplace 搜索 `aura-language`，或从
`tools/ide-extension/aura-vscode-extension/` 源码构建。

---

## 开发

Rust workspace 位于 `rust/`：

```bash
# 格式化
cd rust && cargo fmt --all

# Lint
cd rust && cargo clippy --all-features -- -D warnings

# 测试
cd rust && cargo test --workspace
cd rust && cargo test --release --test perf_lexer -- --nocapture

# 更新快照
INSTA_UPDATE=always cargo test
```

CI 配置在 `rust/.github/workflows/ci.yml`：包含 `cargo fmt --check`、`cargo clippy -D warnings`、
`cargo test`（debug + release）与覆盖率采集。

### HAT / Photon 后端调试

`PhotonHatCompile.exe` 识别的环境变量：

| 变量 | 作用 |
|------|------|
| `AURA_PHOTON_TRACE=1` | 逐函数输出 `[ssab]` / `[ssae]` 标记及 value/expr/type 计数 —— 用来把崩溃定位到具体某个函数 |
| `AURA_SSA_PERFN=1` | 按定义顺序逐个输出 `[ssa-fn] n=<序号> <名称>` |
| `AURA_PHOTON_DEBUG_HIR=1` | 在 HAT 序列化前转储 SSA MIR |
| `AURA_PHOTON_STOP=B\|C` | 在 Phase B（LIR）或 Phase C（DAG）之后停下 —— 用于界定问题属于哪一段 |
| `AURA_HAT_AURA` / `AURA_HAT_OUT` / `AURA_HAT_MODULE` | 驱动输入（源码路径、输出目录、模块名） |

驱动无论详细程度如何都会输出机器可读标记：

```
===COFF-MAIN===<hex>     主 COFF 目标文件的十六进制
===COFF-RUNTIME===<hex>  运行库 COFF 目标文件的十六进制
===LINK===<command>      实际的 lld-link 命令
===RESULT===success|fail
```

> **已知 AOT 陷阱（写 Aura 编译器代码时）**：`String.charCodeAt(i)` 会被降级为
> **无边界检查的内联访存**（见 `aot/Emit.aura`）。形如
> `while (i >= 0 && s.charCodeAt(i) != 10)` 的循环在 `i` 走到 -1 时会越界读取 ——
> `&&` 短路救不了你，因为边界检查写在 `charCodeAt` 的 Aura 源码里，被 AOT 内联时丢弃了。
> 要把循环结构写成「只可能传入合法下标」。

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
HIR（脱糖 / mono / inline / fold）
  │
  ├──► MIR ──► 字节码 (.auc) ──► VM（解释器）───► JIT（Cranelift）───► 原生码
  │
  ├──► LLVM IR (.ll) ──► llc ──► .o ──► 链接器 ──► 原生可执行文件
  │
  └──► SSA MIR ──► .hat ──► LIR ──► Machine DAG ──► RegAlloc ──► X86 ──► COFF ──► lld-link
                  （文本 IR，可往返）
                  HAT / Photon 原生后端 —— 不经 LLVM IR，直接生成 COFF
```

同一条 HIR 上有三条互不依赖的原生路径：经典的 LLVM IR AOT 路径、JIT 路径，以及 HAT / Photon
路径 —— 后者自带 IR、寄存器分配器、x86-64 编码器与 COFF 目标文件写入器。

---

## 许可证

[Apache-2.0](LICENSE)
