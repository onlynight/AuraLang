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

### 6. `bench-src/` — 五门基准源归档
`build/` 被 .gitignore ⇒ 五个基准源（micro1/2/3、fib28、loop1m）归档于此。
`vmopt.sh` 与 `bench-5way.sh` 启动时自动恢复缺失项（不覆盖本地改动）。

### 6b. `prof-src/` — 分离实验 guest 归档（2026-10-08 新增）
逐变量归因用的最小 guest（原先只存于 `build/prof2/`，随 `build/` 清理丢失）：
`intloop`（Int 累加，无 Long 转换）、`longstep`（每次 1 次 `as Long` + store，
量化装箱粒度）、`smallstep`（纯计数器，N=1.5e6）、`loop1100k`（1.1M 次，
>64 MiB 预算 → 触发上限）、`loop2m`、`strcat_oom`。生成 `.auc` 见
`docs/photon/vm-memory-comparison.md` §八。

### 6c. `auc-disasm.py` — `.auc` 反汇编器（2026-10-08 新增）
seed serialize v7 格式全解析（natives/常量/函数/操作码）：
```
python tools/photon-prof/auc-disasm.py <x.auc> --hist        # 按函数统计 opcode
python tools/photon-prof/auc-disasm.py <x.auc> --disasm main # 逐条反汇编
```
**P0-2c 的决定性工具**——正是靠它实测出「当前 `.auc` 里 `as Long` 编译为
`CALL_NATIVE_ARGS("aura_cast", 2)`、`BoxAlloc` 计数为 0」，推翻了
`vm-memory-comparison.md` §4.2 的旧归因。

### 7. `memprof.py` — 进程内存采样器（2026-10-07 新增）
```
<python> tools/photon-prof/memprof.py --runs 3 --label <标签> -- <命令...>
```
启动目标进程、5 ms 间隔采样，输出 peak_wset / peak_private / peak_vms。
配合 VM 内建 `AURA_VM_LOADONLY=1` 可分离「装载期」与「执行期」内存。

### 8. `mem-matrix.sh` — 内存对比矩阵
`micro1/fib28/loop1m` × {Rust VM, Photon Aura VM} + 纯编译对照。
**关键结论**（详见 `docs/photon/vm-memory-comparison.md`）：
- Photon 固定提交 66.6 MB = 64 MiB 静态堆 BSS（`.data` 段）；工作集仅 8.7 MB
- 执行期**线性增长**：`as Long` ≈62.5 B/次、函数调用 ≈35.6 B/次，**从不回收**
- **64 MiB 硬上限**：Long 操作 ~1.02M 次即耗尽 → **SIGSEGV**（N=1.0M 正常 / 1.1M 崩）
- 对照：Rust VM 同负载**零增长**（ARC + 空闲链表复用）

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
