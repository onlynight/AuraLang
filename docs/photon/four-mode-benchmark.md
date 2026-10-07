# 四模式基准对比（2026-10-07）

> 脚本：`scripts/photon/bench-4way.sh [轮数]`（默认 5 轮取 min）
> 基准：`bench_fib28`（fib(28) 递归）、`bench_loop1m`（5000×5000 双循环）
> 四种模式跑**同一份 Aura 源码**，四条完全不同的执行链。

---

## 一、结果

| 基准 | ① LLVM-AOT | ② Rust VM | ③ AuraVM(seed) | ④ AuraVM(Photon) | ④vs③ | ④vs会话起点 |
|---|---|---|---|---|---|---|
| bench_fib28 | **670 ms** | 1659 ms | 3186 ms | 3962 ms | −24.4% | **26.1%** |
| bench_loop1m | **633 ms** | 1889 ms | 3504 ms | 5999 ms | −71.2% | **23.3%** |

**模式定义**

| # | 名称 | 执行链 |
|---|---|---|
| ① | LLVM IR AOT | 种子 Rust 编译器 → `.ll` → `llc -O2` → `clang` 链接 → 原生 exe |
| ② | Rust VM | 种子的 Rust 字节码解释器（`aura run`），含 ~0.6 s 固定启动 |
| ③ | Aura VM（seed 构建） | `auc_vm_runner.exe` 加载 `build/vm/current.auc`；VM 自身由**种子 LLVM-AOT** 编译 |
| ④ | Aura VM（Photon 构建） | `build/hat-vmhost/current.exe` 加载**同一份** `.auc`；VM 自身由 **Photon 后端**编译 |

③ 与 ④ 的源是**同一个文件**（`examples/compiler/auc_vm_runner.aura`，
`build/hat-vmhost/current.aura` 是它的副本），跑**完全相同的字节码**，
装载逻辑同源 —— 差异纯粹来自「用哪个后端编译 VM 自身」。

启动开销已单独隔离：`micro1`（极小 guest）实测 ③=659 ms、④=654 ms，
**两者相同**，故上表的差异全部是执行速度。

---

## 二、两个结论

### 结论 1：本会话优化确实有效 —— ④ 比会话起点快 23~26%

（会话起点 = P409 宿主，同一 Photon 宿主二进制，仅差本会话的
P412 分派链重排 / P413 冗余栈往返消除 / P414+P414e 寄存器着色放开。）

这与会话内交替 A/B 测得的一致（fib28 28.2%、loop1M 22.6%）。

### 结论 2（更重要）：**Photon 后端与 LLVM-AOT 编译同一个 VM，代码质量仍有 24~71% 差距**

③ 用的是 2026-10-05 的源码快照（**早于**本会话优化），④ 含本会话全部优化，
却仍然慢 24.4%（fib28）/ 71.2%（loop1M）。

⇒ 把「源码版本差」算进去后，**真实的后续差距只会更大**：
即便 ③ 也用当前源码重建，它只会更快。

**这正是 Photon 后端存在的意义与当前的短板所在**：
本次对比第一次把「Photon 生成的代码」与「LLVM 生成的代码」放在
*同源、同字节码、同启动开销* 的条件下量化 —— **24~71% 是后端差距**。

`loop1M` 的 71% 差距明显大于 `fib28` 的 24%，与 P412 分析的结论吻合：
`loop1M` 是纯算术密集（53M 条指令，LOAD/STORE_LOCAL 占 72%），
最吃「栈往返 + 低 IPC」；`fib28` 调用密集，部分被调用路径的开销掩盖。

---

## 三、注意事项（读表前必看）

1. **③ 的源码快照早于本会话优化**（10-05 构建）。这使 `④vs③` 对 ④ **偏保守**
   —— 用旧源码的 ③ 当基准已不利于 ④。若要严格同源，需用当前源码重建 ③，
   当前受阻于 seed 词法器对某个导入文件报 `lex: Unexpected character: '\'`
   （`examples/compiler/auc_vm_runner.aura` 及其直接导入均无 CR/反斜杠，
   待定位闭包内的具体文件）。

2. **① 与 ②③④ 的计时口径不同**：① 是纯原生执行（无 VM 装载）；
   ②③④ 均含 ~0.6 s 进程启动 + `.auc` 装载。扣掉启动后，
   `bench_loop1m` 的实际执行时间约为 ①0ms级 / ②1.3s / ③2.9s / ④5.4s。

3. 表中数字为**取 min**（CPU 密集任务最稳健的口径），非中位数。

---

## 四、可复现

```bash
# 完整四模式对比（默认 5 轮）
scripts/photon/bench-4way.sh 5

# 单独看 ④ 的会话净收益（五门回归 + 计时）
scripts/photon/vmopt.sh regress
scripts/photon/vmopt.sh bench
```
