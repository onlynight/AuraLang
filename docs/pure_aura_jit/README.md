# 纯 Aura JIT 化技术方案

> **文档定位**：JIT 后端的纯 Aura 化迁移方案与分阶段开发计划
> **配套目录**：`docs/pure_aura/`（整体纯 Aura 化方案）、`aura/compiler/aura/lang/compiler/jit/`（JIT Aura 实现）
> **代码边界**：除 `compiler/src/bootstrap/`（最小引导层，明确保留 Rust）外，JIT 全部逻辑由 Aura 实现
> **文档日期**：2026-09-13（与 `docs/pure_aura/03-自举验证报告.md` 同步）
> **状态**：方案设计中，分阶段交付

---

## 一、问题域

Aura 编译器的 JIT 后端当前由 **Cranelift 0.116**（Bytecode Alliance，Rust）承担机器码生成，
位置在 `compiler/src/vm/jit.rs`（807 行）+ `jit_opt.rs` + `jit_native.rs`，通过 cargo `jit`
feature 门控（默认关闭）。

纯 Aura 化目标（`docs/pure_aura/02-纯Aura化改造方案.md`）要求：

> 除 `compiler/src/bootstrap/`（最小引导层，明确保留 Rust）外，完全脱离 Rust 编译器。

但 JIT 的「机器码后端」涉及 `mmap` / `mprotect` / W^X 装载等操作系统级操作，
**任何纯 Aura 方案都必须通过 FFI 调用系统调用**。因此「完全脱 Rust」在工程上不现实。

本文档回答三个问题：

1. **边界怎么划？** 哪些留在纯 Aura 侧、哪些归 bootstrap？
2. **技术方案是什么？** 数据流、ABI 契约、W^X 装载策略如何设计？
3. **怎么分阶段独立开发测试？** 每个阶段的验收标准是什么？

---

## 二、核心结论（先看这一段）

> **策略**：保留 Cranelift 在 bootstrap 层，但把它严格圈进「最小引导层」的边界。
> 纯 Aura 侧只做「字节码 → Cranelift 文本 IR（.clif）」的纯函数翻译——
> 编译、mmap 装载、分发表管理由 bootstrap FFI 承担。
>
> **理由**：
> 1. JIT 的机器码装载必然踩 OS syscall（mmap/mprotect），Aura 无法绕过；
> 2. Cranelift 是成熟、活跃维护的后端，自己重写等价物代价高、收益低；
> 3. 这是 Rust、Go 等成熟语言的标准做法（bootstrap 层保留不可脱的最小实现）；
> 4. 项目自举闭环已跑通（`docs/pure_aura/03-自举验证报告.md`），bootstrap 的存在
>    不影响自举——只是自举产物里**包含** bootstrap。

> **不推荐**的路径：
> - 用「文本汇编 + 外部汇编器」（nasm/as/keystone）替换 Cranelift——JIT 延迟从毫秒级涨到
>   50–100ms，失去 JIT 价值；
> - 用 WAMR（WASM Micro Runtime，纯 C）做 JIT 后端——WASM 语义与 Aura OOP 语义差异大，
>   适配层复杂，且仍依赖 C 运行库；
> - 彻底放弃 JIT，只用 AOT + VM——丢失热点检测 + 渐进优化能力，且 AOT 冷启动慢。

---

## 三、文档索引

| # | 文档 | 内容 | 读者 |
|---|------|------|------|
| 01 | [01-现状分析.md](./01-现状分析.md) | 现有 JIT 三条路径、Aura 侧 8 文件能力盘点、bootstrap 边界、历史结论（Fix A/B） | 全员 |
| 02 | [02-技术方案.md](./02-技术方案.md) | 架构分层、数据流、ABI 契约、FFI 边界、W^X 装载策略、纯 Aura 侧职责 | 架构/后端开发者 |
| 03 | [03-分阶段开发计划.md](./03-分阶段开发计划.md) | S0–S5 六个阶段，每阶段含独立可验收的产出、依赖关系、工作量估算 | 开发者 |
| 04 | [04-测试与验收矩阵.md](./04-测试与验收矩阵.md) | 每阶段测试用例清单、不变量、回归检查清单、与自举链的联动 | QA / 开发者 |
| 05 | [05-风险与开放问题.md](./05-风险与开放问题.md) | 技术风险、ABI 漂移风险、回退策略、开放问题 | 架构/维护者 |

---

## 四、关键事实速查

### 4.1 JIT 三条路径（Rust 侧）

| 路径 | 技术 | 位置 | Feature | 默认 |
|------|------|------|---------|------|
| JIT | Cranelift 0.116 | `compiler/src/vm/jit.rs` (807 行) | `jit` | ❌ 关闭 |
| AOT | LLVM 23.1.0（文本 IR + 外部 llc/clang） | `compiler/src/codegen/aot/` | `llvm` | ❌ 关闭 |
| VM | 栈式字节码解释器 | `compiler/src/vm/interp.rs` | — | ✅ 默认 |

### 4.2 纯 Aura JIT 现有实现（Phase 7）

位置：`aura/compiler/aura/lang/compiler/jit/`，8 个文件，共 ~3440 行：

| 文件 | 行数 | 职责 | 对应 Rust |
|------|------|------|-----------|
| `JitCore.aura` | 607 | 字节码预解码、JitUnit 形态 | `bootstrap/jit_core.rs` |
| `JitState.aura` | 403 | 热点状态机、白名单、递归可达 | `vm/jit.rs` 状态机部分 |
| `JitUtil.aura` | 333 | 公共辅助（行/列解析、函数表） | — |
| `JitOpt.aura` | 728 | 7 个优化传递 | `vm/jit_opt.rs` |
| `JitLower.aura` | 462 | 字节码 → Cranelift 文本 IR | `vm/jit.rs` 的 `cranelift_backend` |
| `JitAbi.aura` | 291 | JitValue ABI 定义 | `vm/abi.rs` |
| `JitDispatch.aura` | 396 | 派发决策与解释器回退 | `vm/mod.rs` 热点接缝 |
| `JitRuntime.aura` | 221 | W^X 段加载描述（FFI 占位） | `vm/aot_runtime.rs` |

### 4.3 共享 ABI 契约

```text
JitValue = { tag: i64, payload: i64 }          // 16 字节，双字段
入口签名（VM/JIT/AOT 三路共用）：
  void entry(JitValue* args, JitValue* out, usize argc, void* dispatch_table)
```

类型标签（13 个）：`INT=0, FLOAT=1, BOOL=2, NULL=3, STR=4, PTR=5, OBJ=6,
FUNC=7, ARRAY=8, LIST=9, MAP=10, CLOSURE=11, CSTRING=12`

### 4.4 已实施的两条历史修复

| 修复 | 含义 | 覆盖根因 |
|------|------|----------|
| Fix A | 入口函数强制编译（忽略热点阈值） | 根因 ②④：入口不参与热点计数 + 循环热点天然失效 |
| Fix B | 白名单纳入 `Call`，沿调用图递归编译被调用者 | 根因 ③：递归热点被白名单拒绝 |

---

## 五、与其他文档的关系

- **`docs/pure_aura/02-纯Aura化改造方案.md`**：整体 5 阶段（A→E）路线，本文档是其中
  Phase 7（JIT Aura 化）的详细方案
- **`docs/pure_aura/03-自举验证报告.md`**：自举闭环已跑通的验证证据，是本文档 S0 阶段的前提
- **`docs/JIT性能分析.md`**：Fix A/B 修复前的性能分析，是本文档历史结论的来源
- **`docs/jit优化指南.md`**：JIT 优化策略参考（偏理论，与本文档落地方案互补）
- **`aura/compiler/README.md`**：纯 Aura 编译器整体说明，含 Phase 7 JIT 交付清单

---

## 六、文档维护约定

- 每个开发阶段完成后，**必须**更新 `03-分阶段开发计划.md` 中该阶段的「状态」字段
  （从「设计中」→「开发中」→「已交付」）
- 每个阶段新增的测试用例，必须同步登记到 `04-测试与验收矩阵.md`
- 阶段交付后，如有新的风险或开放问题，追加到 `05-风险与开放问题.md`
- 所有文档引用源码时使用绝对路径 + 行号范围（如 `compiler/src/vm/jit.rs:205-464`）

---

## 七、快速开始

**想快速理解全局**：读 §二「核心结论」 + `01-现状分析.md` 的 §一「三条路径」。

**想开始开发**：先读 `02-技术方案.md` 的 §三「数据流」+ §五「FFI 边界契约」，
再按 `03-分阶段开发计划.md` 从 S0 开始。

**想评审测试**：直接看 `04-测试与验收矩阵.md` 的「测试矩阵总览」表。

**想评估风险**：读 `05-风险与开放问题.md` 的 §一「风险分级表」。

---

*本文档为纯 Aura JIT 化的设计总纲。详细分阶段计划见 `03-分阶段开发计划.md`。*
