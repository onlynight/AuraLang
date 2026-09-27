# 绕过 LLVM 直接生成机器码 —— 可行性评估与技术方案

> **作者**：SenseNova 6.8 Flash Lite  
> **日期**：2026-06-23  
> **状态**：技术评估（不修改代码）

---

## 目录

1. [背景与目标](#1-背景与目标)
2. [现状分析：当前架构全景](#2-现状分析当前架构全景)
3. [可行性评估](#3-可行性评估)
4. [工作量评估](#4-工作量评估)
5. [技术方案（三条路径对比）](#5-技术方案三条路径对比)
6. [风险与缓解措施](#6-风险与缓解措施)
7. [结论与建议](#7-结论与建议)

---

## 1. 背景与目标

### 1.1 现状

Aura 编译器当前通过 **HIR → LLVM IR（文本）→ llc/clang → 机器码** 的管线进行 AOT 编译。该管线存在以下依赖：

- 外部 LLVM 23.1.0 工具链（`llc`、`clang`、`lld`）
- 文本 LLVM IR 作为中间表示（不依赖 inkwell 运行时绑定）
- 跨平台目标三元组（x86_64 / aarch64 / armv7 等）

### 1.2 动机

评估绕过 LLVM 直接生成机器码的可行性与工作量，以：

- **减少外部依赖**：LLVM 工具链体积大（>500MB），部署不便
- **加速编译**：省去 `llc` 进程调用的 IPC 开销
- **控制优化**：对生成代码有更精细的掌控
- **单一二进制分发**：编译器自身不依赖外部工具链

### 1.3 非目标

- 不要求生成代码性能达到 LLVM O3 水平
- 不要求支持 LLVM 的全部优化 pass
- 不要求跨 6 个以上架构（x86_64 / aarch64 / armv7 为目标子集）

---

## 2. 现状分析：当前架构全景

### 2.1 编译管线

```
源码 (.aura)
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  前端：Lexer → Parser → AST → 语义分析 → HIR              │
│                                                          │
│  HIR = 表达式树（Lit/Var/Binary/Unary/Call/Member/       │
│       Index/New/If/Block/Lambda/CallVirtual）             │
│       + 语句序列（Val/Var/Assign/Return/If/While/...）    │
└─────────────────────────────────────────────────────────┘
    │
    ├──▶ 路径 A：HIR → MIR → 字节码 → VM 解释器（或 Cranelift JIT）
    │
    ├──▶ 路径 B：HIR → LLVM IR 文本 → llc/clang → .o/.exe  ← 当前 AOT 路径
    │
    └──▶ 路径 C：HIR → C 代码 → gcc/clang/cl → .exe（备选降级）
```

### 2.2 各路径特点

| 维度 | 路径 A（VM/JIT） | 路径 B（LLVM AOT） | 路径 C（C 后端） |
|------|-----------------|-------------------|-----------------|
| **中间表示** | MIR（寄存器式 CFG） | LLVM IR（文本） | C 源码 |
| **外部依赖** | Cranelift（纯 Rust） | LLVM 23.1.0（~500MB） | gcc/clang/cl |
| **支持目标** | x86_64 / aarch64（JIT） | 6+ 架构 + 交叉编译 | 依赖系统 C 编译器 |
| **优化能力** | Cranelift O2（速度优化） | LLVM O0-O3 + Os/Oz | 依赖 C 编译器 -O2 |
| **启动速度** | 秒级（VM 解释）/毫秒级（JIT） | 秒级（进程调用 llc） | 秒级（进程调用 gcc） |
| **功能覆盖** | 子集（整数/控制流/基础对象） | 全功能（ARC/闭包/枚举/异常） | 简化（无 ARC/无泛型） |
| **运行时开销** | 低（JIT）/ 高（解释） | 零（原生代码） | 零（原生代码） |

### 2.3 关键基础设施（可复用资产）

绕过 LLVM 所需的**已有基础**：

1. **MIR 中间表示**（`mir.rs`，1252 行）
   - 寄存器式 CFG，基本块 + 终结指令
   - 完整支持控制流（if/while/break/continue）
   - 支持 ARC、闭包、枚举、异常处理（try/catch/finally）
   - 已有降级上下文（常量池、原生函数表、类型注册）

2. **AOT Blob 嵌入基础设施**（`aot_runtime.rs` + `aot_embed.rs`）
   - W^X 内存保护（mmap / VirtualAlloc）
   - `JitValue` ABI 共享调用约定（tag + payload）
   - `AuraFuncDesc` 函数描述符表
   - `.auc` v4 容器格式（code 段 + desc 段）

3. **Cranelift JIT 后端**（`jit.rs`，807 行）
   - 已实现 x86_64 / aarch64 机器码生成
   - 支持整数运算、控制流、函数调用、原生调度
   - 已有 AOT 模式（`cranelift::object` crate 可用）

4. **C 后端**（`c_backend.rs`，749 行）
   - 已证明从 HIR 生成中间代码的可行性
   - 支持控制流、结构体、Lambda、基本 FFI

5. **类型映射器**（`types.rs`，214 行）
   - HIR 类型 → LLVM 类型的完整映射
   - 可直接复用为 HIR 类型 → 目标机器码类型的映射

### 2.4 目标调用约定（需实现）

| 平台 | 调用约定 | 参数寄存器 | 返回值寄存器 | 栈对齐 |
|------|---------|-----------|-------------|--------|
| Windows x86_64 | Microsoft x64 | RCX, RDX, R8, R9, ... | RAX / XMM0 | 16 字节 |
| Linux x86_64 | System V AMD64 | RDI, RSI, RDX, RCX, R8, R9 | RAX / XMM0 | 16 字节 |
| macOS x86_64 | System V AMD64 | 同上 | 同上 | 16 字节 |
| Linux aarch64 | AAPCS64 | X0-X7 | X0 / D0 | 16 字节 |
| macOS aarch64 | AAPCS64 | 同上 | 同上 | 16 字节 |
| Linux armv7 | AAPCS | R0-R3 | R0 / F0 | 8 字节 |

---

## 3. 可行性评估

### 3.1 技术可行性

**总体结论：技术上可行，但工作量巨大，且存在显著的性能和维护性代价。**

#### 3.1.1 有利因素

1. **MIR 已就绪**：寄存器式 CFG IR 已完成，可直接作为后端输入
2. **Cranelift AOT 可用**：已有 Cranelift 集成，其 `cranelift::object` crate 提供 AOT 目标文件生成
3. **Blob 基础设施完整**：AOT 机器码嵌入方案已实现（W^X 保护、分发表、JitValue ABI）
4. **C 后端已验证**：从 HIR 生成中间代码的路径已走通
5. **Runtime C ABI 已定义**：ARC、字符串、协程等 runtime 函数签名已确定

#### 3.1.2 不利因素

1. **缺失 LLVM 的优化能力**
   - LLVM O2 包含 20+ 优化 pass（DCE、CSE、GVN、LICM、indvars、loop-unroll 等）
   - 自研优化需要数月研发才能达到同等水平
   - 预估性能差距：直接发射（无优化）比 LLVM O2 慢 2-5 倍

2. **多目标支持成本极高**
   - 每个架构需要独立的指令选择器 + 指令编码
   - x86_64 有 ~1000+ 指令，ARM64 有 ~500+ 指令
   - 每个架构的调用约定、ABI、栈布局都不同
   - 维护 3+ 架构的后端需要持续投入

3. **复杂语义实现困难**
   - **ARC（自动引用计数）**：需要在机器码中插入原子计数操作
   - **闭包**：需要生成捕获环境的堆分配代码 + 闭包结构体
   - **异常处理**：需要栈展开表（EH frame）+ 表驱动展开
   - **协程**：需要栈帧切换代码（coroutine state machine）

4. **调试信息复杂**
   - DWARF 格式规范超过 300 页
   - 需要生成 `.debug_info`、`.debug_line`、`.debug_str` 等段
   - 与 GDB / lldb 兼容性测试成本极高

5. **异常处理（EH）是最大难点**
   - Windows x64 使用 SEH（Structured Exception Handling）+ `.pdata` / `.xdata` 段
   - Linux/macOS 使用表驱动 EH（Itanium ABI）+ `.eh_frame` 段
   - 需要在机器码中插入 `call __C_specific_handler`（Windows）或展开表（Linux）
   - Aura 的 try/catch/finally 语义需要完整的栈展开支持

### 3.2 性能影响评估

| 场景 | 当前（LLVM O2） | 直接发射（无优化） | Cranelift AOT O2 |
|------|----------------|-------------------|------------------|
| 整数密集计算 | 基线 1.0x | 2.0-3.0x 慢 | 1.3-2.0x 慢 |
| 浮点计算 | 基线 1.0x | 1.5-2.5x 慢 | 1.2-1.8x 慢 |
| 内存密集（ARC） | 基线 1.0x | 1.5-2.0x 慢 | 1.3-1.8x 慢 |
| 循环密集型 | 基线 1.0x | 3.0-5.0x 慢 | 2.0-3.5x 慢 |

> 注：Cranelift AOT 使用其内置优化（`opt_level: "speed"`），但仍缺少 LLVM 的跨模块优化、内联缓存、循环展开等激进 pass。

### 3.3 编译速度对比

| 方案 | 编译耗时（1000 行代码） | 编译器启动速度 |
|------|----------------------|--------------|
| 当前（LLVM 文本 IR + llc） | 200-800ms | 需外部 LLVM 安装 |
| Cranelift AOT（纯 Rust） | 50-200ms | 自包含 |
| 直接机器码（纯 Rust） | 30-150ms | 自包含 |

### 3.4 可行性结论

| 评估维度 | 评分 | 说明 |
|---------|------|------|
| 技术可行性 | ★★★★☆ | MIR + Blob 基础设施完备，但 EH/ARC/闭包实现复杂 |
| 维护成本 | ★★☆☆☆ | 每个新架构需 4-8 个月，持续投入高 |
| 性能影响 | ★★☆☆☆ | 无优化情况下比 LLVM O2 慢 2-5 倍 |
| 编译速度提升 | ★★★★☆ | 省去进程调用，提速 2-4 倍 |
| 依赖减少 | ★★★★★ | 完全去除 LLVM 依赖，自包含二进制 |
| **综合推荐度** | **★★★☆☆** | 不建议全面替换 LLVM，建议分层策略 |

---

## 4. 工作量评估

### 4.1 按目标拆解

#### 4.1.1 路径 1：x86_64 单目标直接机器码（最简方案）

| 模块 | 工作量（人天） | 说明 |
|------|-------------|------|
| 指令选择器（Int/F32/F64 运算） | 5-8 | 映射 MIR BinOp → x86_64 指令 |
| 指令选择器（控制流） | 3-5 | Branch/Call/Ret → 条件跳转/调用 |
| 寄存器分配器（线性扫描） | 10-15 | 基本块间寄存器分配 |
| 栈帧布局 + Prologue/Epilogue | 5-8 | RBP-based 帧指针 + 对齐 |
| 调用约定（MSVC + SysV） | 5-8 | 参数寄存器/栈传递 |
| ARC 运行时发射 | 5-8 | 原子计数操作发射 |
| 闭包/函数指针 | 5-8 | 闭包结构体布局 |
| 对象文件/PE 生成 | 8-12 | COFF 格式（Windows） |
| 异常处理（SEH） | 10-15 | .pdata/.xdata + handler |
| DWARF 调试信息 | 8-12 | .debug_info + .debug_line |
| 测试框架 + 回归测试 | 10-15 | 与 LLVM 后端对比 |
| **小计** | **74-114** | **约 4-6 个月（单人）** |

#### 4.1.2 路径 2：x86_64 + aarch64 + armv7（三目标方案）

| 模块 | 工作量（人天） | 说明 |
|------|-------------|------|
| x86_64 完整后端 | 74-114 | 同上 |
| aarch64 后端（新增） | 50-80 | 新的指令集 + AAPCS64 调用约定 |
| armv7 后端（新增） | 40-65 | 新的指令集 + AAPCS 调用约定 |
| 跨目标回归测试 | 20-30 | 三目标 × 功能矩阵 |
| **小计** | **184-289** | **约 10-16 个月（单人）/ 5-8 个月（2 人）** |

#### 4.1.3 路径 3：Cranelift AOT（推荐方案）

| 模块 | 工作量（人天） | 说明 |
|------|-------------|------|
| Cranelift AOT 模式集成 | 10-15 | 使用 `cranelift::object` crate |
| MIR → Cranelift IR 前端 | 20-30 | 从 MIR 发射 Cranelift IR |
| 目标文件输出 + 描述符表 | 10-15 | ELF/PE 段提取 + AuraFuncDesc |
| ARC/闭包/异常处理支持 | 15-25 | 在 Cranelift IR 层面扩展 |
| 与现有 AOT Blob 基础设施集成 | 10-15 | 复用 aot_runtime.rs |
| 测试 + 性能基准 | 15-20 | 与 LLVM 后端对比 |
| **小计** | **80-120** | **约 4-6 个月（单人）** |

### 4.2 团队与时间线

| 方案 | 推荐团队规模 | 时间线 | 总人月 |
|------|------------|--------|--------|
| 路径 1（x86_64 单目标） | 1-2 人 | 4-6 个月 | 5-12 人月 |
| 路径 2（三目标） | 2-3 人 | 8-12 个月 | 16-36 人月 |
| 路径 3（Cranelift AOT） | 1-2 人 | 4-6 个月 | 5-12 人月 |

### 4.3 成本对比

| 维度 | 路径 1（x86_64 直接） | 路径 2（三目标直接） | 路径 3（Cranelift AOT） |
|------|---------------------|--------------------|------------------------|
| 初始开发成本 | 中 | 极高 | 中 |
| 维护成本 | 中 | 极高 | 低 |
| 性能损失 | 中（2-3x） | 中（2-3x） | 低（1.2-2x） |
| 功能覆盖 | 中（需自研 EH） | 中（需自研 EH） | 高（复用 JIT） |
| 新目标扩展成本 | 极高（每目标 4-8 月） | N/A | 低（Cranelift 内置） |

---

## 5. 技术方案（三条路径对比）

### 5.1 方案 A：Cranelift AOT（★ 推荐）

#### 5.1.1 架构

```
HIR
    │
    ▼
┌─────────────────────────────────────────┐
│  MIR（寄存器式 CFG，已存在）              │
│  - BasicBlock + MirInstr + Terminator   │
│  - 支持 ARC/闭包/异常处理                 │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  Cranelift IR 前端（新增）                │
│  - MIR Instr → Cranelift InstBuilder    │
│  - 寄存器分配 → Cranelift VirtualReg    │
│  - 控制流 → Block/Inst                  │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  Cranelift 后端（已存在，AOT 模式）       │
│  - cranelift::object::ObjectModule      │
│  - 目标：x86_64 / aarch64 / armv7 / ... │
│  - 优化：opt_level = "speed"            │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  目标文件输出 + Blob 嵌入（已存在）        │
│  - .o → .text 段提取                    │
│  - AuraFuncDesc 描述符表                 │
│  - JitValue ABI 包装函数                 │
└─────────────────────────────────────────┘
    │
    ▼
  .auc v4（code 段 + desc 段）
```

#### 5.1.2 关键设计决策

**决策 1：MIR → Cranelift IR（而非 HIR → Cranelift）**

- 理由：MIR 已是寄存器式 CFG，与 Cranelift IR 语义接近
- 复用已有 MIR 降级代码（`mir.rs`，1252 行）
- 避免重新实现 HIR → 新 IR 的降级逻辑

**决策 2：复用 AOT Blob 基础设施**

- `aot_runtime.rs` 已实现 W^X 加载、分发表、JitValue ABI
- `aot_embed.rs` 已实现 .auc 段表组装
- 无需修改运行时，只需改变代码生成路径

**决策 3：使用 Cranelift 的 AOT 模式（非 JIT 模式）**

- `cranelift::object::ObjectModule` 生成目标文件（.o）
- 输出格式：ELF / COFF / Mach-O
- 与当前 `link_to_object()` 流程兼容

#### 5.1.3 核心模块设计

```
compiler/src/codegen/cranelift_aot/
├── mod.rs              // 入口：compile(hir) → AotOutput
├── ir_builder.rs       // MIR → Cranelift IR（核心映射）
├── types.rs            // HIR 类型 → Cranelift types::Type
├── abi.rs              // 调用约定适配（Windows x64 / SysV / AAPCS64）
├── runtime.rs          // Runtime 函数导入（aura_arc_increment 等）
├── eh.rs               // 异常处理发射（setjmp/longjmp 简化方案）
├── object.rs           // ObjectModule 配置 + 段提取
└── emit.rs             // 段提取 → AuraFuncDesc → .auc blob
```

#### 5.1.4 指令映射示例（MIR → Cranelift）

| MIR 指令 | Cranelift IR 映射 |
|---------|-------------------|
| `LoadConst { dst, ci }` | `iconst` / `fconst` / `null` |
| `LoadLocal { dst, slot }` | `load` from stack slot |
| `StoreLocal { slot, src }` | `store` to stack slot |
| `BinOp { dst, op, a, b }` | `iadd`/`isub`/`imul`/`sdiv`/`srem` |
| `UnOp { dst, op, a }` | `ineg` / `bswap` |
| `Call { dst, func, args }` | `call` / `call_indirect` |
| `CallNative { dst, func, args }` | `call` to imported native dispatcher |
| `Alloc { dst, type_name }` | `call aura_malloc` + `call aura_alloc_object` |
| `GetField { dst, obj, field }` | `load` from struct offset |
| `SetField { obj, field, src }` | `store` to struct offset |
| `Retain { src }` | `call aura_arc_increment` |
| `Release { src }` | `call aura_arc_decrement` |
| `MakeClosure { dst, func, captures }` | `call aura_closure_create` |
| `CallClosure { dst, closure, args }` | `call_indirect` via closure struct |
| `PushHandler` / `PopHandler` | `call aura_set_handler` / `call aura_pop_handler` |

#### 5.1.5 工作量分解

| 阶段 | 周次 | 内容 |
|------|------|------|
| Phase 1 | W1-W2 | Cranelift AOT 模式集成 + 基本类型映射 |
| Phase 2 | W3-W6 | MIR → Cranelift IR 核心映射（算术/控制流） |
| Phase 3 | W7-W9 | ARC + 闭包 + 函数调用支持 |
| Phase 4 | W10-W12 | 异常处理 + 段提取 + 描述符表 |
| Phase 5 | W13-W16 | 测试 + 性能基准 + 文档 |

#### 5.1.6 预期效果

| 指标 | 当前（LLVM） | Cranelift AOT |
|------|-------------|---------------|
| 编译速度 | 200-800ms | 50-200ms（4x 快） |
| 外部依赖 | LLVM 23.1.0（~500MB） | 无（纯 Rust） |
| 性能 | 基线 1.0x | 0.6-0.8x（略慢） |
| 支持目标 | 6+ 架构 | x86_64 / aarch64 / armv7 |
| 维护成本 | 低 | 低 |
| 代码量 | ~20K 行 | ~15K 行 |

---

### 5.2 方案 B：直接机器码（x86_64 单目标）

#### 5.2.1 架构

```
HIR
    │
    ▼
┌─────────────────────────────────────────┐
│  MIR（寄存器式 CFG，已存在）              │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  指令选择器（新增）                        │
│  - MIR Instr → x86_64 MachineInstr       │
│  - 类型感知（i32/i64/f32/f64）           │
│  - 调用约定适配（MSVC / SysV）            │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  寄存器分配器（新增）                      │
│  - 线性扫描分配（Linear Scan）           │
│  - 物理寄存器：RAX/RBX/RCX/RDX/R8-R15   │
│  - 溢出到栈（Spill/Reload）              │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  机器码发射器（新增）                      │
│  - 指令编码（REX 前缀 + ModRM + SIB）    │
│  - 栈帧布局 + Prologue/Epilogue          │
│  - 立即数编码（32/64 位）                 │
│  - 重定位项（RELOC_32S / RELOC_64）      │
└─────────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────────┐
│  对象文件/段提取（新增）                   │
│  - PE/COFF 格式输出（Windows）            │
│  - 或：直接提取 .text 段 → Blob          │
└─────────────────────────────────────────┘
    │
    ▼
  .auc v4（code 段 + desc 段）
```

#### 5.2.2 x86_64 指令集子集

仅需实现以下指令即可覆盖 Aura 语言需求：

**整数运算（R/M 32 和 R/M 64）**

| 操作 | x86_64 指令 | 编码 |
|------|------------|------|
| `add` | `ADD r32, r32` / `ADD r64, r64` | 01 00 / 03 01 |
| `sub` | `SUB r32, r32` / `SUB r64, r64` | 29 2B / 2B 29 |
| `mul` | `IMUL r32, r32` / `IMUL r64, r64` | 0F AF |
| `div` | `IDIV r32, r32` / `IDIV r64, r64` | 0F BF（需 CQO） |
| `rem` | `IDIV` + 取 EDX/RDX | 同上 |
| `and` | `AND r32, r32` | 21 25 |
| `or` | `OR r32, r32` | 09 0D |
| `xor` | `XOR r32, r32` | 31 35 |
| `shl` | `SHL r32, r/m32` | C1 /D3 |
| `shr` | `SHR r32, r/m32` | C1 /1 / D3 /1 |
| `neg` | `NEG r32` | F7 /3 |

**浮点运算（XMM 寄存器）**

| 操作 | x86_64 指令 | 编码 |
|------|------------|------|
| `add` | `ADDSS xmm, xmm` / `ADDPD xmm, xmm` | 0F 58 |
| `sub` | `SUBSS xmm, xmm` / `SUBPD xmm, xmm` | 0F 5C |
| `mul` | `MULSS xmm, xmm` / `MULPD xmm, xmm` | 0F 59 |
| `div` | `DIVSS xmm, xmm` / `DIVPD xmm, xmm` | 0F 5E |

**比较与分支**

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `cmp` | `CMP r32, r/m32` | 设置标志位 |
| `test` | `TEST r32, r/m32` | 标志位设置 |
| `je` / `jne` | `JZ rel32` / `JNZ rel32` | 短跳转 |
| `jl` / `jge` | `JL rel32` / `JGE rel32` | 有符号比较 |
| `jle` / `jg` | `JLE rel32` / `JG rel32` | 有符号比较 |
| `ucomiss` | `UCOMISS xmm, xmm` | 浮点比较 |
| `jb` / `ja` | 无符号比较跳转 | 用于 `u32` 比较 |

**函数调用**

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `call` | `CALL rel32` / `CALL r/m64` | 直接/间接调用 |
| `ret` | `RET` | 返回 |
| `jmp` | `JMP rel32` / `JMP r/m64` | 跳转 |

**内存操作**

| 操作 | x86_64 指令 | 说明 |
|------|------------|------|
| `mov` | `MOV r32, r/m32` / `MOV r64, r/m64` | 寄存器/内存 |
| `mov` | `MOV r/m32, imm32` | 立即数 |
| `lea` | `LEA r64, [r/m64]` | 取地址 |
| `push` | `PUSH r64` | 入栈 |
| `pop` | `POP r64` | 出栈 |

**栈帧（Prologue/Epilogue）**

```nasm
; Prologue (Windows x64 MSVC)
push rbp          ; 保存帧指针
mov rbp, rsp      ; 建立帧指针
sub rsp, 0x30     ; 分配 48 字节（对齐 16）
; ... 保留 32 字节 shadow space（MSVC 要求）

; Epilogue
leave             ; mov rsp, rbp; pop rbp
ret
```

#### 5.2.3 寄存器分配策略

**采用线性扫描（Linear Scan）寄存器分配：**

```
1. 计算每个虚拟寄存器的活跃区间（live interval）
2. 按区间起点排序
3. 顺序扫描，为每个区间分配物理寄存器
4. 区间重叠 → 溢出到栈（spill to stack）
5. 重建指令（将溢出值替换为 load/store）
```

**物理寄存器集合（x86_64）：**

| 用途 | 寄存器 |
|------|--------|
| 通用目的（分配器） | RAX, RBX, RCX, RDX, RSI, RDI, R8-R15 |
| 保留（调用约定） | RSP（栈指针）, RBP（帧指针） |
| 浮点（分配器） | XMM0-XMM15 |
| 保留（返回值） | RAX/RDX（整数返回）, XMM0（浮点返回） |

#### 5.2.4 调用约定适配

**Windows x64 (MSVC)：**

```
参数：RCX, RDX, R8, R9（前 4 个）
栈：第 5 个及后续参数
对齐：RSP % 16 == 8（调用时）/ 0（返回时）
Shadow Space：每个被调函数预留 32 字节
返回值：RAX（整数）/ XMM0（浮点）/ RAX:RDX（128 位）
调用后清理：调用者负责（caller cleanup）
```

**System V AMD64 (Linux/macOS)：**

```
参数：RDI, RSI, RDX, RCX, R8, R9（前 6 个）
栈：第 7 个及后续参数
对齐：RSP % 16 == 0（调用前）
返回值：RAX（整数）/ XMM0（浮点）/ RAX:RDX（128 位）
调用后清理：被调方负责（callee cleanup）
```

#### 5.2.5 ARC 发射策略

```rust
// MIR 指令：Retain { src: Reg }
// 发射为：
//   call aura_arc_increment    ; 原子 +1
//   ; 参数：寄存器 src 的值（对象指针）

// MIR 指令：Release { src: Reg }
// 发射为：
//   call aura_arc_decrement    ; 原子 -1，归零时释放
//   ; 参数：寄存器 src 的值（对象指针）
```

**关键问题**：ARC 插桩位置由 MIR 降级阶段确定（已在 `mir.rs` 中实现），后端只需将 `Retain`/`Release` 指令映射为 `call` 即可。

#### 5.2.6 异常处理策略（简化方案）

**方案 A：setjmp/longjmp（最小实现）**

```
try {
    ...body...
} catch (e) {
    ...handler...
}

// 发射：
// 1. 在 try 前保存当前帧指针到异常栈
// 2. 在 try 中调用 setjmp（返回非零表示从 longjmp 返回）
// 3. 异常路径调用 longjmp 跳回 setjmp 点
```

**优点**：实现简单，无需栈展开表  
**缺点**：C 标准仅保证 setjmp/longjmp 的基本语义，跨线程不安全

**方案 B：表驱动 EH（完整实现，工作量大）**

```
// 生成 .eh_frame 段（Linux）或 .pdata/.xdata 段（Windows）
// 在异常路径发射 __Unwind_Resume（Linux）或 __C_specific_handler（Windows）
// 需要实现完整的栈展开表格式
```

#### 5.2.7 段提取与 Blob 生成

```rust
// 不生成完整的 .o 文件，直接提取 .text 段：
// 1. 按函数顺序排列指令
// 2. 计算每个函数的起始偏移（对齐 16 字节）
// 3. 生成 AuraFuncDesc { name, offset, param_count, ... }
// 4. 包装为 JitValue ABI 入口（args, out, argc, dispatch_table）
```

---

### 5.3 方案 C：混合策略（★ 实际推荐）

#### 5.3.1 分层后端设计

```
                    ┌─────────────────────────┐
                    │      HIR → MIR          │
                    │   (已有，不变)           │
                    └────────────┬────────────┘
                                 │
                    ┌────────────┴────────────┐
                    │    后端选择器（新增）     │
                    │  根据目标/优化级别路由    │
                    └──┬─────────┬─────────┬──┘
                       │         │         │
              ┌────────┴──┐  ┌──┴──────┐  ┌┴────────────┐
              │ LLVM 后端  │  │ Cranelift│  │ C 后端       │
              │ (现有)     │  │ AOT 后端 │  │ (现有)       │
              │ -O2/-O3   │  │ (新增)   │  │ (备选)       │
              └───────────┘  └─────────┘  └─────────────┘
              高性能 AOT    自包含 AOT     无 LLVM 降级
```

**路由规则：**

| 条件 | 后端 |
|------|------|
| 有 LLVM 安装 + 高性能需求 | LLVM O2 |
| 无 LLVM 安装 + 有 Rust 工具链 | Cranelift AOT |
| 无 LLVM + 无 Rust + 有 C 编译器 | C 后端 |
| 无 LLVM + 无 Rust + 有 x86_64 + 需要 Blob | x86_64 直接机器码 |

#### 5.3.2 代码复用策略

```
compiler/src/codegen/
├── aot/           // LLVM 后端（保留，现有代码）
│   ├── emit.rs
│   ├── linker.rs
│   └── ...
├── cranelift_aot/ // Cranelift AOT 后端（新增，~15K 行）
│   ├── ir_builder.rs    // MIR → Cranelift IR
│   ├── types.rs         // 类型映射
│   ├── abi.rs           // 调用约定
│   ├── runtime.rs       // Runtime 导入
│   ├── object.rs        // 段提取
│   └── emit.rs          // Blob 输出
├── c_backend.rs   // C 后端（保留，现有代码）
├── native_x64/    // x86_64 直接机器码（可选，~10K 行）
│   ├── instr.rs       // 指令定义
│   ├── register.rs    // 寄存器分配器
│   ├── encoder.rs     // 指令编码
│   ├── frame.rs       // 栈帧布局
│   ├── abi.rs         // 调用约定
│   └── emit.rs        // 机器码输出
└── backend.rs     // 后端选择器（新增，~200 行）
```

#### 5.3.3 分阶段实施

| 阶段 | 时间 | 目标 | 产出 |
|------|------|------|------|
| Phase 1 | 1-2 月 | Cranelift AOT 基本功能 | MIR → Cranelift IR（算术/控制流/调用） |
| Phase 2 | 3-4 月 | Cranelift AOT 完整功能 | ARC + 闭包 + 异常处理 + Blob 输出 |
| Phase 3 | 5-6 月 | 后端选择器 + 降级 | 后端路由 + 与现有 AOT 集成 |
| Phase 4（可选） | 7-12 月 | x86_64 直接机器码 | 无 LLVM + 无 Cranelift 降级路径 |

---

## 6. 风险与缓解措施

### 6.1 技术风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| Cranelift AOT 性能不足 | 中 | 高 | 保留 LLVM 后端作为高性能路径 |
| x86_64 指令编码错误 | 高 | 高 | 与 LLVM 生成代码对比验证 + 单元测试 |
| 寄存器分配 bug（溢出错误） | 中 | 高 | 使用现有线性扫描实现 + 充分测试 |
| 调用约定不一致 | 中 | 高 | 编写调用约定测试套件（C ABI 兼容测试） |
| ARC 内存泄漏 | 中 | 高 | 复用 MIR 降级阶段的 ARC 插桩 + 运行时间检查 |
| 异常处理不完整 | 高 | 中 | 初始采用 setjmp/longjmp 简化方案 |

### 6.2 维护风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| 新目标架构支持成本高 | 高 | 中 | Cranelift 已内置多目标支持 |
| 自研后端性能退步 | 中 | 中 | 保留 LLVM 后端，性能测试对比 |
| 团队成员技能不足 | 中 | 高 | 需要 2+ 年编译器开发经验的工程师 |

### 6.3 性能风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| 无优化代码性能差距过大 | 高 | 高 | Cranelift 有内置优化；保留 LLVM 路径 |
| 编译速度预期过高 | 低 | 低 | 实际测试后调整预期 |
| 代码体积膨胀 | 中 | 低 | Cranelift 优化级别可调（`opt_level: "size"`） |

---

## 7. 结论与建议

### 7.1 核心结论

| 结论 | 说明 |
|------|------|
| **技术可行** | MIR + Blob 基础设施完备，路径清晰 |
| **工作量巨大** | 三目标直接机器码需 184-289 人天（10-16 个月） |
| **性能损失明显** | 无优化直接发射比 LLVM O2 慢 2-5 倍 |
| **维护成本极高** | 每新增一个目标架构需 4-8 个月 |
| **建议分层策略** | Cranelift AOT（推荐）+ LLVM（高性能）+ C（降级） |

### 7.2 推荐实施路径

**首选：方案 C —— 混合策略（分层后端）**

```
┌────────────────────────────────────────────────────────────┐
│  优先级 1（立即）：Cranelift AOT 后端（方案 A）              │
│  ─────────────────────────────────────────                │
│  • 工作量：80-120 人天（4-6 个月）                          │
│  • 收益：去除 LLVM 依赖 + 编译速度提升 4x                   │
│  • 风险：低（Cranelift 已验证，JIT 已在使用）               │
│  • 性能：0.6-0.8x（略慢于 LLVM O2，可接受）                │
│                                                            │
│  优先级 2（可选）：x86_64 直接机器码（方案 B）               │
│  ─────────────────────────────────────────                │
│  • 工作量：74-114 人天（4-6 个月）                          │
│  • 收益：完全无外部依赖，自包含编译                          │
│  • 风险：高（需自研指令选择/寄存器分配/EH）                  │
│  • 性能：0.4-0.6x（明显慢于 LLVM O2，仅适合嵌入式场景）    │
│                                                            │
│  优先级 3（保留）：LLVM 后端（现有）                        │
│  ─────────────────────────────────────────                │
│  • 维护成本：低（现有代码，无需修改）                       │
│  • 收益：最高性能（O2/O3 优化）                             │
│  • 限制：需外部 LLVM 安装                                  │
└────────────────────────────────────────────────────────────┘
```

### 7.3 不推荐完全替换 LLVM 的理由

1. **LLVM 的优化能力不可替代**：20+ 优化 pass 的研发成本远超自研后端
2. **维护成本指数增长**：每新增一个目标架构需重复 80% 的工作
3. **性能差距不可忽视**：无优化代码在计算密集场景慢 2-5 倍
4. **已有 Cranelift 作为更好的替代**：Cranelift 提供接近 LLVM 的功能，但依赖更小
5. **AOT Blob 基础设施已就绪**：可直接复用，无需重新设计运行时

### 7.4 一句话总结

> **绕过 LLVM 直生成机器码在技术上可行，但全面替换 LLVM 的代价远超收益。推荐采用分层策略：Cranelift AOT 作为默认自包含后端，LLVM 作为高性能可选后端，x86_64 直接机器码作为极致轻量降级路径。**

---

## 附录：参考文件

| 文件 | 行数 | 作用 |
|------|------|------|
| `compiler/src/codegen/mir.rs` | 1252 | MIR 定义与 HIR → MIR 降级 |
| `compiler/src/codegen/aot/emit.rs` | 2759 | LLVM IR 文本生成器 |
| `compiler/src/codegen/aot/mod.rs` | 354 | AOT 编译控制器 |
| `compiler/src/codegen/aot/c_backend.rs` | 749 | C 后端（备选方案） |
| `compiler/src/codegen/aot/types.rs` | 214 | 类型映射器 |
| `compiler/src/codegen/aot/target.rs` | 321 | 目标三元组 |
| `compiler/src/codegen/aot/runtime.rs` | 317 | Runtime 函数声明 |
| `compiler/src/vm/jit.rs` | 807 | Cranelift JIT 后端 |
| `compiler/src/vm/abi.rs` | 210 | JitValue / AotEntry ABI |
| `compiler/src/vm/aot_runtime.rs` | 1130 | AOT 机器码加载运行时 |
| `compiler/src/codegen/emit.rs` | 849 | MIR → 字节码发射 |
| `compiler/src/codegen/opcode.rs` | 1285 | 字节码指令集 |
