# ADR-004：bootstrap 隔离守卫与依赖方向声明

- **状态**：已接受（2026-10-01）
- **关联**：VM-PA-00 v3.1 §2.5 / P0.1；决策 D1

## 背景

`seed/compiler/src/bootstrap/`（3,047 行 / 10 文件）经全仓库 grep 核实为**孤立死
代码**：0 生产引用，唯一使用者是 `tests/bootstrap_test.rs`。决策 D1：保留不删除，
但新 VM 与 JIT 任务不得引用。事实状态会漂移（尤其 `jit_ffi.rs` 与 JIT 需求高度
重叠，最易被接回），因此建立机制性守卫。

## 决策

### 1. 依赖方向声明（单向性）

`bootstrap/` 是**叶子模块**：

- ✅ 允许引用它的：`tests/`（`bootstrap_test.rs`）；
- ❌ 禁止引用它的：`seed/compiler/src/vm/`、`seed/compiler/src/codegen/`、CLI、
  loom 等一切生产代码；`aura/compiler/` 全部 Aura 代码；
- 🟡 保留的例外：`seed/compiler/src/lib.rs` 的唯一挂载行
  `pub mod bootstrap;`（D1 要求保留现状，不清理）。

### 2. 静态守卫（强制）

`scripts/check-bootstrap-isolation.ps1`（及 `.sh` 版）断言四条约束：

1. `aura/compiler/**/*.aura`：**非注释行**不得出现 `bootstrap`（注释提及允许）；
2. `seed/compiler/src/vm/**`：任何 `bootstrap` 命中即失败（0 命中）；
3. `seed/compiler/src/`（bootstrap/ 自身除外）：仅注释命中 + lib.rs 挂载行；
4. `bootstrap/mod.rs` 头部必须含 `ISOLATED … DO NOT REFERENCE` 隔离标记。

任一违反即退出码 1。CI/回归流程在编译前调用本脚本。

### 3. 物理标记

`bootstrap/mod.rs` 头部已加显著标记（2026-10-01），声明本层定位与禁用范围。

## 后果

- JIT/Photon 任务启动前守卫已就位（VM-PA-00 R6 的缓解前提）；
- 误接线在 CI 阶段失败，而非运行期才暴露；
- bootstrap 的存在不再对读者构成"这是活代码"的误导。
