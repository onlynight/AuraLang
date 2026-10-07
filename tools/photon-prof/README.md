# photon-prof — Photon Aura VM 成本测量工具

> 建立于 2026-10-07（P412）。**用测量替代推测**——此前三轮的源码推测模型
> 连错两次（§10.114 把成本算在类型分派上；实际 type dispatch 只值 1%）。

## 工具

### 1. `Vm_instrumented.aura` — per-opcode 动态计数器
把此文件覆盖到 `aura/compiler/aura/lang/compiler/vm/Vm.aura`，重建宿主后：
```
AURA_OP_PROF=1 ./build/hat-vmhost/current.exe
```
输出 `[OPPROF] <op> <mnemonic> <count>`（单趟按槽序）+ `[OPPROF-TOTAL]`。
排序用外部 `sort -t' ' -k4 -rn`。

**原理**：256 槽 Memory 表（索引 op+3，容纳 OP_NOP=-2 / OP_UNKNOWN=-1），
`run()` 循环内每条指令一次 RMW。⚠️ **纯诊断，不得进入任何计时口径**
（每条指令多一次 Memory 读写 + 布局扰动）。

### 2. `measure.py` — per-opcode 边际成本
```
python3 tools/photon-prof/measure.py
```
用 **N / 2N 双档斜率**消除进程启动与装载开销；**宿主二进制固定、
只换客体 .auc**（客体是指令数据、不是宿主代码）⇒ 无宿主布局噪声。
输出 `build/prof/result.json`（含每档绝对计数与斜率）。

⚠️ 已知局限：Aura 编译器会**内联平凡函数**（g3 的 `f(i)` 被内联，
profile 里 CALL=0），所以无法用此法隔离 CALL 成本——需用递归 guest。

### 3. `vmopt.sh` — 五门回归闭环
`{build|driver|regress|bench|all|full}`。改**发射器侧**代码必须用 `full`
（含驱动重建；`build` 不重建驱动 = 宿主仍用旧发射器）。详见脚本头部注释。

### 4. `bench-5way.sh` — 五模式基准主表
```
scripts/photon/bench-5way.sh [轮数]
```
同一份 Aura 源码 × 五条执行链：① LLVM-AOT ② Rust VM ③ AuraVM(seed)
④ AuraVM(Photon) ⑤ **Photon AOT**。
⚠ ⑤ 会覆盖宿主产物（驱动输入是固定路径且自身落盘 exe）——脚本内自动
备份/恢复（判据=入口源首行特征，**不能用大小**：宿主入口源仅 1398B）。

### 5. `bench-aot-slope.sh` — AOT 后端斜率（消启动开销）
```
scripts/photon/bench-aot-slope.sh [N1] [N2] [轮数]     # 默认 600M/1000M
```
小基准的纯计算（几 ms）远小于进程启动（~0.65s）⇒ **区分不了 AOT 后端**。
用两档 N 的斜率消掉启动开销，实测 **Photon 后端 ≈ LLVM-O2 的 1/2.95 速**。
⚠ N ≤ 1.07e9（`i*2` 的 Int32 约束，见 five-mode-benchmark.md §五）。

### 6. `bench-src/` — 基准源归档
`build/` 被 .gitignore ⇒ 五个基准源（micro1/2/3、fib28、loop1m）归档于此。
`vmopt.sh` 与 `bench-5way.sh` 启动时自动恢复缺失项（不覆盖本地改动）。

## 关键测量结论（2026-10-07）

**动态指令分布（两基准一致）**：
| op | fib28 31.37M | loop1M 53.00M |
|---|---|---|
| LOAD_LOCAL | 41.0% | 39.6% |
| STORE_LOCAL | 26.2% | 32.1% |
| LOAD_CONST_I | 6.6% | 7.5% |
| RETAIN | 6.6% | 3.8% |
| 其余 | 各 ≤3.3% | 各 ≤3.8% |

⇒ **LOAD/STORE_LOCAL 合计 67~72%**——优化的正确靶子。

**每条 `if (op == N)` = 11 条 x86**（常量落栈→读回→cmp→setcc→movzbq→
落栈→读回→cmp 0→jcc）。命中第 d 臂付 11×d 条。

**函数帧普遍 2.4~2.9KB**（来自 `X86Emitter` 的 `frame = 144+range*8+2048`
blanket 边际），序言固定 **8 push**（7 callee-saved + rbp）。
dispatch 帧 18KB（5 次栈探测）、callOrBuiltin 帧 24KB（6 次探测）、
代码 53KB。

**栈往返占生成指令的 31~48%**；其中 **961 对是相邻的「存栈→立即读回
同槽」纯 no-op**（占静态指令 5.1%）。

## 五模式基准（2026-10-07 追加）

| 基准 | ① LLVM-AOT | ② Rust VM | ③ AuraVM(seed) | ④ AuraVM(Photon) | ⑤ PhotonAOT |
|---|---|---|---|---|---|
| fib28  | 641ms | 1535ms | 3081ms | 3751ms | 646ms |
| loop1m | 641ms | 1907ms | 3484ms | 5893ms | 643ms |

- **④vs③**：同字节码，Photon 构建的 VM 比 seed 构建的慢 −21.7%/−69.1%
  （③ 源码快照偏旧，对 ④ 是**保守**估计）；**④vs 会话起点** +27.3%/+24.6%。
- **小基准区分不了 AOT 后端**：空程序 AOT 启动就 0.65~0.74s，fib(28)
  纯计算仅几 ms ⇒ ①⑤ 的 641/646ms 几乎全是启动。
- **AOT 后端斜率**（600M→1000M 消启动开销）：
  LLVM-O2 222ms / 0.56ns-per-iter，Photon 655ms / 1.64ns-per-iter
  ⇒ **Photon 后端 ≈ LLVM-O2 的 1/2.95 速**（复测 2.93× 一致）。
- **意外发现（独立 bug，待定案）**：Int 溢出语义两端不一致——
  N=1.2e9 时 `i*2` 超 Int32：LLVM 版回绕、Photon 版得精确值。

完整报告：`docs/photon/five-mode-benchmark.md`。
