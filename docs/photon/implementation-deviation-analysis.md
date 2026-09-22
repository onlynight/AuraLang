# Photon 后端实现偏差分析报告

> **分析日期**：2026-09-22
> **分析范围**：`aura/compiler/aura/lang/compiler/backend/photon/` + `rust/cli/src/main.rs` + 构建脚本
> **参考文档**：
> - `docs/Lir2MacCode/参考go rust设计新的编译后端.md` (v2.1)
> - `docs/Lir2MacCode/photon-self-contained-design-v3.md` (v3)

---

## 1. 设计目标回顾

| # | 设计目标 | 来源 |
|---|---------|------|
| G1 | **Rust 仅作种子编译器**：Rust 编译 Aura → VM 字节码 → Aura 自举编译器 | v3 §7 |
| G2 | **Aura 自举编译器为核心**：CLI、编译管线全部由 Aura 自举编译器驱动 | v2.1 §1.3 |
| G3 | **LLVM IR + Photon 并存**：LLVM 做过渡 AOT，Photon 做自包含后端 | v2.1 §1.4 |
| G4 | **Photon 零外部依赖**：不依赖 kernel32.dll，用 Nt* syscall | v3 §1, D5 |
| G5 | **端到端产出 exe**：`aura build -b photon` → .exe | v3 §6.1 |
| G6 | **自举验证**：Rust→LLVM→Aura(Photon)→自举→字节一致 | v3 §7 |

---

## 2. 当前实现架构（实际状态）

```
用户调用
  │
  ▼
┌─────────────────────────────────────────────────────┐
│  rust/cli/main.rs (Rust CLI — 仍然承担全部前端)        │
│  ┌─────────────────────────────────────────────┐    │
│  │ cmd_build (-b photon)                       │    │
│  │   Rust: Lex → Parse → Sema → HIR           │    │
│  │   写 HIR JSON (函数体为空 "stmts":[ ])      │    │
│  │   调用 aura run PhotonDriver.aura           │    │
│  │     → Driver 硬编码路径, 不读 JSON           │    │
│  │     → 生成测试 HIR (main→return 42)         │    │
│  │     → 调用 pipeline.compileHir(测试HIR)      │    │
│  │       → SSA→LIR→DAG→RegAlloc→Encode→COFF   │    │
│  │       → 写 .obj.hex → 由 PowerShell 转二进制  │    │
│  │       → 调用 lld-link → .exe               │    │
│  └─────────────────────────────────────────────┘    │
│  cmd_build (--aot)                                  │
│   Rust: LLVM IR → llc/clang → .exe                │
└─────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────┐
│  构建脚本层 (PowerShell — 实际驱动编译流程)              │
│  build-photon-hello.ps1: 硬编码 main+println          │
│  build-photon-full.ps1: 前端→后端→链接三步            │
│  bootstrap-photon.ps1: 全部是空壳 (stub)              │
└─────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────┐
│  aura/compiler/.../photon/ (Aura 后端 — 运行在 VM 下) │
│  PhotonPipeline.compileHir()                         │
│   Phase A: HIR → SSA MIR (SsaBuilder)               │
│   Phase B: SSA → LIR (Lowering)                     │
│   Phase C: LIR → Machine DAG (InstructionSelection) │
│   Phase D: RegAlloc + Peephole                      │
│   Phase E: X86Emitter → COFF → SystemLinker         │
│  PhotonRuntime: kernel32 GetStdHandle/WriteFile      │
└─────────────────────────────────────────────────────┘
```

---

## 3. 六大核心偏差

### 偏差 1: Rust CLI 仍然承担全部前端 — G1/G2 违背

| 设计目标 | 实际状态 | 影响 |
|---------|---------|------|
| Rust 仅作种子，生成 Aura 自举编译器 | `cmd_build_photon` 在 Rust 中做 Lex/Parse/Sema/HIR | Rust 不再是种子，而是主编译器前端 |
| Aura 自举编译器为核心 | Aura 侧 `Main.aura` 存在但从未被用作 CLI 入口 | 无法验证自举闭环 |

**根因**：`cmd_build_photon` (main.rs:294-529) 在 Rust 中完成全部前端工作后，试图将 HIR 传递给 Aura 后端。但 HIR JSON 序列化只写了函数元信息，**函数体是空的**：

```rust
// main.rs:455-456 — 函数体始终为空！
hir_json.push_str(",\"body\":{\"stmts\":[]}");
```

这意味着即使后端管线能运行，也永远编译不到实际用户代码。

**修正路径**：
1. **短期**：修复 HIR JSON 序列化，将完整函数体（含表达式树）序列化到 JSON 中
2. **中期**：让 `Main.aura`（Aura 自举编译器）提供 CLI 入口，Rust 仅作为编译 Main.aura 的种子
3. **长期**：完全移除 Rust 前端，所有编译由 Aura 自举编译器完成

---

### 偏差 2: Rust → Aura 的 HIR 桥接完全断裂 — G5 违背

三个独立问题导致数据流中断：

**2a. JSON 格式完全不兼容**

Rust CLI 产生的 JSON：
```json
{"version":1,"module":"test","functions":[{"name":"main","params":[],"ret":"Unit","isNative":false,"body":{"stmts":[]}}]}
```

Aura 侧 `HirSerializer.parse()` 期望的 JSON：
```json
{"kind":"Program","text":"","ty":"","kids":[{"kind":"Function","text":"main","ty":"Unit","kids":[...]}]}
```

两种格式**毫无交集**。`parseObject()` 寻找 `kind`/`text`/`ty` 字段，而 Rust 产出的 JSON 根本没有这些字段。

**2b. Driver 硬编码路径，不接收 CLI 参数**

```aura
// PhotonDriver.aura:113-115 — 硬编码，不从环境/参数读取
this.hirFile = "build/photon.hir"
this.outDir = "build/lldtest"
this.moduleName = "program"
```

CLI 调用 `Command::new(aura_path).args(["run", driver_path])` 不传任何参数，Driver 也完全忽略环境。

**2c. HIR 路径不匹配**

CLI 写 HIR 到 `<input>.photon.hir`，Driver 读 `build/photon.hir` — 路径完全不同。

**修正路径**：
1. **统一 HIR 序列化格式**：Rust CLI 和 Aura 侧必须使用**同一 JSON schema**。建议采用 Aura 侧的 `kind/text/ty/kids` 递归树格式（已在 `HirSerializer.aura` 中定义），修改 Rust 侧的 `cmd_build_photon` 来生成这种格式
2. **参数传递**：通过环境变量（`AURA_PHOTON_HIR`、`AURA_PHOTON_OUT`、`AURA_PHOTON_MODULE`）或命令行参数将路径传递给 `PhotonDriver`
3. **短期修复**：在 Rust CLI 中正确实现 HIR JSON 序列化（递归序列化函数体的每个 HIR 节点），并修复 Driver 的参数解析

---

### 偏差 3: 端到端 exe 产出依赖硬编码路径 — G5 违背

**唯一能产出可运行 exe 的路径是 `build-photon-hello.ps1`**，它完全绕过了真实编译管线：

```
PhotonHelloBuild.aura (硬编码)
  → X86Encoder 直接编码 main() → call println
  → X86Encoder 直接编码 println() → kernel32::WriteFile
  → PhotonObjectWriter 包装为 COFF hex
  → PowerShell 转二进制 .obj
  → lld-link → hello.exe
```

**这绕过了**：SSA Builder、Lowering、InstructionSelection、RegisterAllocator、PeepholeOptimizer、完整的 `compileHir()` 管线。

**验证**：`PhotonPipeline.compileHir()` 的 `Phase E` 会调用 `X86Emitter.emitFunction(dag, ...)` 来编码真实管线产出的 DAG，但 `PhotonDriver.run()` 传给它的 HIR 是**测试 HIR**（`createTestHir()` 产出的 `main → return 42`），不是真实用户代码。

**修正路径**：
1. 修复偏差 1 和 2 后，`compileHir()` 将接收真实用户代码的 HIR
2. 验证 `compileHir()` 对中等复杂度程序（控制流、循环、函数调用）的端到端产出
3. `PhotonHelloBuild` 降级为回归测试用例，不作为验证手段

---

### 偏差 4: 自举脚本全部是空壳 — G1/G6 违背

`bootstrap-photon.ps1` 的四个步骤全部是 stub：

| 步骤 | 函数 | 实际内容 |
|------|------|---------|
| Step 1: LLVM 编译 | `Build-LvmCompiler` | 仅检查 `main.rs` 是否存在，打印 "Run: cargo build"，返回 true |
| Step 2: Photon 编译运行时 | `Build-PhotonRuntime` | 仅检查文件存在，打印 "requires Phase 1-5 complete"，返回 true |
| Step 3: Photon 编译编译器 | `Build-PhotonCompiler` | 打印 "requires aura-llvm.exe from Step 1"，返回 true |
| Step 4: 验证 | `Verify-Bootstrap` | 检查 `aura-llvm.exe` 和 `aura-photon.exe`（均不存在），生成报告 |

旧脚本 `bootstrap.ps1` 尝试了真正的自举（seed → .auc → test run），但它使用的是**旧自举模型**（seed → VM 字节码），而非设计文档要求的 **Rust → LLVM AOT → Aura(Photon) → 自举验证** 模型。

**修正路径**：
1. **实现 Step 1**：用 Rust LLVM AOT 后端编译 `Main.aura` 为 `aura-llvm.exe`
2. **实现 Step 2**：用 `aura-llvm.exe` 编译 Aura 运行时（`aura/runtime/*.aura`）为 `.obj`
3. **实现 Step 3**：用 `aura-llvm.exe` 的 Photon 后端编译 `Main.aura` 为 `aura-photon.exe`
4. **实现 Step 4**：用 `aura-photon.exe` 重新编译自身为 `aura-photon2.exe`，比较字节一致性
5. **实现确定性**：消除 COFF 中的时间戳、随机数据，确保编译结果可重现

---

### 偏差 5: Windows 运行时依赖 kernel32.dll — G4 违背

| 设计要求 | 实际状态 | 文件 |
|---------|---------|------|
| Nt* syscall，无 kernel32 依赖 | kernel32.dll FFI (GetStdHandle, WriteFile, VirtualAlloc, VirtualFree, ExitProcess) | `PhotonRuntime.aura` |
| Photon 直接生成 syscall 指令 | 通过 `emitCallImport` 调用 DLL 导入表 | `X86Encoder.aura` |
| linker.libs = "" (无默认库) | `linker.libs = "kernel32"` | `PhotonPipeline.aura:240` |

`PhotonPipeline.aura:240`:
```aura
linker.libs = "kernel32"  // runtime 对象引用 kernel32!GetStdHandle/WriteFile
```

v3 设计文档 §4 详细设计了 `@native(N)` → syscall 指令的生成方案（Linux x86_64 syscall 号和 Windows Nt* 服务号），但代码中**完全没有 `SyscallEmitter.aura`**。

**修正路径**：
1. **Phase 1**：实现 `SyscallEmitter.aura`，支持 Linux x86_64 `syscall` 指令（`rax=号, rdi/rsi/rdx/r10/r8/r9=参数`）
2. **Phase 2**：实现 Windows Nt* syscall（`rax=服务号, rcx/rdx/r8/r9=参数, syscall`）
3. **Phase 3**：将 `PhotonRuntime.aura` 中的 kernel32 调用替换为 Nt* syscall
4. **Phase 4**：移除 `linker.libs = "kernel32"`，改为 `linker.libs = ""`
5. **Phase 5**：验证 `ldd`/`dumpbin` 显示无 DLL 依赖

---

### 偏差 6: CLI 工具全部在 Rust 中 — G2 违背

| 工具 | 设计目标 | 实际状态 |
|------|---------|---------|
| `aura build` | Aura 自举编译器 | Rust CLI (`main.rs:154`) |
| `aura run` | Aura 自举编译器 | Rust CLI (`main.rs:873`) |
| `aura check` | Aura 自举编译器 | Rust CLI |
| `aura disasm` | Aura 自举编译器 | Rust CLI |
| `aura fmt` | Aura 自举编译器 | Rust CLI |
| `aura new/package/inspect` | Aura 自举编译器 | Rust CLI |

**根因**：`Main.aura` 虽然存在且被编译为 `.auc`，但它没有 CLI 入口——没有 `cmd_build`、`cmd_run` 等命令分发逻辑。Rust CLI 既是前端编译器，又是 CLI 分发器。

**修正路径**：
1. 在 `Main.aura` 中实现完整的 CLI 命令分发器（`cmd_build`、`cmd_run`、`cmd_check` 等）
2. 实现 `Main.aura` 的入口点，使其可作为独立 exe 运行
3. 用 Rust 种子编译 `Main.aura` → `aura-native.exe`（通过 LLVM AOT）
4. 将 `aura-native.exe` 部署为主要的 `aura` 命令
5. Rust CLI 降级为辅助工具（仅用于首次引导）

---

## 4. HIR 序列化格式问题

### 4.1 HIR JSON 不是原始设计

**原始设计**：Rust HIR (`HirProgram`) 从未设计过序列化——它始终在内存中直接传递给下一阶段（MIR 或 AOT codegen），无需落盘。

**现有序列化**：Rust 侧唯一的序列化是 `.auc` 字节码格式（`serialize.rs`，magic `"AURA"`，version 7），但它序列化的是 `BytecodeModule`（VM 字节码），**不是 HIR**。

**HIR JSON**：`cmd_build_photon` (main.rs:410-460) 中内联手写的 JSON 是**专门为 Photon 后端桥接临时拼凑的**，且存在三个致命问题：

```
┌──────────────────────────────────────────────────────────────────────┐
│  cmd_build_photon 内联手写 JSON (main.rs:410-460)                       │
│  ─────────────────────────────────────────────────────               │
│  {"version":1,"module":"name","functions":[                           │
│    {"name":"main","params":[],"ret":"Unit",                           │
│     "isNative":false,                                                 │
│     "body":{"stmts":[]}}   ← ← ← 函数体永远为空！                       │
│  ]}                                                                   │
│                                                                      │
│  ❌ 1. 函数体始终为空 — 只序列化签名，不序列化表达式树                    │
│  ❌ 2. JSON schema 与 Aura 侧 HirSerializer 完全不兼容                   │
│     Rust 侧:  version/module/functions/name/params/ret/stmts          │
│     Aura 侧:  kind/text/ty/kids  (递归树)                             │
│  ❌ 3. 没有真正的 HIR 序列化 — 没有任何 serialize/to_bytes 方法          │
└──────────────────────────────────────────────────────────────────────┘
```

**证据**：`hir.rs` 中 `HirProgram`、`HirFunction`、`HirExpr`、`HirStmt` 等所有 HIR 类型**没有任何** `serialize`、`to_bytes`、`encode` 方法（grep 验证 0 匹配）。JSON 序列化是 100% 内联在 main.rs 中的临时代码。

### 4.2 正确方案

HIR 序列化应采用 **Photon IR** 标准格式（详见 `photon-ir-format-spec.md`），而非 JSON：

| 方案 | 优点 | 缺点 |
|------|------|------|
| ~~JSON~~ | 人类可读 | 冗长、难解析、低效、格式不兼容 |
| ~~二进制~~ | 高效 | 不透明、调试困难 |
| **Photon IR** | 人类可读 + 高效 + 标准 | 需要实现解析器 |

---

## 5. 偏差优先级矩阵

| 偏差 | 影响程度 | 修复难度 | 优先级 | 依赖 |
|------|---------|---------|--------|------|
| #1 Rust 前端序列化 | 🔴 致命：管线无真实代码输入 | 中 | **P0** | — |
| #2 HIR JSON 格式 | 🔴 致命：Rust→Aura 桥接断裂 | 中 | **P0** | — |
| #3 Driver 参数硬编码 | 🔴 致命：Driver 不接收输入 | 低 | **P0** | — |
| #4 端到端验证 | 🔴 致命：唯一工作路径是硬编码 | 高 | **P0** | #1 #2 #3 |
| #5 自举脚本空壳 | 🟡 重要：无法验证自举 | 高 | **P1** | #1 #2 #3 |
| #6 kernel32 依赖 | 🟡 重要：违背零外部依赖 | 中 | **P1** | — |
| #7 CLI 在 Rust | 🟡 重要：违背自举编译器目标 | 高 | **P2** | #1 #2 #3 #4 #5 |

---

## 6. 分阶段修正计划

### P0 阶段：打通真实编译管线（2-3 周）

| 步骤 | 任务 | 文件 | 产出 |
|------|------|------|------|
| 1 | 统一 HIR 序列化格式为 Photon IR | `main.rs` + 新 `phir/` | Rust 产出 `.phir` 文件 |
| 2 | 实现完整 HIR → Photon IR 转换 | `cmd_build_photon` | 函数体不再为空 |
| 3 | 修复 PhotonDriver 参数解析（环境变量） | `PhotonDriver.aura:110-116` | 接收 CLI 传入的路径 |
| 4 | 修复 Rust→Driver 数据流 | `main.rs:482-485` | 传递 --hir/--out/--module |
| 5 | 端到端测试：hello.aura → hello.exe | 测试脚本 | 可运行的 hello.exe |
| 6 | 端到端测试：add.aura → add.exe | 测试脚本 | 函数调用正确 |
| 7 | 端到端测试：控制流/循环 | 测试脚本 | 复杂程序正确 |

### P1 阶段：自举验证（3-4 周）

| 步骤 | 任务 | 文件 | 产出 |
|------|------|------|------|
| 1 | 实现 bootstrap-photon.ps1 Step 1 | `bootstrap-photon.ps1` | Rust LLVM AOT 编译 Main.aura |
| 2 | 实现 Step 2: Photon 编译运行时 | `bootstrap-photon.ps1` | aura/runtime → .obj |
| 3 | 实现 Step 3: Photon 编译编译器 | `bootstrap-photon.ps1` | Main.aura → aura-photon.exe |
| 4 | 实现 Step 4: 自举验证 | `bootstrap-photon.ps1` | 字节一致性比较 |
| 5 | COFF 确定性 | `PhotonObjectWriter.aura` | 消除时间戳 |

### P2 阶段：自包含运行时（3-4 周）

| 步骤 | 任务 | 文件 | 产出 |
|------|------|------|------|
| 1 | SyscallEmitter.aura | 新文件 | Linux syscall 指令生成 |
| 2 | Windows Nt* syscall | 新文件 | Nt* 服务号 → syscall |
| 3 | Arena 分配器 (Aura) | `aura/runtime/Memory.aura` | mmap 基于堆分配 |
| 4 | ARC 引用计数 (Aura) | `aura/runtime/GC.aura` | 原子 lock inc/dec |
| 5 | 替换 PhotonRuntime kernel32 | `PhotonRuntime.aura` | 无 DLL 依赖 |

### P3 阶段：CLI 自举化（4-6 周）

| 步骤 | 任务 | 文件 | 产出 |
|------|------|------|------|
| 1 | Main.aura CLI 分发器 | `Main.aura` | cmd_build/run/check 等 |
| 2 | Main.aura 入口点 | `Main.aura` | 独立可执行 |
| 3 | 引导脚本 | `bootstrap.ps1` | seed → LLVM → Aura CLI |
| 4 | Rust CLI 降级 | `main.rs` | 仅辅助工具 |

---

## 7. 当前真实完成度

| 维度 | 完成度 | 说明 |
|------|--------|------|
| **编译管线文件存在** | ✅ ~95% | Phase A-E 代码已写完 |
| **编译管线端到端可运行** | ❌ 0% | 真实管线从未处理过真实用户代码 |
| **端到端 exe 产出** | ⚠️ 仅硬编码 hello world | 绕过管线，手写 X86Encoder |
| **自举验证** | ❌ 0% | 脚本是空壳 |
| **零外部依赖** | ❌ 0% | kernel32.dll 依赖 |
| **CLI 自举化** | ❌ 0% | 全部在 Rust 中 |
| **HIR 序列化** | ❌ 0% | 无真正的序列化，JSON 是临时代码 |
| **Photon IR 格式** | ❌ 0% | 未实现，仅在文档中定义 |

---

## 8. 总结

**核心矛盾**：设计文档将 Photon 后端定位为"纯 Aura 自包含编译管线"，自举链为 Rust(seed) → LLVM(AOT) → Aura(Photon) → 自举验证。但实际实现中：

1. **Rust CLI 仍然是主编译器**，不是种子——前端在 Rust，后端通过子进程调用 `aura run`
2. **Rust → Aura 的 HIR 桥接完全断裂**——JSON 格式不兼容，函数体为空，Driver 硬编码路径
3. **唯一能产出 exe 的路径完全绕过了真实管线**——`PhotonHelloBuild` 手写 X86Encoder 编码，不走 SSA→LIR→DAG→RegAlloc
4. **自举脚本是空壳**——4 个步骤全部只打印信息就返回 true
5. **Windows 运行时仍依赖 kernel32.dll**——与"零外部依赖"目标相悖
6. **CLI 工具全部在 Rust 中**——Aura 自举编译器没有 CLI 入口
7. **HIR 序列化是临时代码**——不是原始设计，函数体为空，格式不兼容

**修正优先级**：先修复 P0（HIR 序列化 + JSON 格式 + Driver 参数 + 端到端验证），这是所有后续工作的基础。不修复 P0，P1/P2/P3 都是空中楼阁。

**推荐方案**：用 **Photon IR 标准格式**（`photon-ir-format-spec.md`）替代当前的 HIR JSON 桥接，这是所有偏差的根本解决方案。
