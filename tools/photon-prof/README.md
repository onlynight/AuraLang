# photon-prof — Photon Aura VM 成本测量工具

> 建立于 2026-10-07（P412）。**用测量替代推测**——此前三轮的源码推测模型
> 连错两次（§10.114 把成本算在类型分派上；实际 type dispatch 只值 1%）。

## 三个工具

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
`{build|regress|bench|regress <exe>}`。详见脚本头部注释。

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
