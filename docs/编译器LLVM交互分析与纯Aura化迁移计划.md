# Aura 编译器 LLVM 交互分析与纯 Aura 化迁移计划

> **文档性质**：技术分析 + 开发计划
> **方案选择**：方案四——分阶段迁移路径
> **约束**：本文仅提供分析与计划，不修改任何代码
> **分析日期**：2026-09-10
> **预计执行周期**：约 14–15 人月（单人全职）
>
> **⚠️ 方案调整（2026-09-11）**：**AOT 路径改为「Aura 侧自研 LLVM IR 生成 + 直连 LLVM 工具链生成机器码」，
> 跳过（不移植、不调用、不依赖）Rust AOT 后端。** 详见 §4.8「决策变更记录」。

---

## 目录

- [〇、结论摘要](#〇结论摘要)
- [一、当前 Rust 编译器与 LLVM 的交互机制（背景）](#一当前-rust-编译器与-llvm-的交互机制背景)
- [二、MIR → LLVM IR 的实际路径澄清（背景）](#二mir--llvm-ir-的实际路径澄清背景)
- [三、纯 Aura 化技术方案选型](#三纯-aura-化技术方案选型)
- [四、详细达成路径与开发计划](#四详细达成路径与开发计划)
  - [4.1 迁移总览与依赖关系](#41-迁移总览与依赖关系)
  - [4.2 Phase 0：基础设施与工具链准备](#42-phase-0基础设施与工具链准备)
  - [4.3 Phase 1：前端 Aura 化（Lexer + Parser + AST）](#43-phase-1前端-aura-化lexer--parser--ast)
  - [4.4 Phase 2：语义分析与 HIR 生成器 Aura 化](#44-phase-2语义分析与-hir-生成器-aura-化)
  - [4.5 Phase 3：MIR 生成器与优化器 Aura 化](#45-phase-3mir-生成器与优化器-aura-化)
  - [4.6 Phase 4：VM 字节码发射与解释器 Aura 化](#46-phase-4vm-字节码发射与解释器-aura-化)
  - [4.7 Phase 5：VM 自举（最小引导层保留）](#47-phase-5vm-自举最小引导层保留)
  - [4.8 Phase 6：AOT 后端 Aura 自研（自产 IR + 直连 LLVM）](#48-phase-6aot-后端-aura-自研自产-ir--直连-llvm)
  - [4.9 Phase 6.5：AOT 后端适配与修复（AOT Hardening）](#49-phase-65aot-后端适配与修复aot-hardening)
  - [4.10 Phase 7：JIT 编译（Cranelift 直连机器码）Aura 化](#410-phase-7jit-编译cranelift-直连机器码aura-化)
  - [4.11 Phase 8：核心库与标准库 Aura 化](#411-phase-8核心库与标准库-aura-化)
  - [4.12 Phase 9：LLVM C API 直连（可选优化）](#412-phase-9llvm-c-api-直连可选优化)
- [五、关键技术挑战与解决方案](#五关键技术挑战与解决方案)
- [六、工作量估算与里程碑](#六工作量估算与里程碑)
- [七、风险矩阵与回退策略](#七风险矩阵与回退策略)
- [八、总结](#八总结)
- [附录 A：当前代码库 LLVM 相关关键文件清单](#附录-a当前代码库-llvm-相关关键文件清单)
- [附录 B：关键 LLVM IR 生成示例](#附录-b关键-llvm-ir-生成示例)
- [附录 C：Phase 间依赖图](#附录-cphase-间依赖图)
- [附录 D：验证用例清单](#附录-d验证用例清单)

---

## 〇、结论摘要

### 核心决策

选择**方案四：分阶段迁移路径**，采用 **10 个 Phase**（Phase 0–9，含 2026-09-11 新增的
**Phase 6.5：AOT 后端适配与修复**）的渐进式迁移策略，每个 Phase 独立可验证、可回退。

### 核心原则：独立代码层 + 保留 Rust 实现

> **⚠️ 关键约束**：Aura 编译器作为**独立代码层**开发，当前 Rust 编译器**完全保留不动**。
>
> - **Rust 编译器**：`compiler/` 目录，保持现状，作为 fallback 和参考实现
> - **Aura 编译器**：`aura/compiler/` 目录，独立开发，逐步迁移
> - **并行运行**：两者可并行构建、并行测试，互不影响
> - **切换策略**：仅在 Aura 编译器完全验证通过后，才考虑切换为主路径

这个原则确保：
1. **零风险**：Rust 编译器始终可用，迁移过程不会破坏现有编译能力
2. **可对比**：同一份源码可同时用 Rust/Aura 编译器编译，输出对比验证
3. **可回退**：任何 Phase 失败时，只需放弃 Aura 编译器代码，Rust 编译器不受影响
4. **渐进式**：每个 Phase 独立交付，可随时暂停而不影响主线开发
5. **AOT 零依赖 Rust 后端**：Aura 化 AOT 自产 LLVM IR 并**直连 LLVM** 生成机器码，
   不移植、不调用 Rust AOT 后端（`compiler/src/codegen/aot/` 仅作行为参照）

### 关键技术选择

| 维度 | 决策 | 理由 |
|------|------|------|
| **代码组织** | `aura/compiler/` 独立目录 | 与 `compiler/` 并行，互不干扰 |
| **Rust 编译器** | 完全保留，不修改 | 作为 fallback 和参考实现 |
| **LLVM 后端方式** | **Aura 侧自研文本 IR + 直连 LLVM**（`llc`/`clang` 子进程） | Aura 自己产出 LLVM IR 并**直接调用 LLVM 生成机器码**，无需绑定 LLVM C API |
| **Rust AOT 后端** | **不进入 Aura 编译路径**（仅作行为对照） | 跳过 Rust AOT 后端；避免 Aura 化 AOT 被 Rust 实现绑死，只要求**行为等价** |
| **JIT 后端方式** | Cranelift 进程内原生码（Phase 7） | 无外部工具链、进程内即时编译，补齐 VM 与 AOT 之间的执行档 |
| **IR 生成入口** | AOT 走 HIR 直发（绕过 MIR） | 与现有实现一致，避免冗余转换 |
| **MIR 定位** | VM 解释器 / JIT 共用（AOT 不使用 MIR） | Cranelift IR 与 MIR 的寄存器式 CFG 语义接近，可复用降级结果 |
| **引导层** | 保留最小 Rust 引导层（Layer 0-A） | 解决鸡生蛋问题 |
| **FFI 方式** | Aura `extern "C"` 调用 C ABI | 已有完整支持 |
| **构建系统** | 双编译器并行构建 | Rust/Aura 编译器独立构建，互不依赖 |

### 关键数据

| 指标 | 数值 |
|------|------|
| 当前 Rust 编译器规模 | 107 个 .rs 文件，约 43,000 行（`compiler/`） |
| 已有 Aura 核心库/标准库文件 | 45 个 .aura 文件（`aura/core/aura/lang/`：核心类型 + `collection/` + `coroutine/` + `std/`） |
| 需要迁移的 Rust 文件 | 约 45 个核心文件 → `aura/compiler/` |
| 预计 Aura 代码量 | 约 24,000 行（`aura/compiler/`） |
| 预计总工期 | 14–15 人月 |
| Phase 数量 | 10 个（含 2 个可选 Phase；其中 Phase 6.5 为 AOT 适配修复阶段） |
| **Rust 编译器** | **完全保留，零修改** |
| **Aura 编译器目录** | **`aura/compiler/`（独立）** |

---

## 一、当前 Rust 编译器与 LLVM 的交互机制（背景）

### 1.1 总体架构

```
┌──────────────────────────────────────────────────────────────────┐
│  Aura 源码 (.aura)                                                │
└──────────────────────┬───────────────────────────────────────────┘
                       ↓
┌──────────────────────────────────────────────────────────────────┐
│  前端（compiler/src/{lexer,parser,sema}.rs）                       │
│  Lexer → Parser → AST → Sema → HirProgram                        │
└──────────────────────┬───────────────────────────────────────────┘
                       ↓
        ┌──────────────┴──────────────┐
        ↓                              ↓
┌───────────────────┐        ┌───────────────────────┐
│  VM 路径（默认）    │        │  AOT 路径（--llvm）    │
│  HIR → MIR         │        │  HIR 直接 → LLVM IR   │
│  → 栈式字节码      │        │  文本（绕过 MIR）      │
│  → VM 解释 / JIT  │        │  ↓                     │
│  (Cranelift)       │        │  写 .ll 文件           │
└───────────────────┘        │  ↓                     │
                             │  llc -mtriple → .o     │
                             │  ↓                     │
                             │  clang → .exe          │
                             └───────────────────────┘
```

### 1.2 关键设计决策：不依赖 inkwell / llvm-sys

项目采用"**文本 LLVM IR 生成 + 外部 llc/clang 子进程调用**"方案：

- **原因**：inkwell 0.10.0 最高支持 LLVM 19，项目使用 LLVM 23.1.0
- **优势**：与 LLVM 版本解耦，跨版本兼容
- **文件位置**：`compiler/src/codegen/aot/`（共 11 个子模块）

### 1.3 五步式 AOT 流程（Rust 侧现状，仅作背景）

```
HIR → emit_program() → LLVM IR 文本 → 写 .ll → llc → .o → clang → .exe
```

> **Aura 侧不走这条路**：Aura 化 AOT 自行完成「HIR → LLVM IR 文本」的发射，
> 并**直接调用 LLVM 工具链**（`llc` → `clang`）产出机器码/可执行文件，
> **不经过 Rust AOT 后端**（`compiler/src/codegen/aot/`）。该目录对 Aura 侧
> 仅具有「行为参照」意义，不是迁移源、也不是运行时依赖。详见 §4.8。

### 1.4 类型映射表（TypeMapper）

| Aura 类型 | LLVM 类型 |
|-----------|-----------|
| `Int` | `i32` |
| `Long` | `i64` |
| `Short` | `i16` |
| `Byte` / `U8` | `i8` |
| `Float` | `float` |
| `Double` | `double` |
| `Boolean` / `Bool` | `i1` |
| `Char` | `i16` |
| `Unit` / `Void` | `""`（void） |
| `String` | `{ i8*, i64 }`（长度感知） |
| `Any` / `Nothing` | `i8*` |
| `Pointer<T>` | `ptr`（不透明指针） |
| `Nullable<Scalar>` | `{ <scalar>, i1 }` |
| 用户结构体 | `%struct.<Name>` |

### 1.5 LLVM 工具探测

五级探测顺序：
1. `AotOptions.llvm_home` 显式指定
2. 运行时环境变量 `AURA_LLVM_HOME`
3. 编译时配置 `AURA_CONFIG_LLVM_HOME`
4. 编译时搜索路径 `AURA_CONFIG_LLVM_SEARCH_PATHS`
5. 系统 `PATH`（`where` / `which`）

---

## 二、MIR → LLVM IR 的实际路径澄清（背景）

### 2.1 重要发现：MIR 不流向 LLVM IR

代码库中 `codegen/mir.rs` 生成的 MIR **仅服务于 VM 字节码路径**，AOT 后端**直接从 HIR 发射 LLVM IR 文本**。

### 2.2 MIR 的实际流向

```
HIR → lower_program() → Vec<MirFunction> → codegen/emit.rs → 栈式字节码 → VM/JIT
```

### 2.3 HIR → LLVM IR 的直发路径

```rust
// compiler/src/codegen/aot/mod.rs
pub fn compile_program(&self, program: &crate::ast::Program, ...) -> Result<AotOutput, AotError> {
    let mut hir = desugar_program(program);  // AST → HIR
    synthesize_main_if_missing(&mut hir);     // 合成 main
    mono_hir(&mut hir);                       // 泛型单态化
    inline_hir(&mut hir);                     // 内联
    fold_hir(&mut hir);                       // 常量折叠
    self.compile(&hir, ...)                   // HIR → LLVM IR 文本 → 目标文件
}
```

### 2.4 为什么不从 MIR 走？

1. **MIR 是为栈式字节码设计的**：`LoadLocal`/`StoreLocal` 假设所有变量在寄存器中
2. **LLVM IR 已是 SSA + CFG**：再走 MIR 是冗余
3. **MIR 缺少类型信息**：`Reg = usize` 不带类型
4. **历史原因**：MIR 早于 AOT 后端

---

## 三、纯 Aura 化技术方案选型

### 3.1 四种方案对比

| 方案 | 描述 | 开发量 | 性能 | 复杂度 | 推荐度 |
|------|------|--------|------|--------|--------|
| 方案一 | 文本 IR + 子进程（**Aura 自研 IR 生成，直连 LLVM**） | 中 | 中 | 低 | ⭐⭐⭐⭐ |
| 方案二 | C FFI 直连 LLVM C API | 高 | 高 | 高 | ⭐⭐⭐ |
| 方案三 | Aura-to-C 转译（fallback） | 低 | 低 | 低 | ⭐⭐ |
| **方案四** | **分阶段迁移** | **高** | **高** | **中** | **⭐⭐⭐⭐⭐** |

### 3.2 选择方案四的理由

1. **风险可控**：每个 Phase 独立可验证、可回退
2. **渐进式改进**：每完成一个 Phase 就获得一个可交付成果
3. **保留 fallback**：Rust 编译器始终可用
4. **技术路线清晰**：Phase 1–8 用方案一（Aura 自研文本 IR + **直连 LLVM** 生成机器码），
   Phase 9 可选升级到方案二；**AOT 全程跳过 Rust AOT 后端**
5. **与现有文档对齐**：符合 `完全Aura化技术方案-final.md` 的分层架构

### 3.3 方案四的技术路线

```
Phase 0: 准备
    ↓
Phase 1-4: 前端 + IR 生成器 Aura 化（纯逻辑，无 FFI 依赖）
    ↓
Phase 5: VM 自举（需要最小 Rust 引导层）
    ↓
Phase 6: AOT 后端 Aura 自研（自产 LLVM IR + 直连 LLVM 生成机器码；跳过 Rust AOT 后端）
    ↓
Phase 6.5: AOT 后端适配与修复（表示统一 / 运行库契约 / std 签名表 / IR 门禁）
    ↓
Phase 7: JIT 编译（Cranelift 直连机器码）Aura 化  ← 补全「VM / JIT / AOT」三执行路径
    ↓
Phase 8: 核心库与标准库 Aura 化
    ↓
Phase 9: LLVM C API 直连（可选，用方案二）
```

### 3.4 并行开发模型：Rust 与 Aura 编译器共存

#### 目录结构

```
AuraLang/
├── compiler/                          # Rust 编译器（保留不动）
│   ├── src/
│   │   ├── lexer.rs                   # Rust Lexer
│   │   ├── parser.rs                  # Rust Parser
│   │   ├── sema/                      # Rust 语义分析
│   │   ├── codegen/                   # Rust 代码生成
│   │   │   ├── hir.rs
│   │   │   ├── mir.rs
│   │   │   ├── emit.rs                # VM 字节码
│   │   │   └── aot/                   # AOT LLVM 后端
│   │   └── vm/                        # Rust VM
│   └── Cargo.toml
│
├── aura/                              # Aura 编译器（独立开发）
│   ├── compiler/                      # ← 新增：Aura 编译器源码
│   │   ├── lexer/
│   │   │   └── Lexer.aura             # Aura Lexer
│   │   ├── parser/
│   │   │   └── Parser.aura            # Aura Parser
│   │   ├── sema/
│   │   │   └── TypeChecker.aura       # Aura 语义分析
│   │   ├── hir/
│   │   │   └── Hir.aura               # Aura HIR 生成
│   │   ├── mir/
│   │   │   └── Mir.aura               # Aura MIR 生成
│   │   ├── codegen/
│   │   │   └── Emit.aura              # Aura 字节码发射
│   │   ├── vm/
│   │   │   └── Interp.aura            # Aura VM 解释器
│   │   ├── aot/
│   │   │   └── Emit.aura              # Aura 自研 AOT IR 发射器（直连 LLVM）
│   │   └── main.aura                  # Aura 编译器入口
│   └── core/                          # 核心库 / 标准库（Phase 8 唯一真相源）
│       └── aura/lang/
│           ├── *.aura                 # 核心类型（Int/String/Boolean/Char/...）
│           ├── collection/            # Array/ArrayList/List/Map/HashMap/Set/HashSet/...
│           ├── coroutine/             # Coroutine/Actor
│           └── std/                   # Math/IO/FileSystem/Json/.../Channel
│
├── cli/                               # CLI（双编译器支持）
│   └── src/
│       └── main.rs                    # 支持 --aura-compiler 标志
│
└── scripts/
    └── build-compiler.aura            # 构建 Aura 编译器
```

#### 构建流程

```
┌──────────────────────────────────────────────────────────────────┐
│  Rust 编译器构建（不变）                                            │
│  cargo build --release → target/release/aura                     │
└──────────────────────────────────────────────────────────────────┘
                              ↓
┌──────────────────────────────────────────────────────────────────┐
│  Aura 编译器构建（新增）                                            │
│  1. 用 Rust 编译器编译 Aura 编译器源码：                             │
│     aura target/release/aura --aot aura/compiler/**/*.aura       │
│     → aura-compiler.auc（字节码）                                  │
│                                                                  │
│  2. 嵌入字节码到 aura-compiler 可执行文件：                          │
│     aura target/release/aura --emit-exe \                         │
│     --input aura-compiler.auc \                                   │
│     --output aura-compiler                                         │
│     → aura-compiler（独立可执行文件）                               │
│                                                                  │
│  3. 运行 Aura 编译器：                                              │
│     aura-compiler --aot user-program.aura                         │
│     → user-program.exe（用 Aura 编译器编译的用户程序）                │
└──────────────────────────────────────────────────────────────────┘
```

#### 双编译器对比测试

```bash
# 用 Rust 编译器编译（参照实现）
aura --rust-compiler --aot tests/example.aura → output_rust/example.exe

# 用 Aura 编译器编译（自产 IR + 直连 LLVM；不经过 Rust AOT 后端）
aura-compiler --aot tests/example.aura → output_aura/example.exe

# 行为差分（等价性判据）：运行输出与退出码一致
output_rust/example.exe > rust.out ; echo $? > rust.code
output_aura/example.exe > aura.out ; echo $? > aura.code
diff rust.out aura.out && diff rust.code aura.code
```

> **判据变更**：AOT 的等价性以**运行行为**为准；LLVM IR 文本与目标文件**不再做逐字节比对**
> （理由见 §4.8 决策变更记录）。VM/JIT 侧仍可做 MIR / 字节码级对比。

#### 关键约束

| 约束 | 说明 |
|------|------|
| **Rust 编译器零修改** | `compiler/` 目录完全保留，不修改任何文件 |
| **Aura 编译器独立目录** | `aura/compiler/` 是新增目录，不影响现有代码 |
| **构建系统独立** | Rust/Aura 编译器各自独立构建，互不依赖 |
| **CLI 双模式** | `aura` CLI 支持 `--rust-compiler` 和 `--aura-compiler` 两种模式 |
| **测试对比** | 每个 Phase 完成后运行对比测试；**AOT 以「运行行为等价」判定（输出 / 退出码），不做 IR / 目标文件逐字节比对** |
| **回退简单** | 任何 Phase 失败，只需删除 `aura/compiler/` 目录，Rust 编译器不受影响 |

---

## 四、详细达成路径与开发计划

### 4.1 迁移总览与依赖关系

```
Phase 0 (准备) ──────────────────────────────────────────────┐
    │                                                          │
    ├──→ Phase 1 (Lexer/Parser) ──→ Phase 2 (Sema/HIR)       │
    │                              │                           │
    │                              ├──→ Phase 3 (MIR/Opt)     │
    │                              │         │                 │
    │                              └─────────┼→ Phase 4 (VM)  │
    │                                        │        │        │
    │                                        │        ├→ Phase 5 (自举)
    │                                        │        │
    │                                        │        └→ Phase 7 (JIT / Cranelift)
    │                                        │                 │
    │                                        └──→ Phase 6 (AOT LLVM)
    │                                                │
    │                                                └──→ Phase 6.5 (AOT 适配与修复)
    │                                                        │
    │                                                        └──→ Phase 8 (核心库/标准库)
    │                                                                │
    │                                                                └──→ Phase 9 (LLVM C API, 可选)
    └──────────────────────────────────────────────────────────┘
```

### 4.2 Phase 0：基础设施与工具链准备

#### 目标

建立纯 Aura 编译器的开发基础设施，**保持 Rust 编译器完全不动**，新增独立的 `aura/compiler/` 目录。

#### 核心原则

- **Rust 编译器零修改**：`compiler/` 目录完全保留，不修改任何文件
- **Aura 编译器独立目录**：`aura/compiler/` 是新增目录，不影响现有代码
- **并行构建**：Rust/Aura 编译器各自独立构建，互不依赖
- **对比测试**：每个 Phase 完成后，运行对比测试验证输出一致性

#### 任务清单

| # | 任务 | 文件/目录 | 预估 | 依赖 |
|---|------|-----------|------|------|
| 0.1 | 创建 `aura/compiler/` 目录结构 | `aura/compiler/` | 0.5d | — |
| 0.2 | 定义 Aura 编译器的包结构 | `aura/compiler/{lexer,parser,sema,...}/` | 1d | 0.1 |
| 0.3 | 编写 Aura 测试框架 | `aura/compiler/test/TestRunner.aura` | 2d | 0.2 |
| 0.4 | 编写构建脚本（调用 Rust 编译器编译 Aura 编译器） | `scripts/build-aura-compiler.sh` | 1d | 0.3 |
| 0.5 | 建立源码快照对比机制 | `tests/snapshots/` | 1d | 0.3 |
| 0.6 | 配置 CI 流水线（并行运行 Rust/Aura 编译器测试） | `.github/workflows/` | 1d | 0.5 |
| 0.7 | 编写 Phase 0 验证用例 | `tests/phase0_tests.aura` | 0.5d | 0.4 |

#### 交付物

- `aura/compiler/` 目录结构（独立于 `compiler/`）
- `TestRunner.aura`（Aura 测试框架）
- `build-aura-compiler.sh`（构建脚本，调用 Rust 编译器）
- CI 流水线配置（并行运行 Rust/Aura 编译器测试）

#### 验证标准

- [ ] `TestRunner.aura` 可运行简单测试用例
- [ ] `build-aura-compiler.sh` 可调用 Rust 编译器编译 Aura 编译器源码
- [ ] CI 可同时运行 Rust 编译器和 Aura 编译器测试
- [ ] **Rust 编译器 `cargo build` 仍然通过**（确认未被修改）
- [ ] **Rust 编译器 `cargo test` 仍然通过**（确认功能未受影响）

#### 目录结构

```
aura/compiler/                      # ← 新增：Aura 编译器独立目录
├── lexer/                          # Lexer Aura 实现（Phase 1）
├── parser/                         # Parser Aura 实现（Phase 1）
├── ast/                            # AST 定义（Phase 1）
├── sema/                           # 语义分析（Phase 2）
├── hir/                            # HIR 定义与生成（Phase 2）
├── mir/                            # MIR 定义与生成（Phase 3）
├── codegen/                        # 字节码发射（Phase 4）
├── vm/                             # VM 解释器（Phase 5）
├── aot/                            # AOT 后端：自研 IR + 直连 LLVM（Phase 6）
├── jit/                            # JIT 编译器 / Cranelift 后端（Phase 7）
├── test/                           # 测试框架
└── main.aura                       # 编译器入口
```

> 注：**核心库/标准库不位于 `aura/compiler/`**，而在 `aura/core/aura/lang/`
> （核心类型 + `collection/` + `coroutine/` + `std/`，Phase 8 唯一真相源，详见 §4.11）。

#### 与 Rust 编译器的关系

```
compiler/                           # Rust 编译器（保留不动）
├── src/
│   ├── lexer.rs                    # Rust Lexer（参考实现）
│   ├── parser.rs                   # Rust Parser（参考实现）
│   ├── sema/                       # Rust 语义分析（参考实现）
│   ├── codegen/                    # Rust 代码生成（参考实现）
│   └── vm/                         # Rust VM（参考实现）
└── Cargo.toml

aura/compiler/                      # Aura 编译器（独立开发）
├── lexer/
│   └── Lexer.aura                  # Aura Lexer（新实现）
├── parser/
│   └── Parser.aura                 # Aura Parser（新实现）
├── sema/
│   └── TypeChecker.aura            # Aura 语义分析（新实现）
├── codegen/
│   └── Emit.aura                   # Aura 字节码发射（新实现）
└── vm/
    └── Interp.aura                 # Aura VM 解释器（新实现）
```

**关键点**：
- `compiler/` 和 `aura/compiler/` 是**平行的**两个目录，互不依赖
- Rust 编译器是**参考实现**，用于对比验证 Aura 编译器的正确性
- 任何时刻都可以同时使用两个编译器编译同一份源码

---

### 4.3 Phase 1：前端 Aura 化（Lexer + Parser + AST）

#### 目标

将 Lexer、Parser、AST 定义从 Rust 迁移到 Aura，实现纯 Aura 的词法分析和语法分析。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/lexer.rs` | 61.9 KB | `aura/lang/compiler/lexer/Lexer.aura` | 3d | P0 |
| `compiler/src/token.rs` | ~15 KB | `aura/lang/compiler/lexer/Token.aura` | 1d | P0 |
| `compiler/src/span.rs` | ~10 KB | `aura/lang/compiler/lexer/Span.aura` | 0.5d | P0 |
| `compiler/src/source_map.rs` | ~20 KB | `aura/lang/compiler/lexer/SourceMap.aura` | 2d | P1 |
| `compiler/src/parser.rs` | 136.5 KB | `aura/lang/compiler/parser/Parser.aura` | 7d | P0 |
| `compiler/src/ast.rs` | ~30 KB | `aura/lang/compiler/ast/Ast.aura` | 2d | P0 |
| `compiler/src/errors.rs` | ~15 KB | `aura/lang/compiler/errors/CompileError.aura` | 1d | P1 |
| `compiler/src/signature.rs` | ~10 KB | `aura/lang/compiler/parser/Signature.aura` | 1d | P2 |

#### 迁移策略

1. **Token 定义**：将 Rust `enum Token` 翻译为 Aura `enum` 或 `value class`
2. **Lexer**：将字符分类逻辑翻译为 Aura 函数，使用 `List<Token>` 作为输出
3. **Parser**：将递归下降解析器翻译为 Aura 函数，使用 `AstNode` 类层次结构
4. **AST**：将 Rust 的 `enum` 类型翻译为 Aura 的 `sealed class` 层次结构
5. **错误处理**：使用 Aura 的 `Try`/`Catch` 或显式错误返回

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| Token 表示 | `enum` + `value class Token` | 不可变、高效 |
| AST 节点 | `sealed class` 层次结构 | 支持 `when` 匹配 |
| 错误传播 | `Result<Ast, CompileError>` | 与 Rust 一致 |
| 源码位置 | `Span` value class（`start`, `end`, `file`） | 不可变 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 1.1 | 迁移 `token.rs` → `Token.aura` | 1d | 0.4 |
| 1.2 | 迁移 `span.rs` → `Span.aura` | 0.5d | 1.1 |
| 1.3 | 迁移 `source_map.rs` → `SourceMap.aura` | 2d | 1.2 |
| 1.4 | 迁移 `lexer.rs` → `Lexer.aura` | 3d | 1.1, 1.3 |
| 1.5 | 迁移 `ast.rs` → `Ast.aura` | 2d | 1.2 |
| 1.6 | 迁移 `parser.rs` → `Parser.aura` | 7d | 1.4, 1.5 |
| 1.7 | 迁移 `errors.rs` → `CompileError.aura` | 1d | 1.3 |
| 1.8 | 迁移 `signature.rs` → `Signature.aura` | 1d | 1.5 |
| 1.9 | 编写 Phase 1 验证用例 | 2d | 1.6 |
| 1.10 | 集成测试：Aura 编译器 vs Rust 编译器输出对比 | 2d | 1.9 |

#### 验证标准

- [ ] `Lexer.aura` 对 `tests/*.aura` 的 token 输出与 Rust 一致
- [ ] `Parser.aura` 对 `tests/*.aura` 的 AST 快照与 Rust 一致
- [ ] 错误诊断位置与 Rust 一致（行号、列号）
- [ ] 支持所有语法特性（泛型、闭包、协程、FFI 等）

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| Aura 字符串性能不足 | 使用 `StringBuilder` 预分配 |
| Parser 递归深度过大 | 迭代化关键循环（`for`/`while`） |
| AST 层次结构过于复杂 | 分阶段迁移（先核心语法，后扩展语法） |

---

### 4.4 Phase 2：语义分析与 HIR 生成器 Aura 化

#### 目标

将语义分析（类型检查、空安全检查、作用域解析）和 HIR 生成器从 Rust 迁移到 Aura。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/sema/checker.rs` | 156.9 KB | `aura/lang/compiler/sema/TypeChecker.aura` | 10d | P0 |
| `compiler/src/sema/symbol.rs` | ~20 KB | `aura/lang/compiler/sema/SymbolTable.aura` | 2d | P0 |
| `compiler/src/sema/ty.rs` | ~15 KB | `aura/lang/compiler/sema/Type.aura` | 2d | P0 |
| `compiler/src/sema/info.rs` | ~10 KB | `aura/lang/compiler/sema/TypeInfo.aura` | 1d | P1 |
| `compiler/src/codegen/hir.rs` | 210.1 KB | `aura/lang/compiler/hir/Hir.aura` | 8d | P0 |
| `compiler/src/codegen/hir/mod.rs` | — | `aura/lang/compiler/hir/desugar.aura` | 3d | P1 |
| `compiler/src/codegen/hir/mono.rs` | — | `aura/lang/compiler/hir/mono.aura` | 3d | P1 |
| `compiler/src/codegen/hir/inline.rs` | — | `aura/lang/compiler/hir/inline.aura` | 2d | P2 |
| `compiler/src/codegen/hir/fold.rs` | — | `aura/lang/compiler/hir/fold.aura` | 2d | P2 |

#### 迁移策略

1. **类型系统**：将 Rust `enum HirType` 翻译为 Aura `sealed class Type`
2. **符号表**：使用 Aura `Map<String, Symbol>` 实现作用域链
3. **类型检查**：递归遍历 AST，使用 `TypeChecker` 类维护类型环境
4. **HIR 生成**：将 AST 转换为 HIR，处理语法糖消解、泛型单态化等

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 类型表示 | `sealed class Type` 层次结构 | 支持 `when` 匹配 |
| 符号表 | `List<Map<String, Symbol>>`（作用域链） | 简单高效 |
| 错误收集 | `List<CompileError>` | 支持多错误报告 |
| 泛型单态化 | 编译期类型替换 | 与 Rust 一致 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 2.1 | 迁移 `ty.rs` → `Type.aura` | 2d | 1.5 |
| 2.2 | 迁移 `symbol.rs` → `SymbolTable.aura` | 2d | 2.1 |
| 2.3 | 迁移 `info.rs` → `TypeInfo.aura` | 1d | 2.2 |
| 2.4 | 迁移 `checker.rs` → `TypeChecker.aura` | 10d | 2.2, 2.3 |
| 2.5 | 迁移 `hir.rs` → `Hir.aura` | 8d | 2.4 |
| 2.6 | 迁移 `desugar_program` → `desugar.aura` | 3d | 2.5 |
| 2.7 | 迁移 `mono_hir` → `mono.aura` | 3d | 2.5 |
| 2.8 | 迁移 `inline_hir` → `inline.aura` | 2d | 2.5 |
| 2.9 | 迁移 `fold_hir` → `fold.aura` | 2d | 2.5 |
| 2.10 | 编写 Phase 2 验证用例 | 2d | 2.4 |
| 2.11 | 集成测试：HIR 输出对比 | 2d | 2.10 |

#### 验证标准

- [ ] `TypeChecker.aura` 对 `tests/*.aura` 的类型错误报告与 Rust 一致
- [ ] `Hir.aura` 生成的 HIR 与 Rust 一致（结构、类型、属性）
- [ ] 泛型单态化结果一致
- [ ] 内联优化结果一致
- [ ] 常量折叠结果一致

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 类型系统过于复杂 | 分阶段迁移（先基本类型，后泛型/协变） |
| 符号表性能不足 | 使用数组索引替代字符串键 |
| 单态化循环检测 | 使用 `Set<String>` 记录已访问类型 |

---

### 4.5 Phase 3：MIR 生成器与优化器 Aura 化

#### 目标

将 MIR 生成器（HIR → MIR）和优化器（内联、常量折叠、DCE）从 Rust 迁移到 Aura。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/codegen/mir.rs` | 40.2 KB | `aura/lang/compiler/mir/Mir.aura` | 5d | P0 |
| `compiler/src/codegen/opt.rs` | 29.9 KB | `aura/lang/compiler/mir/Optimizer.aura` | 4d | P0 |
| `compiler/src/codegen/opcode.rs` | 42.2 KB | `aura/lang/compiler/mir/Opcode.aura` | 2d | P0 |

#### 迁移策略

1. **MIR 指令**：将 Rust `enum MirInstr` 翻译为 Aura `sealed class MirInstr`
2. **基本块**：使用 `class BasicBlock` 包含 `List<MirInstr>` 和 `Terminator`
3. **MIR 函数**：`class MirFunction` 包含参数槽、基本块列表、寄存器总数
4. **优化器**：递归遍历 MIR，应用内联、常量折叠、DCE 等优化

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| MIR 指令 | `sealed class MirInstr` | 支持 `when` 匹配 |
| 寄存器编号 | `Int`（局部变量槽） | 与 Rust 一致 |
| 优化顺序 | 内联 → 常量折叠 → DCE → 死代码消除 | 与 Rust 一致 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 3.1 | 迁移 `opcode.rs` → `Opcode.aura` | 2d | 2.5 |
| 3.2 | 迁移 `mir.rs` → `Mir.aura` | 5d | 3.1 |
| 3.3 | 迁移 `opt.rs` → `Optimizer.aura` | 4d | 3.2 |
| 3.4 | 编写 Phase 3 验证用例 | 2d | 3.3 |
| 3.5 | 集成测试：MIR 输出对比 | 2d | 3.4 |

#### 验证标准

- [ ] `Mir.aura` 生成的 MIR 与 Rust 一致（基本块、指令、终结符）
- [ ] 优化器结果一致（内联、折叠、DCE）
- [ ] 寄存器分配一致
- [ ] 支持所有 MIR 指令（28 种）

---

### 4.6 Phase 4：VM 字节码发射与解释器 Aura 化

#### 目标

将字节码发射器（MIR → 字节码）和 VM 解释器从 Rust 迁移到 Aura。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/codegen/emit.rs` | 122 KB | `aura/lang/compiler/codegen/Emit.aura` | 8d | P0 |
| `compiler/src/vm/interp.rs` | 61.3 KB | `aura/lang/compiler/vm/Interp.aura` | 6d | P0 |
| `compiler/src/vm/abi.rs` | ~15 KB | `aura/lang/compiler/vm/Abi.aura` | 2d | P1 |
| `compiler/src/vm/value.rs` | ~20 KB | `aura/lang/compiler/vm/Value.aura` | 3d | P0 |
| `compiler/src/vm/heap.rs` | ~25 KB | `aura/lang/compiler/vm/Heap.aura` | 3d | P1 |
| `compiler/src/vm/serialize.rs` | 40.5 KB | `aura/lang/compiler/vm/Serialize.aura` | 4d | P2 |

#### 迁移策略

1. **字节码发射**：将 MIR 寄存器指令线性化为栈式字节码
2. **VM 解释器**：实现指令分发循环（`while` + `when`）
3. **值表示**：使用 Aura `Any` 类型或 `Value` value class
4. **栈帧管理**：使用 `List<Frame>` 模拟调用栈

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 字节码 | `Array<Byte>` | 与 Rust 一致 |
| 值表示 | `Value` value class（tagged union） | 类型安全 |
| 栈帧 | `class Frame`（字节码指针、栈、局部变量） | 与 Rust 一致 |
| 指令分发 | `while` + `when`（解释循环） | 简单高效 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 4.1 | 迁移 `value.rs` → `Value.aura` | 3d | 3.1 |
| 4.2 | 迁移 `emit.rs` → `Emit.aura` | 8d | 3.2 |
| 4.3 | 迁移 `interp.rs` → `Interp.aura` | 6d | 4.1, 4.2 |
| 4.4 | 迁移 `abi.rs` → `Abi.aura` | 2d | 4.3 |
| 4.5 | 迁移 `heap.rs` → `Heap.aura` | 3d | 4.3 |
| 4.6 | 迁移 `serialize.rs` → `Serialize.aura` | 4d | 4.5 |
| 4.7 | 编写 Phase 4 验证用例 | 3d | 4.3 |
| 4.8 | 集成测试：VM 执行结果对比 | 3d | 4.7 |

#### 验证标准

- [ ] `Emit.aura` 生成的字节码与 Rust 一致（指令序列、常量池）
- [ ] `Interp.aura` 对 `tests/*.auc` 的执行结果与 Rust 一致
- [ ] 支持所有字节码指令（50+ 条）
- [ ] 异常处理一致（错误类型、消息）

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| VM 性能不足 | 热点函数 JIT 编译（Cranelift，正式落地于 Phase 7） |
| 栈溢出 | 限制最大栈深度（256） |
| 无限循环 | 指令计数器（`MAX_INSTRUCTIONS = 1000000`） |

---

### 4.7 Phase 5：VM 自举（最小引导层保留）

#### 目标

使用最小编译器（Rust）编译完整 Aura 编译器（Aura 源码），实现自举。

#### 架构

```
Layer 0-A: 最小引导层（Rust，不能上移）
├── 最小编译器（解析器 + 编译器）
│   ├── 词法分析（子集）
│   ├── 语法分析（子集）
│   ├── 语义分析（子集）
│   └── 代码生成（字节码）
├── 最小 VM（字节码解释器）
│   ├── 基础指令集（~50 条）
│   ├── 栈帧管理
│   └── 简单错误处理
└── AOT 编译器入口
    ├── LLVM IR 生成（子集）
    └── 机器代码生成
```

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 5.1 | 编写最小编译器（Rust，~5000 行） | 10d | 4.8 |
| 5.2 | 编写最小 VM（Rust，~2000 行） | 5d | 5.1 |
| 5.3 | 用最小 VM 编译完整 Aura 编译器 | 5d | 5.2 |
| 5.4 | 验证自举成功 | 2d | 5.3 |

#### 验证标准

- [ ] 最小编译器可编译完整 Aura 编译器的源码
- [ ] 最小 VM 可执行完整 Aura 编译器生成的字节码
- [ ] 完整 Aura 编译器可编译标准库

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 最小编译器功能不足 | 逐步扩展指令集和语法支持 |
| 自举失败 | 保留 Rust 编译器作为 fallback |

---

### 4.8 Phase 6：AOT 后端 Aura 自研（自产 IR + 直连 LLVM）

#### 决策变更记录（2026-09-11）

**原方案**：把 Rust AOT 后端（`compiler/src/codegen/aot/*.rs`）**逐文件迁移**到 Aura，
并要求 Aura 产物与 Rust 产物「逐字节一致」（IR 文本对比 / exe MD5 对比）。

**新方案（本文档生效版本）**：

> **Aura 化 AOT 跳过 Rust AOT 后端：Aura 自行产出 LLVM IR 文本，
> 并直接调用 LLVM 工具链（`llc` → `clang`）生成机器码/可执行文件。**

变更理由：

| # | 理由 | 说明 |
|---|------|------|
| 1 | **自举要求「脱 Rust」** | AOT 是执行后端；若 Aura 编译器仍需经 Rust AOT 后端才能出机器码，自举不成立 |
| 2 | **「逐字节一致」是伪目标** | 临时变量命名、指令顺序、常量池布局属实现自由；Rust 后端自身也在演进，锁定文本等价会持续产生无效返工 |
| 3 | **Rust AOT 后端并非「已完成的标准答案」** | 实测仍有大量缺口（std 调用点符号层缺失导致链接期 undefined symbol、类对象「值/指针」两种表示混用、`ret` 未按返回类型转换等），照抄会把缺陷一并搬过来 |
| 4 | **Aura 侧已有更合适的形态** | `aura/compiler/.../aot/` 已按功能拆分（TypeMapper/Target/Runtime/Ffi/Optimize/Emit/Linker/CBackend/Dwarf），应按**功能重实现**而非按文件搬运 |
| 5 | **直连 LLVM 本就不需要 Rust** | 生成机器码只依赖「IR 文本 + `llc`/`clang` 子进程」，Aura 用 `Process.run` 即可直连，中间不需要任何 Rust 代码 |

**新的等价性判据**：由「产物逐字节一致」改为**行为等价**——同一份 Aura 源码分别经
Aura-AOT 与 Rust-AOT 编译后，**可执行文件的运行输出与退出码一致**（差分测试）。

#### 目标

在纯 Aura 中实现 AOT 后端：`HIR → LLVM IR 文本 → 直连 LLVM（llc → clang）→ 机器码/可执行文件`。

**边界（明确不做的事）**：

- ❌ 不调用 Rust AOT 后端（`compiler/src/codegen/aot/`）产出 IR 或目标文件
- ❌ 不要求 IR 文本与 Rust 逐字节一致
- ❌ 不把 Rust AOT 后端作为运行期 fallback（fallback 是「Rust 编译器整体」，见 §7.3）
- ✅ 只要求：Aura 自产 IR 能被 LLVM 接受（`llc -verify` 通过）并生成可运行机器码

#### 实现范围（按功能，而非按 Rust 文件搬运）

下表的 Rust 文件**仅作行为参照**（用于确认类型映射、ABI、命令行形状等约定），
Aura 侧按功能模块独立实现：

| 功能模块 | Aura 目标文件 | Rust 参照 | 预估 |
|----------|---------------|-----------|------|
| 类型映射 | `TypeMapper.aura` | `types.rs` | 2d |
| 目标平台/三元组 | `Target.aura` | `target.rs` | 1d |
| 运行库 ABI 声明与「调用点符号」 | `Runtime.aura` | `runtime.rs` | 2d |
| FFI 声明 | `Ffi.aura` | `ffi.rs` | 1d |
| 优化档 | `Optimize.aura` | `optimize.rs` | 0.5d |
| **IR 发射器（核心）** | `Emit.aura` | `emit.rs` | 14d |
| **LLVM 直连（llc/clang 驱动）** | `Linker.aura` | `linker.rs` | 4d |
| 调试信息 | `Dwarf.aura` | `dwarf.rs` | 1d（P2） |
| C 后端备选 | `CBackend.aura` | `c_backend.rs` | 3d（P2） |

#### 实现策略（重实现 + 直连，而非搬运）

1. **IR 生成**：Aura 内用字符串拼接产出 LLVM IR 文本（预分配 + 常量池去重）
2. **类型映射**：按 §1.4 的表在 Aura 中独立实现；`String` 统一为 `{ i8*, i64 }`，
   `Any`/引用为不透明指针
3. **表示一致性（新增硬约束）**：**类实例统一为指针表示**；`ret` 必须按函数返回类型
   做转换（`%struct.X` ⇄ `i8*`）；禁止出现「声明返回结构体却 `ret` 指针」这类非法 IR
4. **直连 LLVM**：用 `Process.run` 依次调用
   `llc -mtriple <triple> <mod>.ll -o <mod>.obj -filetype=obj <opt>` →
   `clang <mod>.obj <aura_std_cffi.obj> -o <exe> <opt>`
   （链接宿主目标**不加** `-target`，避免 clang 走另一套库/ABI）
5. **运行库**：AOT 产物所需的 `aura_*` 符号由 C 运行库
   `compiler/src/std/cffi/aura_std_cffi.{c,h}` 提供；Aura 侧负责编译它并一并链接进产物
6. **工具发现**：沿用五级探测（§1.5），Aura 侧用 `Env` / `FileSystem` 实现
7. **失败即显性**：IR 不被 LLVM 接受时直接报错并**保留中间 `.ll`** 便于定位，不静默降级

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| IR 生成 | Aura 字符串拼接（自研） | 不依赖 Rust；可控、可调试 |
| LLVM 交互方式 | 文本 IR + `llc`/`clang` 子进程（**直连**） | 与 LLVM 版本解耦；无需绑定 LLVM C API |
| 与 Rust AOT 后端关系 | **零依赖**（仅行为参照） | 保证 Aura 编译器可独立自举 |
| 等价性判据 | **行为等价**（运行输出/退出码） | 替代「逐字节一致」这一伪目标 |
| 表示约束 | 类实例 = 指针；`ret` 严格按返回类型转换 | 消除非法 IR 的主要来源 |
| 运行库 ABI | 调用点符号 `sanitize(aura.lang.std.X.y)` | 与发射器命名同源，避免链接期 undefined symbol |
| 错误处理 | 显式错误 + 保留中间 `.ll` | 便于定位 IR 问题 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 6.1 | 实现 `TypeMapper.aura` | 2d | 2.1 |
| 6.2 | 实现 `Target.aura` | 1d | 6.1 |
| 6.3 | 实现 `Runtime.aura`（运行库声明 + 调用点符号） | 2d | 6.1 |
| 6.4 | 实现 `Ffi.aura` | 1d | 6.1 |
| 6.5 | 实现 `Optimize.aura` | 0.5d | 6.1 |
| 6.6 | **实现 `Emit.aura`（HIR → LLVM IR 文本）** | 14d | 6.1–6.5 |
| 6.7 | **实现 `Linker.aura`（直连 llc/clang + 链接 C 运行库）** | 4d | 6.6 |
| 6.8 | 表示一致性整改：类实例统一指针、`ret` 类型转换 | 3d | 6.6 |
| 6.9 | 实现 `Dwarf.aura`（可选） | 1d | 6.6 |
| 6.10 | 实现 `CBackend.aura`（LLVM 缺失时的备选） | 3d | 6.6 |
| 6.11 | Phase 6 验证用例（IR 可验证 + exe 可运行） | 3d | 6.7 |
| 6.12 | 差分测试：Aura-AOT vs Rust-AOT（运行输出/退出码） | 3d | 6.11 |

#### 验证标准

- [ ] `Emit.aura` 产出的 IR 通过 `llc -verify`（无非法 IR）
- [ ] **`Linker.aura` 可直连 llc/clang 产出可执行文件，且不链接、不调用任何 Rust AOT 产物**
- [ ] **差分测试（行为等价）**：同一源码经 Aura-AOT 与 Rust-AOT 编译，运行输出与退出码一致
- [ ] 支持 `.ll` / `.obj` / `.exe` 输出；`.blob` / `.so` 为 P2
- [ ] 交叉编译支持（`-mtriple`）
- [ ] C 后端 fallback 可用（LLVM 缺失时）
- [ ] 标准库/运行库调用（`String.*`、`Collections.*`、`println` 等）链接成功并可运行

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| Aura 侧 IR 发射与 LLVM 契约不符 | 每类指令落地即用 `llc -verify` 回归；保留中间 `.ll` |
| 表示混用（指针/结构体）产生非法 IR | 6.8 单独立项整改；把「类实例 = 指针」写成硬约束 |
| 运行库符号缺失（undefined symbol） | 调用点符号层与发射器命名同源；链接阶段全量校验 |
| 与 Rust 实现行为偏差 | 差分测试（6.12）作门禁；按「行为等价」判定，不做文本比对 |
| 工具发现失败 | 配置 `AURA_LLVM_HOME` 或根 `Cargo.toml` 的 `llvm-home` |

> **后续阶段**：本节只交付「IR 发射 + 直连 LLVM」的能力骨架；
> AOT 产物在真实程序上的**表示一致性、运行库契约、sema 类型信息**等系统性问题，
> 由下一节 **[Phase 6.5：AOT 后端适配与修复](#49-phase-65aot-后端适配与修复aot-hardening)** 专门收敛。

---

### 4.9 Phase 6.5：AOT 后端适配与修复（AOT Hardening）

> **本阶段为 2026-09-11 新增**：Phase 6 只交付「能把 HIR 发射成 LLVM IR」的能力；
> 实测在真实程序（尤其 **Aura 编译器自身**）上暴露出一批系统性的语义/表示缺陷，
> 导致产物要么被 `llc` 拒绝、要么链接期 `undefined symbol`、要么运行期语义错误。
> 本阶段专门用于**适配改造 AOT 后端**，把它从「能发射 IR」推进到「能产出可正确运行的机器码」。

#### 目标

收敛 AOT 后端的**表示一致性与运行库契约**，使 AOT 成为可靠执行后端，
并为「Aura 编译器自身 AOT 出可执行文件」扫清障碍。

#### 为什么必须单独立项

这些问题**不是单点 bug，而是跨层契约缺陷**，横跨 IR 发射器、类型系统（sema）、
C 运行库与链接驱动，且在 Phase 6 的「简单程序」验证下不可见：

| # | 缺陷 | 现象 | 归属层 |
|---|------|------|--------|
| 1 | 类实例「值 / 指针」两种表示混用 | 同一对象时而 `i8*` 时而 `%struct.X` | IR 发射器 |
| 2 | `ret` 未按函数返回类型转换 | `define %struct.Token` 却 `ret i8*` → `llc` 报 `value doesn't match function result type` | IR 发射器 |
| 3 | 运行库「调用点符号」缺失 | 发射 `aura_lang_std_String_split`，C 库只导出 `aura_string_*` → 链接期 undefined symbol | 运行库 ABI |
| 4 | 运行库覆盖面不足 | `List` 取值/计数/追加、`String.split/indexOf/countChar` 等缺失 | 运行库实现 |
| 5 | 字符串双表示 | Aura 内 `{ i8*, i64 }` vs C ABI `char*`，跨界未统一转换 | 类型映射 / coerce |
| 6 | 整型 i32/i64 混用 | `add i32 …, i64 …` → 非法 IR | IR 发射器 |
| 7 | sema 缺少 std 签名表 | std 调用类型退化为 `Any` → 发射器选错表示 | 语义分析 |
| 8 | 缺 IR 合法性门禁 | 直到 `llc` 才报错，且错误定位困难 | 工具链 / CI |
| 9 | 失败不可诊断 | 中间 `.ll` 未保留，无法定位首个非法指令 | 工具链 |

#### 适配改造清单

| 项 | 现状 | 改造目标 |
|----|------|----------|
| 类实例表示 | 指针 / 结构体两种并存 | **统一为「类实例 = 指针」**，成员访问、方法调用、返回、传参全链路一致 |
| 返回类型 | `ret` 直接返回表达式值 | `ret` 前按 `current_ret_ty` 插入转换（`%struct.X` ⇄ `i8*`：`load` / `alloca`+取址） |
| 运行库符号 | 发射器命名与 C 库命名脱节 | 建立**调用点符号层**（`sanitize(aura.lang.std.X.y)` 为唯一契约），发射器与运行库同源 |
| 运行库范围 | 仅覆盖部分 String/Math | 按「编译器实际依赖」补齐：String 全量、List（get/size/append/count）、Map 最小可用 |
| 字符串边界 | 结构体与 `char*` 混用 | 明确边界：**Aura 内部 = `{ i8*, i64 }`，跨 C ABI = `char*`**，仅在调用点做 coerce |
| 整型提升 | 无统一规则 | 算术 / 比较 / 下标统一提升规则，禁止跨宽度直接运算 |
| sema 类型信息 | std 调用退化为 `Any` | 补 **std 签名表**（函数名 → 参数/返回类型）；沿用「只告警不阻断」策略 |
| IR 门禁 | 无 | 每个用例跑 `llc -verify`，纳入 CI；失败保留中间 `.ll` 并打印首个非法指令位置 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 6.5.1 | 统一类实例表示（指针）并完成全链路改造 | 5d | 6.6 |
| 6.5.2 | `ret` 按返回类型转换 + 单测 | 2d | 6.5.1 |
| 6.5.3 | 运行库调用点符号层（C 实现 + 头文件） | 3d | 6.3 |
| 6.5.4 | 运行库补齐（String 全量 + List/Map 最小可用） | 4d | 6.5.3 |
| 6.5.5 | 字符串双表示的 coerce 规则统一 | 2d | 6.5.1 |
| 6.5.6 | 整型统一提升（算术 / 比较 / 下标） | 2d | 6.5.1 |
| 6.5.7 | sema std 签名表 | 4d | 6.4 |
| 6.5.8 | `llc -verify` 门禁 + 失败保留 `.ll` | 2d | 6.7 |
| 6.5.9 | 端到端用例：String / List / 类 / 控制流 / 异常 的 AOT 编译与运行 | 3d | 6.5.1–6.5.8 |
| 6.5.10 | 阶段性目标：Aura 编译器自身 AOT 出可运行 exe（自举前置） | 5d | 6.5.9 |

#### 验证标准

- [ ] AOT 用例集（String / List / 类 / 控制流 / 异常）全部「编译 → 链接 → 运行输出正确」
- [ ] `llc -verify` 全绿（无非法 IR），并作为 CI 门禁
- [ ] 同一源码经 Aura-AOT 与 Rust-AOT 编译，**运行输出与退出码一致**（行为差分）
- [ ] `tests/aot/*` 夹具退出码符合预期
- [ ] **阶段性目标**：Aura 编译器自身可 AOT 出可运行可执行文件
- [ ] 失败时可从保留的 `.ll` 定位到首个非法指令，无需重跑

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 表示整改波及面大（改一处崩一片） | 先建 **IR 差分基线**（改造前后跑同一批用例集），逐项小步提交 |
| sema 签名表引入新类型错误 | 签名表遵循「只告警不阻断」（沿用 P3 策略），先给类型、后收紧 |
| 运行库无限膨胀 | 以「调用点符号扫描」驱动补齐——只实现发射器真实引用的符号 |
| 「自举 exe」目标过重 | 拆为「先能链接 → 再运行正确 → 最后自举」三步，允许阶段性交付 |
| 与 Rust 后端行为偏差 | 以行为差分测试作门禁；偏差按语义修正，不追求文本一致 |

#### 交付物

| 交付物 | 说明 |
|--------|------|
| AOT 表示规范 | 「类实例 = 指针」「字符串边界 = 结构体/`char*`」写入 §4.8 硬约束并落地 |
| 运行库契约表 | 调用点符号 ↔ C 实现 ↔ ABI 签名 三方一致的清单 |
| std 签名表 | sema 可见的 std 函数签名（参数 / 返回类型） |
| AOT 用例集 | `tests/aot/*`：String / List / 类 / 控制流 / 异常 + 行为差分 |
| CI 门禁 | `llc -verify` + 用例集退出码校验 |

---

### 4.10 Phase 7：JIT 编译（Cranelift 直连机器码）Aura 化

#### 目标

补全「**VM 解释器 / JIT 即时编译 / AOT 静态编译**」三执行路径中的 **JIT** 路径：把
Rust 侧已有的 Cranelift JIT（`compiler/src/vm/jit.rs` 及其配套）迁移为纯 Aura 实现，
使 Aura 编译器自身即可在**进程内**把热点函数编译为原生机器码，无需外部 LLVM 工具链、
无子进程与启动开销，适用于 REPL、热重载、长驻服务热路径与自举引导层。

> **前情提要（务必继承）**：`docs/JIT性能分析.md` 记录了旧 JIT「与解释器同速（≈1.0x）」的
> 根因——四点机制互相抵消，热点永远无法触发编译：
> ① 编译期内联删除了小函数调用；② 入口函数（`main`）不参与热点计数；
> ③ 递归热点被 `is_jit_compilable()` 白名单拒绝；④ 仅靠调用频率的检测对「循环热点」天然失效。
> Rust 侧已通过 **Fix A（入口函数强制编译）** 与 **Fix B（递归函数支持：dispatch table +
> `call_indirect`）** 修复，并叠加 7 个高级字节码优化传递，取得 sum ~109x / fib ~78x。
> **Aura 化必须等价继承 Fix A/B 与优化传递**，否则会重蹈覆辙。

#### 与 VM / AOT 的分工

| 维度 | VM 解释器（Phase 4/5） | **JIT（Phase 7）** | AOT（Phase 6） |
|------|----------------------|--------------------|----------------|
| 中间输入 | 字节码（`.auc`）/ MIR | 字节码 / MIR | HIR |
| 代码生成 | 无（解释执行） | Cranelift → 原生机器码（进程内） | LLVM IR 文本 → `llc`/`clang` |
| 外部依赖 | 无 | Cranelift（纯 Rust，可内联/FFI） | LLVM 工具链（~500MB） |
| 启动开销 | 低 | 低（首次编译热点有延迟） | 高（子进程 + 链接） |
| 执行性能 | 低 | 高（接近原生） | 高（O2/O3，最优） |
| 典型场景 | 脚本 / 冷启动 / 调试 | REPL / 热重载 / 长驻热路径 | 产物分发 / 交叉编译 |

三者共享**同一份 MIR / 字节码与 Runtime ABI（`JitValue`）**，任何一路都不得改变语义。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/vm/jit.rs`（热点/白名单/编译/派发） | ~807 | `aura/lang/compiler/jit/JitState.aura` | 4d | P0 |
| `compiler/src/vm/jit.rs`（IR 发射核心） | — | `aura/lang/compiler/jit/JitLower.aura` | 6d | P0 |
| `compiler/src/vm/jit_opt.rs`（7 优化传递） | — | `aura/lang/compiler/jit/JitOpt.aura` | 4d | P0 |
| `compiler/src/vm/abi.rs`（`JitValue` / `AotEntry`） | ~210 | `aura/lang/compiler/jit/JitAbi.aura` | 3d | P0 |
| `compiler/src/vm/mod.rs`（热点计数 / 原生派发 / 回退） | — | `aura/lang/compiler/jit/JitDispatch.aura` | 3d | P0 |
| `compiler/src/vm/aot_runtime.rs`（W^X 加载 / 描述符表 / Blob） | ~1130 | `aura/lang/compiler/jit/JitRuntime.aura` | 3d | P1 |
| `compiler/src/bootstrap/jit_core.rs`（最小 JIT 引导） | — | `aura/lang/compiler/jit/JitCore.aura` | 2d | P1 |

#### 迁移策略

1. **热点与白名单**：以调用计数（`call_counts`）+ 入口强制编译（Fix A）+ 递归可达分析
   （Fix B，放宽 `is_jit_compilable` 白名单）决定编译时机；被调用者先编译，保证
   dispatch table 条目存在。
2. **IR 发射**：把字节码/MIR 逐条映射为 Cranelift IR（`iconst`/`iadd`/`brif`/`call` /
   `call_indirect` / `load`/`store` stack slot …），沿用现有指令映射表。
3. **优化传递**：等价实现 7 个传递——常量折叠、死码消除、跳转线程化、强度削弱
   （`/2^n`→移位、`*2^n`→左移）、指令调度、小函数内联、简单循环展开；优化结果需与
   字节码语义逐例对齐（差分测试）。
4. **ABI / 派发**：`JitValue`（tag + payload）作为统一调用约定；`dispatch_table` 走
   `call_indirect`，实现同速递归与跨函数调用。
5. **运行时段与回退**：复用 AOT Blob 基础设施（W^X mmap/VirtualAlloc、`AuraFuncDesc`
   描述符表、`.auc v4` 段格式）；任何不可编译函数**回退解释器**，保证结果一致。
6. **单一路径真相源**：VM / JIT / AOT 共用 MIR，通过「后端选择器」路由，禁止各自
   维护不同的语义实现。

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| JIT 输入 | 字节码 / MIR（非 HIR） | 已是寄存器式 CFG，与 Cranelift IR 语义接近，复用 Phase 3/4 |
| 代码生成 | Cranelift（纯 Rust，经 FFI 内联） | 与 AOT 的 LLVM 解耦，无外部工具链，进程内直接执行 |
| 热点判定 | 调用计数 + 入口强制 + 递归可达 | 直接继承 Fix A/B，避免「JIT 等于解释器」复发 |
| 调用约定 | `JitValue` ABI + `dispatch_table` | 与 AOT Blob 兼容，可复用到 `.auc v4` |
| 优化 | 7 个字节码级传递（与 Rust 等价） | 在 Cranelift 之前削减指令、简化控制流 |
| 失败处理 | 回退解释器 | JIT 不可编译时仍保证正确性 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 7.1 | 迁移 `jit.rs` 状态机 → `JitState.aura`（计数/阈值/白名单/递归可达） | 4d | 4.6 |
| 7.2 | 迁移 `abi.rs` → `JitAbi.aura`（`JitValue` / `AotEntry` / dispatch table） | 3d | 7.1 |
| 7.3 | 迁移 IR 发射 → `JitLower.aura`（算术/控制流/调用/间接调用） | 6d | 7.2 |
| 7.4 | 迁移 `jit_opt.rs` → `JitOpt.aura`（7 个优化传递） | 4d | 7.3 |
| 7.5 | 原生派发 + 解释器回退 → `JitDispatch.aura` | 3d | 7.3 |
| 7.6 | 迁移 `aot_runtime.rs` 段加载 → `JitRuntime.aura`（W^X / 描述符 / Blob） | 3d | 7.2 |
| 7.7 | 最小引导层 `JitCore.aura` | 2d | 7.3 |
| 7.8 | 编写 Phase 7 验证用例 + 三路（VM/JIT/AOT）差分对比 | 3d | 7.4–7.7 |

#### 验证标准

- [ ] **热点可达**：入口函数强制编译（Fix A）与递归函数编译（Fix B）均生效，
  `jit_state()` 不再出现「❄️未达阈值」/「⛔已跳过」的静默回退
- [ ] **优化等价**：7 个优化传递的产物与 Rust 基线逐例一致（常量折叠/DCE/跳转线程化/
  强度削弱/指令调度/内联/循环展开）
- [ ] **执行一致**：同一程序 VM / JIT / AOT 三路结果逐字节一致（含异常与整数溢出语义）
- [ ] **回退正确**：不可编译函数回退解释器，结果与纯 JIT 一致
- [ ] **递归/互递归**：fib 等递归热点经 `dispatch_table` + `call_indirect` 正常执行
- [ ] **ABI 兼容**：`JitValue` 可被 AOT Blob 加载路径复用（`.auc v4` 读取）
- [ ] **性能**：`sum`/`fib` 相对解释器 ≥ 50x（Rust 基线 109x / 78x 的下限，留回归余量）

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| 热点检测再次失效（历史问题） | 直接继承 Fix A/B；以 `jit_state()` 诊断输出作为门禁断言 |
| Cranelift IR 与 VM/AOT 语义漂移 | 以 MIR 为单一真相源 + 三路差分测试（VM/JIT/AOT） |
| 机器码 ABI / 调用约定错误 | `JitValue` ABI 兼容性测试 + C ABI 对照，复用 Phase 6 的 ABI 断言 |
| Aura 侧 Cranelift FFI 不可用 | 保留 Rust JIT 作为 fallback；本 Phase 可整体回退（见 §7.3） |
| 维护 VM/JIT/AOT 三套后端成本 | 后端选择器统一路由，共享 MIR/字节码与 Runtime，禁止语义分叉 |

---

### 4.11 Phase 8：核心库与标准库 Aura 化

#### 目标

将标准库与核心库从 Rust 实现上移到 Aura 源码，使 **`aura/core/aura/lang/`** 成为唯一真相源
（Rust 侧 `compiler/src/std/*.rs` 退化为引导/兼容层）。

目标目录以**当前仓库实际布局**为准，分为四类：

```
aura/core/aura/lang/
├── *.aura                       # 核心类型（Any/Boolean/Byte/Char/Double/Float/Function/
│                                #   Int/Long/Nothing/Short/String/Type/Unit + prelu）
├── collection/                  # 集合库（Array/ArrayList/Collection/Collections/
│                                #   HashMap/HashSet/List/Map/Set）
├── coroutine/                   # 并发（Actor/Coroutine）
└── std/                         # 标准模块（Ascii/Assert/Builtin/Channel/Console/Encoding/
                                 #   Env/FileSystem/IO/Iter/Json/Math/Network/Path/Process/
                                 #   Random/Test/TestHelper/Time）
```

> **现状说明**：上述 Aura 文件（核心类型、`collection/`、`coroutine/`、`std/`）**已经存在**
> （见 `build/*.auc` 编译产物）。Phase 8 的工作是**完成 Rust → Aura 的等价迁移、API 对齐与
> 一致性验证**，而不是从零创建目录。

#### 目录对应关系

| 域 | Rust 源 | Aura 目标目录（当前布局） |
|----|---------|--------------------------|
| 核心类型 | `compiler/src/std/std_builtin.rs`、`compiler/src/vm/value.rs` | `aura/core/aura/lang/*.aura`（顶层） |
| 集合 | `compiler/src/std/std_collections.rs` | `aura/core/aura/lang/collection/*.aura` |
| 并发 | `compiler/src/vm/native.rs`（`register_concurrent`） | `aura/core/aura/lang/coroutine/*.aura` + `std/Channel.aura` |
| 标准模块 | `compiler/src/std/std_*.rs` | `aura/core/aura/lang/std/*.aura` |

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件（当前布局） | 预估 | 优先级 |
|-----------|------|--------------------------|------|--------|
| `compiler/src/std/std_builtin.rs` + `vm/value.rs` | ~20 KB | `aura/core/aura/lang/{Any,Boolean,Byte,Char,Double,Float,Function,Int,Long,Nothing,Short,String,Type,Unit}.aura` | 3d | P0 |
| `compiler/src/std/std_math.rs` | ~30 KB | `aura/core/aura/lang/std/Math.aura` | 2d | P0 |
| `compiler/src/std/std_string.rs` | ~50 KB | `aura/core/aura/lang/String.aura`（核心类型） | 5d | P0 |
| `compiler/src/std/std_ascii.rs` | ~10 KB | `aura/core/aura/lang/std/Ascii.aura` | 1d | P2 |
| `compiler/src/std/std_path.rs` | ~20 KB | `aura/core/aura/lang/std/Path.aura` | 2d | P0 |
| `compiler/src/std/std_encoding.rs` | ~30 KB | `aura/core/aura/lang/std/Encoding.aura` | 3d | P0 |
| `compiler/src/std/std_time.rs` | ~20 KB | `aura/core/aura/lang/std/Time.aura` | 2d | P1 |
| `compiler/src/std/std_collections.rs` | ~40 KB | `aura/core/aura/lang/collection/{Collection,Collections,List,Array,ArrayList,Map,HashMap,Set,HashSet}.aura` | 4d | P0 |
| `compiler/src/std/std_iter.rs` | ~20 KB | `aura/core/aura/lang/std/Iter.aura` | 3d | P1 |
| `compiler/src/std/std_io.rs` | ~30 KB | `aura/core/aura/lang/std/IO.aura` | 3d | P1 |
| `compiler/src/std/std_fs.rs` | ~30 KB | `aura/core/aura/lang/std/FileSystem.aura` | 3d | P1 |
| `compiler/src/std/std_net.rs` | ~40 KB | `aura/core/aura/lang/std/Network.aura` | 5d | P2 |
| `compiler/src/std/std_json.rs` | ~30 KB | `aura/core/aura/lang/std/Json.aura` | 4d | P2 |
| `compiler/src/std/std_assert.rs` | ~10 KB | `aura/core/aura/lang/std/Assert.aura` | 1d | P0 |
| `compiler/src/std/std_test.rs` | ~15 KB | `aura/core/aura/lang/std/Test.aura` + `aura/core/aura/lang/std/TestHelper.aura` | 2d | P1 |
| `compiler/src/std/std_env.rs` | ~10 KB | `aura/core/aura/lang/std/Env.aura` | 1d | P1 |
| `compiler/src/std/std_process.rs` | ~15 KB | `aura/core/aura/lang/std/Process.aura` | 2d | P1 |
| `compiler/src/std/std_random.rs` | ~10 KB | `aura/core/aura/lang/std/Random.aura` | 1d | P2 |
| `compiler/src/std/std_console.rs` | ~10 KB | `aura/core/aura/lang/std/Console.aura` | 1d | P1 |
| `compiler/src/vm/native.rs`（`register_concurrent`） | — | `aura/core/aura/lang/coroutine/{Coroutine,Actor}.aura` + `std/Channel.aura` | 3d | P1 |

#### 迁移策略

1. **核心类型**（`aura/core/aura/lang/*.aura`）：内建方法表（`std_builtin.rs`）与值模型
   （`vm/value.rs`）上移为顶层 `.aura`，保持运算符/字面量语义与 VM 一致。
2. **纯逻辑模块**（`std/Math`、`String`、`std/Path`、`std/Encoding`、`std/Time`）：直接翻译为 Aura 函数。
3. **FFI 模块**（`std/IO`、`std/FileSystem`、`std/Network`、`std/Process`）：`extern "C"` 声明 + Aura 包装。
4. **集合模块**（`collection/`、`std/Iter`）：以 `List`/`Map`/`Set` 为基础，拆分为
   `Collection/Collections/Array/ArrayList/Map/HashMap/Set/HashSet` 等独立文件。
5. **并发模块**（`coroutine/` + `std/Channel`）：`Coroutine`/`Actor`/`Channel` 与
   `register_concurrent` 对齐（`Coroutine.spawnActor` / `Actor.spawnActor` 等符号名一致）。
6. **测试模块**（`std/Assert`、`std/Test`、`std/TestHelper`）：使用 Aura 的异常与断言。

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 核心类型落点 | `aura/core/aura/lang/*.aura`（顶层） | 与语言内建类型一一对应，供 std/collection 复用 |
| 集合落点 | `aura/core/aura/lang/collection/` | 多实现在同一子目录，避免污染顶层类型 |
| 并发落点 | `aura/core/aura/lang/coroutine/` + `std/Channel.aura` | 与现有 `coroutine/` 目录一致 |
| 纯逻辑 | Aura 函数 | 无 FFI 依赖 |
| FFI 模块 | `extern "C"` + Aura 包装 | 类型安全 |
| 集合 | Aura 原生类型 | 与语言集成 |
| 错误处理 | `Result<T, E>` 或异常 | 与 Rust 一致 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 8.1 | 核心基础类型校对（`aura/core/aura/lang/*.aura`） | 3d | 6.12 |
| 8.2 | 迁移 `std_math.rs` → `std/Math.aura` | 2d | 8.1 |
| 8.3 | 迁移 `std_string.rs` → `String.aura` | 5d | 8.1 |
| 8.4 | 迁移 `std_ascii.rs` → `std/Ascii.aura` | 1d | 8.3 |
| 8.5 | 迁移 `std_path.rs` → `std/Path.aura` | 2d | 8.2 |
| 8.6 | 迁移 `std_encoding.rs` → `std/Encoding.aura` | 3d | 8.3 |
| 8.7 | 迁移 `std_time.rs` → `std/Time.aura` | 2d | 8.2 |
| 8.8 | 迁移 `std_collections.rs` → `collection/*.aura` | 4d | 8.1 |
| 8.9 | 迁移 `std_iter.rs` → `std/Iter.aura` | 3d | 8.8 |
| 8.10 | 迁移 `std_io.rs` → `std/IO.aura` | 3d | 8.2 |
| 8.11 | 迁移 `std_fs.rs` → `std/FileSystem.aura` | 3d | 8.10 |
| 8.12 | 迁移 `std_net.rs` → `std/Network.aura` | 5d | 8.10 |
| 8.13 | 迁移 `std_json.rs` → `std/Json.aura` | 4d | 8.3 |
| 8.14 | 迁移 `std_assert.rs` → `std/Assert.aura` | 1d | 8.1 |
| 8.15 | 迁移 `std_test.rs` → `std/Test.aura` + `std/TestHelper.aura` | 2d | 8.14 |
| 8.16 | 迁移 `std_env.rs` → `std/Env.aura` | 1d | 8.10 |
| 8.17 | 迁移 `std_process.rs` → `std/Process.aura` | 2d | 8.10 |
| 8.18 | 迁移 `std_random.rs` → `std/Random.aura` | 1d | 8.2 |
| 8.19 | 迁移 `std_console.rs` → `std/Console.aura` | 1d | 8.10 |
| 8.20 | 迁移 `vm/native.rs` 并发注册 → `coroutine/*` + `std/Channel.aura` | 3d | 8.1 |
| 8.21 | 编写 Phase 8 验证用例 | 3d | 8.1–8.20 |
| 8.22 | 集成测试：标准库/核心库 API 对比 | 3d | 8.21 |

#### 验证标准

- [ ] **核心类型**（`aura/core/aura/lang/*.aura`）方法/运算符与 VM 内建行为逐例一致
- [ ] **集合**（`aura/core/aura/lang/collection/`）各实现 API 与 Rust `std_collections.rs` 一致
- [ ] **并发**（`coroutine/` + `std/Channel.aura`）符号名与 `register_concurrent` 对齐
- [ ] **标准模块**（`aura/core/aura/lang/std/`）所有函数在 Aura 中可用
- [ ] API 签名与 Rust 一致
- [ ] 测试用例全部通过
- [ ] FFI 模块正确调用 C 函数

---

### 4.12 Phase 9：LLVM C API 直连（可选优化档）

#### 目标

AOT 已默认**直连 LLVM**（Phase 6：Aura 自产文本 IR + `llc`/`clang` 子进程）。
当文本 IR 路径性能不足时，升级到方案二：Aura 经 C FFI 直接调用 LLVM C API，
在进程内生成机器码（省去 IR 文本落盘与子进程开销）。

#### 新增文件

| 文件 | 行数 | 预估 | 优先级 |
|------|------|------|--------|
| `llvm_bindings.h` | ~200 行 C | 2d | P0 |
| `llvm_bindings.c` | ~100 行 C | 1d | P0 |
| `llvm_bindings.aura` | ~500 行 Aura | 5d | P0 |
| `llvm_codegen.aura` | ~2500 行 Aura | 10d | P0 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 9.1 | 编写 `llvm_bindings.h` | 2d | 8.22 |
| 9.2 | 编写 `llvm_bindings.c` | 1d | 9.1 |
| 9.3 | 编写 `llvm_bindings.aura` | 5d | 9.2 |
| 9.4 | 编写 `llvm_codegen.aura` | 10d | 9.3 |
| 9.5 | 集成测试：LLVM C API 输出对比 | 3d | 9.4 |

#### 验证标准

- [ ] `llvm_codegen.aura` 生成的 LLVM IR 与方案一一致
- [ ] 编译速度提升（目标：>50%）
- [ ] 错误诊断更详细

---

## 五、关键技术挑战与解决方案

### 5.1 挑战 1：字符串拼接性能

**问题**：当前 Rust 实现用 `format!()` 宏，编译期零成本抽象。Aura 的字符串拼接如果实现不当，会产生大量临时对象。

**解决方案**：
1. **预分配 StringBuilder**：`StringBuilder` 类，避免反复分配
2. **类型安全的字符串 API**：`String.format(fmt, args...)` 替代手动拼接
3. **常量池**：对重复的字符串（如类型名、指令名）使用常量池去重

```aura
// 示例：StringBuilder 模式
let sb = StringBuilder.new()
sb.append("define ")
sb.append(ret_ty)
sb.append(" @")
sb.append(func.name)
sb.append("(")
for (i, param) in params.enumerate() {
    if (i > 0) { sb.append(", ") }
    sb.append(param.llvm_ty)
    sb.append(" %arg.")
    sb.append(sanitize(param.name))
}
sb.append(") {\n")
```

### 5.2 挑战 2：HashMap / HashSet 的性能

**问题**：当前 Rust 实现大量使用 `HashMap` / `HashSet`，Aura 的 `Map` / `Set` 实现必须高效。

**解决方案**：
1. **基于数组的 Map**：对键空间较小的情况使用数组
2. **字符串哈希**：使用 FNV-1a 或 MurmurHash 实现字符串哈希
3. **开放寻址**：避免链式哈希的内存开销

### 5.3 挑战 3：泛型单态化

**问题**：当前 Rust 的 `mono_hir` 实现是递归的类型替换。Aura 的泛型系统必须支持单态化。

**解决方案**：
1. **编译期单态化**：在 HIR 阶段替换类型参数
2. **字典分发**：对协变类型参数使用虚表

### 5.4 挑战 4：自举问题

**问题**：Aura 编译器需要编译器来编译，形成鸡生蛋问题。

**解决方案**：
1. **Layer 0-A：最小编译器**（Rust，约 5000 行）
   - 只支持 Aura 的子集（无泛型、无闭包、无类）
   - 编译出完整 Aura 编译器
2. **Layer 0-B：完整编译器**（Aura 编写，自举）
   - 完整指令集、完整优化器
   - 用最小编译器编译，用完整编译器自举

### 5.5 挑战 5：LLVM 工具链发现

**问题**：纯 Aura 编译器需要找到 `llc` / `clang` 工具。

**解决方案**：
1. **环境变量**：`AURA_LLVM_HOME`（已支持）
2. **配置文件**：`aura.toml` 中的 `[llvm] home = "..."`
3. **PATH 搜索**：通过 `Process.run("where llc")` 或 `Process.run("which llc")`
4. **交叉编译**：通过 `aura.toml` 中的 `[targets.<name>]` 配置

---

## 六、工作量估算与里程碑

### 6.1 工作量汇总

| Phase | 工作项 | 预估天数 | 预估人月 |
|-------|--------|----------|----------|
| Phase 0 | 基础设施 | 7d | 0.3 |
| Phase 1 | 前端 Aura 化 | 20d | 1.0 |
| Phase 2 | 语义分析 + HIR | 33d | 1.7 |
| Phase 3 | MIR + 优化器 | 15d | 0.8 |
| Phase 4 | VM 字节码 + 解释器 | 33d | 1.7 |
| Phase 5 | VM 自举 | 22d | 1.1 |
| Phase 6 | AOT 后端（自研 IR + 直连 LLVM） | 38d | 1.9 |
| **Phase 6.5** | **AOT 后端适配与修复** | **32d** | **1.6** |
| Phase 7 | JIT（Cranelift） | 25d | 1.3 |
| Phase 8 | 标准库 + 核心库 | 57d | 2.7 |
| Phase 9 | LLVM C API（可选） | 21d | 1.1 |
| **总计** | | **303d** | **15.2** |

### 6.2 里程碑

```
M0（第 0.3 月末）：Phase 0 完成，基础设施就绪
M1（第 1.3 月末）：Phase 1 完成，Lexer/Parser 可独立运行
M2（第 3.0 月末）：Phase 2 完成，Sema/HIR 可独立运行
M3（第 3.8 月末）：Phase 3 完成，MIR/Optimizer 可独立运行
M4（第 5.5 月末）：Phase 4 完成，VM 字节码发射可用
M5（第 6.6 月末）：Phase 5 完成，VM 自举成功
M6（第 8.5 月末）：Phase 6 完成，Aura 侧 AOT 可自产 IR 并直连 LLVM 产出可执行文件
M6.5（第 10.1 月末）：Phase 6.5 完成，AOT 产物可正确运行（含「Aura 编译器自身 AOT 出 exe」）
M7（第 11.4 月末）：Phase 7 完成，JIT（Cranelift）可用，VM/JIT/AOT 三路互通
M8（第 13.5 月末）：Phase 8 完成，核心库/标准库 Aura 化完成
M9（第 14.5 月末）：Phase 9 完成，LLVM C API 直连（可选）
```

### 6.3 交付物清单

| 里程碑 | 交付物 | 验证方式 |
|--------|--------|----------|
| M1 | `Lexer.aura` + `Parser.aura` | 快照测试 |
| M2 | `TypeChecker.aura` + `Hir.aura` | 类型检查测试 |
| M3 | `Mir.aura` + `Optimizer.aura` | MIR 输出对比 |
| M4 | `Emit.aura` + `Interp.aura` | VM 执行测试 |
| M5 | 最小编译器 + 自举成功 | 编译标准库 |
| M6 | `Emit.aura`（AOT）+ `Linker.aura`（直连 LLVM） | IR 通过 `llc -verify` + 与 Rust-AOT 行为差分 |
| M6.5 | AOT 适配整改（表示规范 / 运行库契约表 / std 签名表） | AOT 用例集全绿 + `llc -verify` 门禁 + 行为差分 |
| M7 | `JitState.aura` + `JitLower.aura` + `JitOpt.aura` | 三路（VM/JIT/AOT）差分 + 性能对比 |
| M8 | 核心库/标准库 `.aura` 文件（`aura/core/aura/lang/`） | API 对比测试 |
| M9 | `llvm_bindings.aura` + `llvm_codegen.aura` | 性能对比 |

---

## 七、风险矩阵与回退策略

### 7.1 风险矩阵

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Aura 字符串性能不足 | 中 | 高 | StringBuilder + 预分配 + 常量池 |
| Aura Map/Set 性能不足 | 中 | 高 | 基于数组的实现 + 哈希优化 |
| 自举失败 | 中 | 致命 | 保留 Rust 编译器作为 fallback |
| LLVM C API 不稳定 | 低 | 中 | 优先方案一（文本 IR） |
| 交叉编译工具链缺失 | 高 | 中 | C 后端备选 + 预装工具链 |
| 泛型单态化不完整 | 中 | 中 | 分阶段支持（标量 → 引用 → 函数） |
| ARC 语义在 LLVM IR 中表达不清 | 中 | 中 | 显式 `Retain`/`Release` 指令 |
| 闭包捕获在 LLVM IR 中表达不清 | 低 | 高 | 闭包结构体 + 函数指针（已验证） |
| **Aura 侧 IR 发射不符合 LLVM 契约** | 中 | 中 | **Phase 6.5.8**：每类指令落地即跑 `llc -verify` 回归；报错时保留中间 `.ll` |
| **Aura-AOT 与 Rust-AOT 行为偏差** | 中 | 中 | **Phase 6.5**：差分测试（运行输出/退出码）作门禁；按「行为等价」判定，不做文本比对 |
| **类实例表示混用 / `ret` 类型不符导致非法 IR** | 高 | 高 | **Phase 6.5.1–6.5.2**：统一「类实例 = 指针」；`ret` 按返回类型转换 |
| **运行库符号缺失（undefined symbol）** | 中 | 高 | **Phase 6.5.3–6.5.4**：调用点符号层与发射器命名同源；按依赖扫描补齐；链接阶段全量校验 |
| **sema 缺 std 签名表致类型退化 `Any`** | 中 | 高 | **Phase 6.5.7**：补 std 签名表（只告警不阻断） |
| 最小编译器功能不足 | 中 | 高 | 逐步扩展指令集和语法支持 |
| **JIT 热点不可达（历史问题复发）** | **中** | **高** | **继承 Fix A（入口强制编译）+ Fix B（递归 `dispatch_table`），以 `jit_state()` 诊断作门禁** |
| JIT 与 VM/AOT 语义漂移 | 中 | 高 | 以 MIR 为单一真相源 + 三路（VM/JIT/AOT）差分测试 |
| Aura 侧 Cranelift FFI 不可用 | 中 | 中 | 保留 Rust JIT 作为 fallback；Phase 7 整体可回退 |
| Phase 间依赖冲突 | 低 | 中 | 每个 Phase 独立可回退 |
| **迁移过程破坏现有编译** | **极低** | **致命** | **Rust 编译器完全保留，Aura 编译器独立目录，互不干扰** |

### 7.2 核心风险缓解：并行开发模型

> **关键缓解策略**：通过"独立代码层 + 保留 Rust 实现"的设计，**彻底消除"迁移过程破坏现有编译"的风险**。

```
传统迁移（高风险）：
┌──────────────────────────────────────────────────────────────┐
│  修改 compiler/src/lexer.rs → 可能编译失败                     │
│  修改 compiler/src/parser.rs → 可能破坏语法分析                 │
│  修改 compiler/src/codegen/ → 可能破坏代码生成                  │
│                                                              │
│  风险：任何修改都可能导致无法编译，回退困难                       │
└──────────────────────────────────────────────────────────────┘

并行开发（低风险）：
┌──────────────────────────────────────────────────────────────┐
│  保留 compiler/（不动）                                       │
│  新增 aura/compiler/（独立）                                   │
│                                                              │
│  ✓ Rust 编译器始终可用                                        │
│  ✓ Aura 编译器独立开发，失败可回退                              │
│  ✓ 两者可并行测试，对比验证                                     │
│  ✓ 迁移过程零风险                                              │
└──────────────────────────────────────────────────────────────┘
```

### 7.3 回退策略

1. **LLVM 不可用** → C 后端（Phase 6.10 的 `CBackend.aura`；Rust 侧 `c_backend.rs` 仅作参照）
2. **自举失败** → 保留 Rust 编译器作为 fallback
3. **方案一性能不足** → 升级到方案二（LLVM C API）
4. **泛型单态化失败** → 限制泛型使用范围（仅标量类型）
5. **JIT 失败/不可编译** → 回退解释器（Phase 4/5）；若 Aura 侧 Cranelift FFI 不可用，再回退 Rust JIT
6. **AOT 适配（Phase 6.5）未达标** → AOT 视为「实验性后端」；执行路径回退 VM（Phase 4/5）与 JIT（Phase 7），不阻塞其余 Phase
7. **Phase 失败** → 删除 `aura/compiler/` 目录，Rust 编译器不受影响
7. **任何时刻** → `cargo build` 仍然可用（Rust 编译器完全保留）

### 7.4 Phase 间隔离

每个 Phase 都是独立可回退的，且 **Rust 编译器始终可用**：

```
Phase 0 ─→ Phase 1 ─→ Phase 2 ─→ Phase 3 ─→ Phase 4 ─→ Phase 5 ─→ Phase 6 ─→ Phase 6.5 ─→ Phase 7 ─→ Phase 8 ─→ Phase 9
  │          │          │          │          │          │          │           │           │          │          │
  ↓          ↓          ↓          ↓          ↓          ↓          ↓           ↓           ↓          ↓          ↓
  fallback   fallback   fallback   fallback   fallback   fallback   fallback    fallback    fallback   fallback   fallback
  (Rust)     (Rust)     (Rust)     (Rust)     (Rust)     (Rust)     (Rust)      (Rust)      (Rust)     (Rust)     (Rust)

注意：fallback 始终是完整的 Rust 编译器，不是上一个 Phase 的 Aura 代码
```

---

## 八、总结

### 8.1 核心建议

1. **采用分阶段迁移（方案四）**：每个 Phase 独立可验证、可回退
2. **并行开发模型**：Aura 编译器作为独立代码层（`aura/compiler/`），Rust 编译器完全保留（`compiler/`）
3. **零风险迁移**：迁移过程不会破坏现有编译能力，任何 Phase 失败只需删除 `aura/compiler/` 目录
4. **优先迁移纯逻辑模块**：Lexer、Parser、Sema、HIR、MIR 最容易 Aura 化
5. **AOT 后端用方案一且直连 LLVM**：Aura 自研 IR 生成 + 直接调用 `llc`/`clang` 生成机器码，
   **跳过并零依赖 Rust AOT 后端**；等价性以行为差分测试判定
6. **JIT 补全三执行路径**：Cranelift 进程内原生码，必须继承 Fix A/B 与 7 个优化传递，
   避免「JIT 等于解释器」复发
7. **LLVM C API 作为可选优化**：仅在性能不足时考虑

### 8.2 预期成果

- **完整 Aura 编译器**：约 24,000 行 Aura 代码（`aura/compiler/`）
- **Rust 编译器保留**：约 43,000 行 Rust 代码（`compiler/`），完全不动
- **核心库/标准库 Aura 化**：核心类型 + `collection/` + `coroutine/` + `std/` 全部在
  `aura/core/aura/lang/` 中
- **自举成功**：Aura 编译器可编译自身
- **三执行路径**：VM 解释器 / JIT（Cranelift 进程内原生码）/ AOT（Aura 自研 LLVM IR + 直连 LLVM）语义一致、可路由
- **LLVM 后端可用**：Aura 侧自产 IR 并直连 LLVM，支持 AOT 编译、交叉编译（不依赖 Rust AOT 后端）
- **性能目标**：AOT 性能接近 Rust 实现（>90%）；JIT 循环/递归热点相对解释器 ≥ 50x

### 8.3 一句话总结

> **采用方案四的分阶段迁移路径，通过 10 个独立可验证的 Phase（含 AOT 适配修复阶段 Phase 6.5），在约 15 人月内将 Aura 编译器从 Rust 完全迁移到 Aura 语言自身。核心原则是"Aura 编译器独立代码层 + Rust 编译器完全保留"，确保迁移过程零风险。执行后端补齐 VM（Phase 4/5）、JIT（Phase 7，Cranelift 进程内原生码）、AOT（Phase 6，文本 IR + 子进程）三条路径并保持语义一致（其中 AOT 由 Aura 自产 LLVM IR 并**直连 LLVM 生成机器码**，跳过 Rust AOT 后端），LLVM C API 直连作为可选优化（Phase 9）。**

---

## 附录 A：当前代码库 LLVM 相关关键文件清单

| 文件 | 行数 | 职责 | 迁移 Phase |
|------|------|------|------------|
| `compiler/src/codegen/aot/mod.rs` | 354 | AOT 编排器 | Phase 6 |
| `compiler/src/codegen/aot/emit.rs` | 3380 | HIR → LLVM IR 文本（核心） | Phase 6 |
| `compiler/src/codegen/aot/types.rs` | 214 | 类型映射 | Phase 6 |
| `compiler/src/codegen/aot/linker.rs` | 646 | llc/clang 调用 + blob 提取 | Phase 6 |
| `compiler/src/codegen/aot/target.rs` | ~200 | 目标三元组 | Phase 6 |
| `compiler/src/codegen/aot/runtime.rs` | ~150 | Runtime 函数声明 | Phase 6 |
| `compiler/src/codegen/aot/ffi.rs` | ~100 | FFI 外部函数声明 | Phase 6 |
| `compiler/src/codegen/aot/dwarf.rs` | ~100 | DWARF 元数据 | Phase 6 |
| `compiler/src/codegen/aot/optimize.rs` | ~50 | 优化级别 | Phase 6 |
| `compiler/src/codegen/aot/c_backend.rs` | 749 | C 后端备选 | Phase 6 |
| `compiler/src/bootstrap/aot_core.rs` | 703 | Bootstrap 最小 AOT | Phase 5 |
| `compiler/src/codegen/mir.rs` | 1087 | MIR 定义 + HIR → MIR | Phase 3 |
| `compiler/src/codegen/emit.rs` | ~122 KB | MIR → VM 字节码 | Phase 4 |
| `compiler/src/vm/jit.rs` | ~29 KB | Cranelift JIT（热点/白名单/编译/派发） | Phase 7 |
| `compiler/src/vm/jit_opt.rs` | — | JIT 字节码优化（7 个传递） | Phase 7 |
| `compiler/src/vm/jit_native.rs` | — | JIT 原生码/机器码细节 | Phase 7 |
| `compiler/src/vm/abi.rs` | ~210 | `JitValue` / `AotEntry` ABI | Phase 7 |
| `compiler/src/vm/aot_runtime.rs` | ~1130 | W^X 加载 / 描述符表 / Blob 运行时 | Phase 7 |
| `compiler/src/bootstrap/jit_core.rs` | — | Bootstrap 最小 JIT 引导 | Phase 7 |
| `compiler/src/codegen/ffi_aot.rs` | ~220 | AOT FFI 直连 | Phase 6 |

---

## 附录 B：关键 LLVM IR 生成示例

### B.1 简单函数

```aura
// Aura 源码
func add(a: Int, b: Int): Int {
    return a + b
}
```

```llvm
; 生成的 LLVM IR
define i32 @add(i32 %arg.a, i32 %arg.b) {
entry:
  %var.0 = alloca i32
  store i32 %arg.a, i32* %var.0
  %var.1 = alloca i32
  store i32 %arg.b, i32* %var.1
  %var.2 = alloca i32
  %var.3 = load i32, i32* %var.0
  %var.4 = load i32, i32* %var.1
  %var.5 = add i32 %var.3, %var.4
  store i32 %var.5, i32* %var.2
  %var.6 = load i32, i32* %var.2
  ret i32 %var.6
}
```

### B.2 字符串拼接

```aura
// Aura 源码
func greet(name: String): String {
    return "Hello, " + name + "!"
}
```

```llvm
; 生成的 LLVM IR
@str_data.0 = private constant [8 x i8] c"Hello, \00"
@str_data.1 = private constant [2 x i8] c"!\00"

define { i8*, i64 } @greet({ i8*, i64 } %arg.name) {
entry:
  %var.0 = alloca { i8*, i64 }
  store { i8*, i64 } %arg.name, { i8*, i64 }* %var.0
  %var.1 = alloca { i8*, i64 }
  %var.2 = load { i8*, i64 }, { i8*, i64 }* %var.0
  %var.3 = getelementptr [8 x i8], [8 x i8]* @str_data.0, i64 0, i64 0
  %var.4 = insertvalue { i8*, i64 } undef, i8* %var.3, 0
  %var.5 = insertvalue { i8*, i64 } %var.4, i64 7, 1
  %var.6 = extractvalue { i8*, i64 } %var.2, 0
  %var.7 = extractvalue { i8*, i64 } %var.2, 1
  %var.8 = call i8* @aura_string_concat(i8* %var.3, i64 7, i8* %var.6, i64 %var.7)
  %var.9 = add i64 7, %var.7
  %var.10 = insertvalue { i8*, i64 } undef, i8* %var.8, 0
  %var.11 = insertvalue { i8*, i64 } %var.10, i64 %var.9, 1
  ret { i8*, i64 } %var.11
}
```

### B.3 If 语句

```aura
// Aura 源码
func max(a: Int, b: Int): Int {
    if a > b {
        return a
    } else {
        return b
    }
}
```

```llvm
; 生成的 LLVM IR
define i32 @max(i32 %arg.a, i32 %arg.b) {
entry:
  %var.0 = alloca i32
  store i32 %arg.a, i32* %var.0
  %var.1 = alloca i32
  store i32 %arg.b, i32* %var.1
  %var.2 = load i32, i32* %var.0
  %var.3 = load i32, i32* %var.1
  %var.4 = icmp sgt i32 %var.2, %var.3
  br i1 %var.4, label %bb_then_0, label %bb_else_1

bb_then_0:
  %var.5 = load i32, i32* %var.0
  ret i32 %var.5

bb_else_1:
  %var.6 = load i32, i32* %var.1
  ret i32 %var.6

bb_merge_2:
  ; 空块
}
```

### B.4 While 循环

```aura
// Aura 源码
func sum(n: Int): Int {
    var s = 0
    var i = 1
    while i <= n {
        s += i
        i += 1
    }
    return s
}
```

```llvm
; 生成的 LLVM IR
define i32 @sum(i32 %arg.n) {
entry:
  %var.0 = alloca i32
  store i32 %arg.n, i32* %var.0
  %var.1 = alloca i32
  %var.2 = add i32 0, 0
  store i32 %var.2, i32* %var.1
  %var.3 = alloca i32
  %var.4 = add i32 0, 1
  store i32 %var.4, i32* %var.3
  br label %bb_loop.cond_0

bb_loop.cond_0:
  %var.5 = load i32, i32* %var.3
  %var.6 = load i32, i32* %var.0
  %var.7 = icmp sle i32 %var.5, %var.6
  br i1 %var.7, label %bb_loop.body_1, label %bb_loop.end_2

bb_loop.body_1:
  %var.8 = load i32, i32* %var.1
  %var.9 = load i32, i32* %var.3
  %var.10 = add i32 %var.8, %var.9
  store i32 %var.10, i32* %var.1
  %var.11 = load i32, i32* %var.3
  %var.12 = add i32 %var.11, 1
  store i32 %var.12, i32* %var.3
  br label %bb_loop.cond_0

bb_loop.end_2:
  %var.13 = load i32, i32* %var.1
  ret i32 %var.13
}
```

---

## 附录 C：Phase 间依赖图

```
Phase 0 (基础设施)
    │
    ├──→ Phase 1 (Lexer/Parser/AST)
    │       │
    │       ├──→ Phase 2 (Sema/HIR)
    │       │       │
    │       │       ├──→ Phase 3 (MIR/Optimizer)
    │       │       │       │
    │       │       │       └──→ Phase 4 (VM Emit/Interp)
    │       │       │               │
    │       │       │               ├──→ Phase 5 (VM 自举)
    │       │       │               │       │
    │       │       │               │       └──→ Phase 6 (AOT LLVM)
    │       │       │               │               │
    │       │       │               │               ├──→ Phase 6.5 (AOT 适配与修复)
    │       │       │               │               │       │
    │       │       │               │               │       └──→ Phase 8 (核心库/标准库)
    │       │       │               │               │               │
    │       │       │               │               │               └──→ Phase 9 (LLVM C API, 可选)
    │       │       │               │
    │       │       │               └──→ Phase 7 (JIT / Cranelift)
    │       │       │
    │       │       └──→ Phase 6 (AOT LLVM, 依赖 Phase 2 的 HIR)
    │       │
    │       └──→ Phase 6 (AOT LLVM, 依赖 Phase 1 的 AST)
    │
    └──→ Phase 6 (AOT LLVM, 依赖 Phase 0 的工具链)
```

**关键依赖**：
- Phase 6 依赖 Phase 1（AST）、Phase 2（HIR）、Phase 0（工具链）
- Phase 5 依赖 Phase 4（VM）
- **Phase 6.5 依赖 Phase 6（AOT 后端）——AOT 适配与修复阶段，是 AOT 可用的前置门禁**
- Phase 7 依赖 Phase 3（MIR）与 Phase 4（VM / 字节码）——JIT 与 AOT 并列，不互相依赖
- Phase 8 依赖 Phase 6.5（AOT 可用）与 Phase 7（JIT）
- Phase 9 依赖 Phase 8（标准库）

---

## 附录 D：验证用例清单

### D.1 Phase 1 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 1.1 | `tests/lexer_control_flow.aura` | if/else/while/for token 化 |
| 1.2 | `tests/lexer_declaration.aura` | val/var/fun/class token 化 |
| 1.3 | `tests/lexer_doc_comment.aura` | 文档注释 token 化 |
| 1.4 | `tests/lexer_string_interpolation.aura` | 字符串插值 token 化 |
| 1.5 | `tests/parser_function.aura` | 函数解析 |
| 1.6 | `tests/parser_class.aura` | 类解析 |
| 1.7 | `tests/parser_generic_function.aura` | 泛型函数解析 |

### D.2 Phase 2 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 2.1 | `tests/sema_type_errors.aura` | 类型错误检测 |
| 2.2 | `tests/sema_null_safety.aura` | 空安全检查 |
| 2.3 | `tests/sema_clean_program.aura` | 正确程序通过 |
| 2.4 | `tests/hir_function.aura` | 函数 HIR 生成 |
| 2.5 | `tests/hir_enum.aura` | 枚举 HIR 生成 |
| 2.6 | `tests/hir_lambda.aura` | Lambda HIR 生成 |

### D.3 Phase 3 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 3.1 | `tests/mir_basic.aura` | 基本 MIR 生成 |
| 3.2 | `tests/mir_control_flow.aura` | 控制流 MIR 生成 |
| 3.3 | `tests/mir_optimization.aura` | 优化器验证 |

### D.4 Phase 4 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 4.1 | `tests/vm_arithmetic.aura` | 算术运算 |
| 4.2 | `tests/vm_control_flow.aura` | 控制流执行 |
| 4.3 | `tests/vm_function_call.aura` | 函数调用 |
| 4.4 | `tests/vm_string.aura` | 字符串操作 |

### D.5 Phase 6 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 6.1 | `tests/aot_simple.aura` | 简单函数 AOT 编译 |
| 6.2 | `tests/aot_string.aura` | 字符串 AOT 编译 |
| 6.3 | `tests/aot_control_flow.aura` | 控制流 AOT 编译 |
| 6.4 | `tests/aot_ffi.aura` | FFI AOT 编译 |
| 6.5 | `tests/aot_cross_compile.aura` | 交叉编译 |
| 6.6 | `tests/aot/direct_llvm.aura` | **直连 LLVM**：Aura 自产 IR → `llc` → `clang` → 可执行文件（全程不依赖 Rust AOT 产物） |
| 6.7 | `tests/aot/diff_vs_rust_aot.aura` | **行为差分**：同一源码 Aura-AOT vs Rust-AOT，运行输出与退出码一致 |

### D.6 Phase 6.5 验证用例（AOT 适配与修复）

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 6.5.1 | `tests/aot/class_pointer.aura` | 类实例统一指针表示：字段读写 / 方法调用 / 返回对象 |
| 6.5.2 | `tests/aot/ret_type.aura` | `ret` 按函数返回类型转换（`%struct.X` ⇄ `i8*`）无非法 IR |
| 6.5.3 | `tests/aot/runtime_symbols.aura` | 运行库调用点符号可链接（String / Collections / Process / FileSystem） |
| 6.5.4 | `tests/aot/string_runtime.aura` | String 全量方法（length/contains/indexOf/countChar/substring/startsWith…） |
| 6.5.5 | `tests/aot/list_runtime.aura` | `split` → 列表 size / 下标取值 / 内容比较 |
| 6.5.6 | `tests/aot/mixed_int.aura` | i32/i64 混用（算术 / 比较 / 下标）产出合法 IR |
| 6.5.7 | `tests/aot/verify_gate.aura` | `llc -verify` 门禁：全量用例无非法 IR |
| 6.5.8 | `tests/aot/self_compile_exe.aura` | **阶段性目标**：Aura 编译器自身 AOT 出可运行 exe |

### D.7 Phase 7 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 7.1 | `tests/jit_hotness.aura` | 热点检测（入口强制编译 + 递归可达） |
| 7.2 | `tests/jit_optimize.aura` | 7 个优化传递结果一致 |
| 7.3 | `tests/jit_dispatch.aura` | `dispatch_table` / `call_indirect` 递归调用 |
| 7.4 | `tests/jit_fallback.aura` | 不可编译函数回退解释器 |
| 7.5 | `tests/jit_vs_vm_vs_aot.aura` | 三路（VM/JIT/AOT）结果差分 |

### D.8 Phase 8 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 8.1 | `tests/core_types.aura` | 核心类型（`aura/core/aura/lang/*.aura`）方法/运算符 |
| 8.2 | `tests/std_math.aura` | Math 模块 |
| 8.3 | `tests/std_string.aura` | String 模块 |
| 8.4 | `tests/std_collections.aura` | `collection/` 集合实现 |
| 8.5 | `tests/std_iter.aura` | Iter 模块 |
| 8.6 | `tests/std_io.aura` | IO 模块 |
| 8.7 | `tests/std_fs.aura` | FileSystem 模块 |
| 8.8 | `tests/std_concurrent.aura` | `coroutine/`（Coroutine/Actor）+ `std/Channel` |

---

## 附录 E：与现有文档的关系

| 文档 | 关系 |
|------|------|
| `docs/编译器后端方案.md` | 原设计文档，推荐 inkwell 绑定；本文档澄清当前实现采用文本 IR |
| `docs/完全Aura化技术方案-final.md` | 完全 Aura 化总方案；本文档聚焦 LLVM 交互细节和迁移计划 |
| `docs/完全Aura化架构重新规划.md` | 架构重新规划；本文档补充 LLVM 绑定策略和 Phase 计划 |
| `docs/标准库上移与AOT直连性能影响分析.md` | AOT 直连性能分析；本文档补充 LLVM 交互机制 |
| `docs/AOT机器码嵌入方案-详细设计.md` | AOT Blob 嵌入设计；本文档引用其 `link_to_blob` 机制 |
| `docs/AOT一致性检查报告.md` | AOT 一致性检查；本文档参考其发现的 LLVM IR 生成问题 |
| `docs/JIT性能分析.md` | JIT 热点不可达根因与 Fix A/B、7 优化传递；本文档 Phase 7 的设计依据 |
| `docs/jit优化指南.md` | JIT 优化清单；本文档 Phase 7 `JitOpt` 的参照 |
| `docs/绕过LLVM直生成机器码-可行性评估与技术方案.md` | Cranelift AOT / 直接机器码可行性评估；本文档 Phase 7 的选型参考 |
| `docs/开发规划与实现进度.md` | 现有开发进度；本文档补充 Aura 化迁移计划 |
