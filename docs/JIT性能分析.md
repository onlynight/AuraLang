# JIT 性能分析报告（P5 遗留问题）

> 日期：2026-09 · 结论：当前 JIT 模式性能与纯解释器（VM）完全相同（≈1.0x），
> 根本原因已通过运行时诊断确认，非基准测量误差。

## 1. 现象

`benches/aot_benchmarks.rs` 与 `examples/aot_bench.rs` 测量（release 模式）：

| 场景 | VM（解释器） | JIT（`--jit`） | AOT（LLVM） |
|------|-------------|---------------|------------|
| fib(25) | 226-229 ms/op | 228-229 ms/op（1.0x） | 5.1-5.4 ms/op（**44x**） |
| sum(60000) | 54-55 ms/op | 54-55 ms/op（1.0x） | 4.1-4.4 ms/op（**13x**） |

JIT 与 VM 的耗时完全相同，说明 **JIT 从未真正派发原生码**，所有执行都回退到了解释器。

## 2. 运行证据

`examples/jit_diag.rs`（`Vm::jit_state()` 诊断输出）：

```
--- A: sum(60000) 循环在 main 内 ---
  fn#0 main  调用=1      ❄️未达阈值(解释器)
--- A': 同上但 run 20 次 ---
  fn#0 main  调用=20     ❄️未达阈值(解释器)
--- B: fib(25) 递归热点 ---
  fn#0 fib  调用=242785 ⛔已跳过(回退解释器)
  fn#1 main  调用=1      ❄️未达阈值(解释器)
--- C: add 叶子函数被调用 100k 次 ---
  fn#0 add  调用=0      ❄️未达阈值(解释器)
  fn#1 main  调用=1      ❄️未达阈值(解释器)
```

## 3. 根因链（三层叠加）

### 3.1 编译器内联提前消灭了函数调用（案例 C 实证）

`codegen/mod.rs` 的 `compile()` 在编译期执行 `inline_hir()`（HIR 内联展开）。
小函数（如 `add`）的调用点在字节码中被替换为内联指令序列。

反汇编证实（案例 C 的 `.auc`）：main 中 **无任何 `Call add` 指令**，
`add(s, i)` 已被展开为内联 `ADD`。

后果：运行时没有 `Call` → 热点计数（`call_counts`）永远不增加 →
`maybe_jit_compile` 永远不会被这些函数触发。**JIT 的检测对象在编译期就被删除了。**

### 3.2 入口函数（main）从不参与热点检测（案例 A 实证）

`Vm::run()` 直接 `push_frame(entry)` 启动入口函数，入口函数的调用计数
每次 `run()` 只 +1。热点循环（如 sum）位于 main **内部**，循环体不产生
函数调用，因此不存在任何 `Call` 指令来触发 `maybe_jit_compile`。

即使反复 `run()` 20 次，计数也只有 20 ≪ 阈值 10_000 → 永远不编译。

对 `run()` 入口前置一次 `maybe_jit_compile(entry)` 也无法解决：
「循环热点」不产生调用计数，基于调用频率的检测对本场景天然失效。

### 3.3 唯一能达阈值的递归热点被 JIT 白名单拒绝（案例 B 实证）

fib(25) 单次运行中递归调用 fib 达 **242,785 次** ≫ 10_000 阈值，
成功触发 `maybe_jit_compile(fib)`。

但 `vm/jit.rs::is_jit_compilable()` 白名单**排除含 `Call` 的函数**
（「叶子整数函数」策略，见 jit.rs 头注释 §编译范围）。递归函数必然含
`Call`（调用自身），因此被记入 skip 集合，**永久回退解释器**。

## 4. 结论

**JIT 四点机制互相抵消，导致热点永远不可达编译：**

```
内联优化删除小函数调用  ─┐
入口函数无调用计数        ├─→ 没有任何函数能同时满足：
递归热点被白名单拒绝    ─┘     ① 调用次数 ≥ 10000
                             ② is_jit_compilable == true
```

而 AOT（LLVM）完全没有这些问题：它不依赖运行时热点检测，
直接将 HIR 编译为原生码，因此获得 44x / 13x 加速。

## 5. 修复方向（P5 后续任务，按收益排序）

| 方案 | 做法 | 解决 | 风险 |
|------|------|------|------|
| A. 入口函数整体编译 | 对 main 在首次运行时直接 Cranelift 编译（忽略调用阈值），循环热点走原生码 | 3.2 | 中（需验证 ABI/栈） |
| B. 放宽白名单支持递归 | 允许含「自递归或无间接环」Call 的函数编译 | 3.3 | 高（需完整调用 ABI） |
| C. 内联后整体编译 | 既然内联已展开小函数，直接编译内联后的 main | 3.1 | 中 |
| D. 关闭内联保留调用点 | `CodeGenOptions.optimize=false` 时保留真实 Call | 3.1 | 低（但会损失优化） |

> 推荐路径：先实施 A（入口整体编译）获得循环热点收益；
> 再评估 B（递归支持）覆盖 fib 类计算密集型递归。

## 6. 相关文件

- `aura-compiler/src/vm/jit.rs` — Cranelift JIT（白名单、编译、派发）
- `aura-compiler/src/vm/mod.rs` — 热点计数 / `maybe_jit_compile` / `run`
- `aura-compiler/src/codegen/opt.rs` — `inline_hir` 内联展开
- `aura-compiler/examples/jit_diag.rs` — 状态诊断工具
- `aura-compiler/examples/aot_bench.rs` — 三路性能对比