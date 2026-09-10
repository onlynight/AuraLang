# Aura 编译器 LLVM 交互分析与纯 Aura 化迁移计划

> **文档性质**：技术分析 + 开发计划
> **方案选择**：方案四——分阶段迁移路径
> **约束**：本文仅提供分析与计划，不修改任何代码
> **分析日期**：2026-09-10
> **预计执行周期**：约 11–13 人月（单人全职）

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
  - [4.8 Phase 6：AOT LLVM 后端 Aura 化](#48-phase-6aot-llvm-后端-aura-化)
  - [4.9 Phase 7：标准库 Aura 化](#49-phase-7标准库-aura-化)
  - [4.10 Phase 8：LLVM C API 直连（可选优化）](#410-phase-8llvm-c-api-直连可选优化)
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

选择**方案四：分阶段迁移路径**，采用 8 个 Phase 的渐进式迁移策略，每个 Phase 独立可验证、可回退。

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

### 关键技术选择

| 维度 | 决策 | 理由 |
|------|------|------|
| **代码组织** | `aura/compiler/` 独立目录 | 与 `compiler/` 并行，互不干扰 |
| **Rust 编译器** | 完全保留，不修改 | 作为 fallback 和参考实现 |
| **LLVM 后端方式** | 文本 IR + 外部 `llc`/`clang` 子进程 | 与现有 Rust 实现同构，无需绑定 LLVM C API |
| **IR 生成入口** | HIR 直发（绕过 MIR） | 与现有实现一致，避免冗余转换 |
| **MIR 定位** | VM 字节码路径专用 | LLVM 不需要 MIR 这种 CFG |
| **引导层** | 保留最小 Rust 引导层（Layer 0-A） | 解决鸡生蛋问题 |
| **FFI 方式** | Aura `extern "C"` 调用 C ABI | 已有完整支持 |
| **构建系统** | 双编译器并行构建 | Rust/Aura 编译器独立构建，互不依赖 |

### 关键数据

| 指标 | 数值 |
|------|------|
| 当前 Rust 编译器规模 | 107 个 .rs 文件，约 43,000 行（`compiler/`） |
| 已有 Aura 标准库文件 | 55 个 .aura 文件（`aura/` + `core/`） |
| 需要迁移的 Rust 文件 | 约 40 个核心文件 → `aura/compiler/` |
| 预计 Aura 代码量 | 约 20,000 行（`aura/compiler/`） |
| 预计总工期 | 11–13 人月 |
| Phase 数量 | 8 个（含 2 个可选 Phase） |
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

### 1.3 五步式 AOT 流程

```
HIR → emit_program() → LLVM IR 文本 → 写 .ll → llc → .o → clang → .exe
```

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
| 方案一 | 文本 IR + 子进程（Aura 重写） | 中 | 中 | 低 | ⭐⭐⭐⭐ |
| 方案二 | C FFI 直连 LLVM C API | 高 | 高 | 高 | ⭐⭐⭐ |
| 方案三 | Aura-to-C 转译（fallback） | 低 | 低 | 低 | ⭐⭐ |
| **方案四** | **分阶段迁移** | **高** | **高** | **中** | **⭐⭐⭐⭐⭐** |

### 3.2 选择方案四的理由

1. **风险可控**：每个 Phase 独立可验证、可回退
2. **渐进式改进**：每完成一个 Phase 就获得一个可交付成果
3. **保留 fallback**：Rust 编译器始终可用
4. **技术路线清晰**：Phase 1–7 用方案一（文本 IR），Phase 8 可选升级到方案二
5. **与现有文档对齐**：符合 `完全Aura化技术方案-final.md` 的分层架构

### 3.3 方案四的技术路线

```
Phase 0: 准备
    ↓
Phase 1-4: 前端 + IR 生成器 Aura 化（纯逻辑，无 FFI 依赖）
    ↓
Phase 5: VM 自举（需要最小 Rust 引导层）
    ↓
Phase 6: AOT LLVM 后端 Aura 化（用方案一：文本 IR + 子进程）
    ↓
Phase 7: 标准库 Aura 化
    ↓
Phase 8: LLVM C API 直连（可选，用方案二）
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
│   │   │   └── Emit.aura              # Aura AOT LLVM 后端
│   │   └── main.aura                  # Aura 编译器入口
│   └── core/                          # Aura 标准库
│       └── lang/std/
│           ├── Math.aura
│           ├── String.aura
│           └── ...
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
# 用 Rust 编译器编译
aura --rust-compiler --aot tests/example.aura → output_rust/

# 用 Aura 编译器编译
aura-compiler --aot tests/example.aura → output_aura/

# 对比输出
diff output_rust/example.ll output_aura/example.ll   # LLVM IR 对比
diff output_rust/example.o output_aura/example.o     # 目标文件对比
```

#### 关键约束

| 约束 | 说明 |
|------|------|
| **Rust 编译器零修改** | `compiler/` 目录完全保留，不修改任何文件 |
| **Aura 编译器独立目录** | `aura/compiler/` 是新增目录，不影响现有代码 |
| **构建系统独立** | Rust/Aura 编译器各自独立构建，互不依赖 |
| **CLI 双模式** | `aura` CLI 支持 `--rust-compiler` 和 `--aura-compiler` 两种模式 |
| **测试对比** | 每个 Phase 完成后，运行对比测试验证输出一致性 |
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
    │                                        │        └→ Phase 5 (自举)
    │                                        │
    │                                        └──→ Phase 6 (AOT LLVM)
    │                                                │
    │                                                └──→ Phase 7 (标准库)
    │                                                        │
    │                                                        └──→ Phase 8 (LLVM C API, 可选)
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
├── aot/                            # AOT LLVM 后端（Phase 6）
├── std/                            # 标准库（Phase 7）
├── test/                           # 测试框架
└── main.aura                       # 编译器入口
```

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
| VM 性能不足 | 热点函数 JIT 编译（Cranelift） |
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

### 4.8 Phase 6：AOT LLVM 后端 Aura 化

#### 目标

将 AOT LLVM 后端（HIR → LLVM IR 文本 + 子进程调用）从 Rust 迁移到 Aura。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/codegen/aot/emit.rs` | 3380 行 | `aura/lang/compiler/aot/Emit.aura` | 10d | P0 |
| `compiler/src/codegen/aot/types.rs` | 214 行 | `aura/lang/compiler/aot/TypeMapper.aura` | 2d | P0 |
| `compiler/src/codegen/aot/linker.rs` | 646 行 | `aura/lang/compiler/aot/Linker.aura` | 4d | P0 |
| `compiler/src/codegen/aot/target.rs` | ~200 行 | `aura/lang/compiler/aot/Target.aura` | 1d | P0 |
| `compiler/src/codegen/aot/runtime.rs` | ~150 行 | `aura/lang/compiler/aot/Runtime.aura` | 1d | P1 |
| `compiler/src/codegen/aot/ffi.rs` | ~100 行 | `aura/lang/compiler/aot/Ffi.aura` | 1d | P1 |
| `compiler/src/codegen/aot/dwarf.rs` | ~100 行 | `aura/lang/compiler/aot/Dwarf.aura` | 1d | P2 |
| `compiler/src/codegen/aot/optimize.rs` | ~50 行 | `aura/lang/compiler/aot/Optimize.aura` | 0.5d | P1 |
| `compiler/src/codegen/aot/c_backend.rs` | 749 行 | `aura/lang/compiler/aot/CBackend.aura` | 3d | P2 |

#### 迁移策略

1. **LLVM IR 生成**：使用 `StringBuilder` 拼接 LLVM IR 文本
2. **类型映射**：将 `TypeMapper` 翻译为 Aura 函数
3. **子进程调用**：使用 `Process.aura` 的 `exec()` 方法调用 `llc`/`clang`
4. **工具发现**：使用 `Env.aura` 和 `File.aura` 探测 LLVM 工具

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| IR 生成 | `StringBuilder` 拼接 | 高效、可控 |
| 子进程调用 | `Process.exec(argv)` | 与 Rust `Command` 一致 |
| 工具发现 | 五级探测（同 Rust） | 与 Rust 一致 |
| 错误处理 | `Result<AotOutput, AotError>` | 与 Rust 一致 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 6.1 | 迁移 `types.rs` → `TypeMapper.aura` | 2d | 2.1 |
| 6.2 | 迁移 `target.rs` → `Target.aura` | 1d | 6.1 |
| 6.3 | 迁移 `runtime.rs` → `Runtime.aura` | 1d | 6.1 |
| 6.4 | 迁移 `ffi.rs` → `Ffi.aura` | 1d | 6.1 |
| 6.5 | 迁移 `optimize.rs` → `Optimize.aura` | 0.5d | 6.1 |
| 6.6 | 迁移 `emit.rs` → `Emit.aura` | 10d | 6.1–6.5 |
| 6.7 | 迁移 `linker.rs` → `Linker.aura` | 4d | 6.6 |
| 6.8 | 迁移 `dwarf.rs` → `Dwarf.aura` | 1d | 6.6 |
| 6.9 | 迁移 `c_backend.rs` → `CBackend.aura` | 3d | 6.6 |
| 6.10 | 编写 Phase 6 验证用例 | 3d | 6.7 |
| 6.11 | 集成测试：LLVM IR 输出对比 | 3d | 6.10 |
| 6.12 | 集成测试：可执行文件输出对比 | 3d | 6.11 |

#### 验证标准

- [ ] `Emit.aura` 生成的 LLVM IR 与 Rust 一致（文本对比）
- [ ] `Linker.aura` 生成的可执行文件与 Rust 一致（MD5 对比）
- [ ] 支持所有输出格式（`.ll`、`.o`、`.exe`、`.blob`、`.so`）
- [ ] 交叉编译支持（`-mtriple`）
- [ ] C 后端 fallback 可用

#### 风险与缓解

| 风险 | 缓解 |
|------|------|
| LLVM IR 文本拼接错误 | 使用 `llc -verify` 验证 IR |
| 子进程调用失败 | 保留 Rust 编译器作为 fallback |
| 工具发现失败 | 配置 `AURA_LLVM_HOME` 环境变量 |

---

### 4.9 Phase 7：标准库 Aura 化

#### 目标

将标准库从 Rust 实现上移到 Aura 源码，实现 `core/aura/lang/std/` 作为唯一真相源。

#### 迁移文件清单

| Rust 文件 | 行数 | Aura 目标文件 | 预估 | 优先级 |
|-----------|------|---------------|------|--------|
| `compiler/src/std/std_math.rs` | ~30 KB | `core/aura/lang/std/Math.aura` | 2d | P0 |
| `compiler/src/std/std_string.rs` | ~50 KB | `core/aura/lang/std/String.aura` | 5d | P0 |
| `compiler/src/std/std_path.rs` | ~20 KB | `core/aura/lang/std/Path.aura` | 2d | P0 |
| `compiler/src/std/std_encoding.rs` | ~30 KB | `core/aura/lang/std/Encoding.aura` | 3d | P0 |
| `compiler/src/std/std_time.rs` | ~20 KB | `core/aura/lang/std/Time.aura` | 2d | P1 |
| `compiler/src/std/std_collections.rs` | ~40 KB | `core/aura/lang/std/Collections.aura` | 4d | P0 |
| `compiler/src/std/std_io.rs` | ~30 KB | `core/aura/lang/std/IO.aura` | 3d | P1 |
| `compiler/src/std/std_fs.rs` | ~30 KB | `core/aura/lang/std/FileSystem.aura` | 3d | P1 |
| `compiler/src/std/std_net.rs` | ~40 KB | `core/aura/lang/std/Network.aura` | 5d | P2 |
| `compiler/src/std/std_json.rs` | ~30 KB | `core/aura/lang/std/Json.aura` | 4d | P2 |
| `compiler/src/std/std_assert.rs` | ~10 KB | `core/aura/lang/std/Assert.aura` | 1d | P0 |
| `compiler/src/std/std_test.rs` | ~15 KB | `core/aura/lang/std/Test.aura` | 2d | P1 |
| `compiler/src/std/std_iter.rs` | ~20 KB | `core/aura/lang/std/Iter.aura` | 3d | P1 |
| `compiler/src/std/std_env.rs` | ~10 KB | `core/aura/lang/std/Env.aura` | 1d | P1 |
| `compiler/src/std/std_process.rs` | ~15 KB | `core/aura/lang/std/Process.aura` | 2d | P1 |
| `compiler/src/std/std_random.rs` | ~10 KB | `core/aura/lang/std/Random.aura` | 1d | P2 |
| `compiler/src/std/std_builtin.rs` | ~20 KB | `core/aura/lang/std/Builtin.aura` | 3d | P0 |
| `compiler/src/std/std_console.rs` | ~10 KB | `core/aura/lang/std/Console.aura` | 1d | P1 |
| `compiler/src/std/std_ascii.rs` | ~10 KB | `core/aura/lang/std/Ascii.aura` | 1d | P2 |
| `compiler/src/std/std_path.rs` | ~20 KB | `core/aura/lang/std/Path.aura` | 2d | P0 |

#### 迁移策略

1. **纯逻辑模块**（Math、String、Path、Encoding、Time）：直接翻译为 Aura 函数
2. **FFI 模块**（IO、FileSystem、Network、Process）：使用 `extern "C"` 声明 + Aura 包装
3. **集合模块**（Collections、Iter）：使用 Aura 的 `List`/`Map`/`Set` 实现
4. **测试模块**（Assert、Test）：使用 Aura 的异常和断言

#### 关键技术决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 纯逻辑 | Aura 函数 | 无 FFI 依赖 |
| FFI 模块 | `extern "C"` + Aura 包装 | 类型安全 |
| 集合 | Aura 原生类型 | 与语言集成 |
| 错误处理 | `Result<T, E>` 或异常 | 与 Rust 一致 |

#### 任务清单

| # | 任务 | 预估 | 依赖 |
|---|------|------|------|
| 7.1 | 迁移 `std_math.rs` → `Math.aura` | 2d | 6.12 |
| 7.2 | 迁移 `std_string.rs` → `String.aura` | 5d | 7.1 |
| 7.3 | 迁移 `std_path.rs` → `Path.aura` | 2d | 7.1 |
| 7.4 | 迁移 `std_encoding.rs` → `Encoding.aura` | 3d | 7.2 |
| 7.5 | 迁移 `std_collections.rs` → `Collections.aura` | 4d | 7.2 |
| 7.6 | 迁移 `std_builtin.rs` → `Builtin.aura` | 3d | 7.1 |
| 7.7 | 迁移 `std_assert.rs` → `Assert.aura` | 1d | 7.1 |
| 7.8 | 迁移 `std_io.rs` → `IO.aura` | 3d | 7.1 |
| 7.9 | 迁移 `std_fs.rs` → `FileSystem.aura` | 3d | 7.8 |
| 7.10 | 迁移 `std_time.rs` → `Time.aura` | 2d | 7.1 |
| 7.11 | 迁移 `std_env.rs` → `Env.aura` | 1d | 7.8 |
| 7.12 | 迁移 `std_process.rs` → `Process.aura` | 2d | 7.8 |
| 7.13 | 迁移 `std_iter.rs` → `Iter.aura` | 3d | 7.5 |
| 7.14 | 迁移 `std_test.rs` → `Test.aura` | 2d | 7.7 |
| 7.15 | 迁移 `std_net.rs` → `Network.aura` | 5d | 7.8 |
| 7.16 | 迁移 `std_json.rs` → `Json.aura` | 4d | 7.2 |
| 7.17 | 迁移 `std_random.rs` → `Random.aura` | 1d | 7.1 |
| 7.18 | 迁移 `std_console.rs` → `Console.aura` | 1d | 7.8 |
| 7.19 | 迁移 `std_ascii.rs` → `Ascii.aura` | 1d | 7.2 |
| 7.20 | 编写 Phase 7 验证用例 | 3d | 7.1–7.19 |
| 7.21 | 集成测试：标准库 API 对比 | 3d | 7.20 |

#### 验证标准

- [ ] 所有标准库函数在 Aura 中可用
- [ ] API 签名与 Rust 一致
- [ ] 测试用例全部通过
- [ ] FFI 模块正确调用 C 函数

---

### 4.10 Phase 8：LLVM C API 直连（可选优化）

#### 目标

在方案一（文本 IR + 子进程）性能不足时，升级到方案二（C FFI 直连 LLVM C API）。

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
| 8.1 | 编写 `llvm_bindings.h` | 2d | 7.21 |
| 8.2 | 编写 `llvm_bindings.c` | 1d | 8.1 |
| 8.3 | 编写 `llvm_bindings.aura` | 5d | 8.2 |
| 8.4 | 编写 `llvm_codegen.aura` | 10d | 8.3 |
| 8.5 | 集成测试：LLVM C API 输出对比 | 3d | 8.4 |

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
| Phase 6 | AOT LLVM 后端 | 39d | 2.0 |
| Phase 7 | 标准库 | 45d | 2.3 |
| Phase 8 | LLVM C API（可选） | 21d | 1.1 |
| **总计** | | **235d** | **12.0** |

### 6.2 里程碑

```
M0（第 0.3 月末）：Phase 0 完成，基础设施就绪
M1（第 1.3 月末）：Phase 1 完成，Lexer/Parser 可独立运行
M2（第 3.0 月末）：Phase 2 完成，Sema/HIR 可独立运行
M3（第 3.8 月末）：Phase 3 完成，MIR/Optimizer 可独立运行
M4（第 5.5 月末）：Phase 4 完成，VM 字节码发射可用
M5（第 6.6 月末）：Phase 5 完成，VM 自举成功
M6（第 8.6 月末）：Phase 6 完成，AOT LLVM 后端可用
M7（第 10.9 月末）：Phase 7 完成，标准库 Aura 化完成
M8（第 12.0 月末）：Phase 8 完成，LLVM C API 直连（可选）
```

### 6.3 交付物清单

| 里程碑 | 交付物 | 验证方式 |
|--------|--------|----------|
| M1 | `Lexer.aura` + `Parser.aura` | 快照测试 |
| M2 | `TypeChecker.aura` + `Hir.aura` | 类型检查测试 |
| M3 | `Mir.aura` + `Optimizer.aura` | MIR 输出对比 |
| M4 | `Emit.aura` + `Interp.aura` | VM 执行测试 |
| M5 | 最小编译器 + 自举成功 | 编译标准库 |
| M6 | `Emit.aura`（AOT）+ `Linker.aura` | 可执行文件对比 |
| M7 | 标准库 `.aura` 文件 | API 对比测试 |
| M8 | `llvm_bindings.aura` + `llvm_codegen.aura` | 性能对比 |

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
| 最小编译器功能不足 | 中 | 高 | 逐步扩展指令集和语法支持 |
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

1. **LLVM 不可用** → C 后端（`c_backend.rs` 已实现）
2. **自举失败** → 保留 Rust 编译器作为 fallback
3. **方案一性能不足** → 升级到方案二（LLVM C API）
4. **泛型单态化失败** → 限制泛型使用范围（仅标量类型）
5. **Phase 失败** → 删除 `aura/compiler/` 目录，Rust 编译器不受影响
6. **任何时刻** → `cargo build` 仍然可用（Rust 编译器完全保留）

### 7.4 Phase 间隔离

每个 Phase 都是独立可回退的，且 **Rust 编译器始终可用**：

```
Phase 0 ──→ Phase 1 ──→ Phase 2 ──→ Phase 3 ──→ Phase 4 ──→ Phase 5 ──→ Phase 6 ──→ Phase 7
  │            │            │            │            │            │            │            │
  ↓            ↓            ↓            ↓            ↓            ↓            ↓            ↓
  fallback     fallback     fallback     fallback     fallback     fallback     fallback     fallback
  (Rust)       (Rust)       (Rust)       (Rust)       (Rust)       (Rust)       (Rust)       (Rust)

注意：fallback 始终是完整的 Rust 编译器，不是上一个 Phase 的 Aura 代码
```

---

## 八、总结

### 8.1 核心建议

1. **采用分阶段迁移（方案四）**：每个 Phase 独立可验证、可回退
2. **并行开发模型**：Aura 编译器作为独立代码层（`aura/compiler/`），Rust 编译器完全保留（`compiler/`）
3. **零风险迁移**：迁移过程不会破坏现有编译能力，任何 Phase 失败只需删除 `aura/compiler/` 目录
4. **优先迁移纯逻辑模块**：Lexer、Parser、Sema、HIR、MIR 最容易 Aura 化
5. **AOT 后端用方案一**：文本 IR + 子进程，与现有实现同构
6. **LLVM C API 作为可选优化**：仅在性能不足时考虑

### 8.2 预期成果

- **完整 Aura 编译器**：约 20,000 行 Aura 代码（`aura/compiler/`）
- **Rust 编译器保留**：约 43,000 行 Rust 代码（`compiler/`），完全不动
- **标准库 Aura 化**：20+ 模块，全部在 `core/aura/lang/std/` 中
- **自举成功**：Aura 编译器可编译自身
- **LLVM 后端可用**：支持 AOT 编译、交叉编译
- **性能目标**：AOT 性能接近 Rust 实现（>90%）

### 8.3 一句话总结

> **采用方案四的分阶段迁移路径，通过 8 个独立可验证的 Phase，在 12 人月内将 Aura 编译器从 Rust 完全迁移到 Aura 语言自身。核心原则是"Aura 编译器独立代码层 + Rust 编译器完全保留"，确保迁移过程零风险。AOT 后端采用文本 IR + 子进程方案（方案一），与现有实现同构，LLVM C API 直连作为可选优化（Phase 8）。**

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
| `compiler/src/vm/jit.rs` | ~29 KB | Cranelift JIT | Phase 5 |
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
    │       │       │               └──→ Phase 5 (VM 自举)
    │       │       │                       │
    │       │       │                       └──→ Phase 6 (AOT LLVM)
    │       │       │                               │
    │       │       │                               └──→ Phase 7 (标准库)
    │       │       │                                       │
    │       │       │                                       └──→ Phase 8 (LLVM C API, 可选)
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
- Phase 7 依赖 Phase 6（AOT LLVM）
- Phase 8 依赖 Phase 7（标准库）

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

### D.6 Phase 7 验证用例

| 用例 | 文件 | 验证内容 |
|------|------|----------|
| 7.1 | `tests/std_math.aura` | Math 模块 |
| 7.2 | `tests/std_string.aura` | String 模块 |
| 7.3 | `tests/std_collections.aura` | Collections 模块 |
| 7.4 | `tests/std_io.aura` | IO 模块 |
| 7.5 | `tests/std_fs.aura` | FileSystem 模块 |

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
| `docs/开发规划与实现进度.md` | 现有开发进度；本文档补充 Aura 化迁移计划 |
