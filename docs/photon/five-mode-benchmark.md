# 五模式基准对比（2026-10-07）

> 脚本：`scripts/photon/bench-5way.sh [轮数]`（默认 5 轮取 min）
> 斜率实验：`scripts/photon/bench-aot-slope.sh [N1] [N2] [轮数]`
> 基准：`bench_fib28`（fib(28) 递归）、`bench_loop1m`（100 万次 Long 累加）
> 五种模式跑**同一份 Aura 源码**，五条完全不同的执行链。

---

## 一、五种模式定义

| # | 名称 | 执行链 |
|---|---|---|
| ① | LLVM IR AOT | 种子 Rust 编译器 → `.ll` → `llc -O2` → `clang` → 原生 exe（369 KB） |
| ② | Rust VM | 种子的 Rust 字节码解释器（`aura run`） |
| ③ | Aura VM（seed 构建） | `auc_vm_runner.exe` + `build/vm/current.auc`；VM 自身由**种子 LLVM-AOT** 编译 |
| ④ | Aura VM（Photon 构建） | `build/hat-vmhost/current.exe` + **同一份** `.auc`；VM 自身由 **Photon 后端**编译 |
| ⑤ | **Photon AOT** | **PhotonHatCompile → HAT → 自研 X86 后端 → COFF → lld-link → 原生 exe**（67 MB） |

⑤ 的输入通道是固定路径约定：调用方把入口源拷到 `build/hat-vmhost/current.aura`，
驱动读它、把两个 COFF 以 hex 打到 stdout，再由调用方 `lld-link` 链接。
⚠ 驱动**自身也会落盘** `build/hat-vmhost/current.exe` ⇒ 跑 ⑤ 会覆盖宿主，
`bench-5way.sh` 内已做自动备份/恢复（判据 = 入口源首行特征，**不能用文件大小**
——宿主入口源仅 1398 B 的薄入口，bench 源 200~600 B）。

---

## 二、主表（5 轮取 min）

| 基准 | ① LLVM-AOT | ② Rust VM | ③ AuraVM(seed) | ④ AuraVM(Photon) | ⑤ PhotonAOT |
|---|---|---|---|---|---|
| bench_fib28 | **641 ms** | 1535 ms | 3081 ms | 3751 ms | **646 ms** |
| bench_loop1m | **641 ms** | 1907 ms | 3484 ms | 5893 ms | **643 ms** |

**派生指标**

| 基准 | ④vs③ | ④vs 会话起点 | ⑤vs① |
|---|---|---|---|
| bench_fib28 | −21.7% | **+27.3%** | −0.8% |
| bench_loop1m | −69.1% | **+24.6%** | −0.3% |

- **④vs③**：同一份字节码下，Photon 构建的 VM 相对 seed 构建的 VM（负数 = 更慢）。
- **④vs 会话起点**：本会话 P409→当前 在 VM 上的净收益（同一 Photon 宿主）。
- **⑤vs①**：两个 AOT 后端的原生代码差距 —— ⚠ **在小基准上无意义**，原因见下节。

---

## 三、关键发现 1：小基准区分不了 AOT 后端（启动开销 ~0.65 s）

实测**空程序**（只有一句 `println`）的 AOT 产物启动耗时：

| 产物 | 二进制 | 空程序启动 |
|---|---|---|
| LLVM-AOT | 369 KB | 736 ms |
| Photon AOT | 67 MB | ~650 ms（同量级） |

而 fib(28) 的**纯计算**在原生码下只有几毫秒量级 ⇒ 主表里 ①⑤ 的 641/646 ms
**几乎全部是进程启动**，`⑤vs① = −0.8%` 这个「Photon 比 LLVM 快」的结论
是噪声，**不可采信**。

要测后端真实差距，必须消掉启动开销 → 见下节斜率实验。

---

## 四、关键发现 2：AOT 后端真实差距 ≈ **2.9×**（斜率实验）

方法：同一套循环源，两个规模档 N1 / N2，两侧各编译原生 exe，
`(T_N2 − T_N1)` 即纯计算时间（启动开销被相减消掉）。

**源**（与 `bench_loop1m` 同构，只放大 N）：

```
fun main() {
    var i: Int = 0
    var acc: Long = 0
    while (i < N) {
        acc = acc + (i * 2 - 1) as Long
        i = i + 1
    }
    println("loop acc = " + toStr(acc))
}
```

**档位**：N1 = 6e8、N2 = 1e9（`i*2` 上限受 Int32 约束：N ≤ 1.07e9）。

| 后端 | N1=600M | N2=1000M | 斜率（400M 次迭代） | 单次迭代 |
|---|---|---|---|---|
| LLVM -O2 | 1015 ms | 1237 ms | **222 ms** | **0.56 ns** |
| Photon | 1678 ms | 2333 ms | **655 ms** | **1.64 ns** |

> **⇒ Photon 自研 X86 后端生成的循环代码 ≈ LLVM -O2 的 1/2.95 速（慢 2.95×）。**

（独立复测一轮：LLVM 228 ms / 0.57 ns，Photon 668 ms / 1.67 ns，比 2.93×
—— 两次一致，数据稳定。）

这是**第一次把「Photon 后端 vs LLVM -O2」的代码质量差量化**。
它与此前 VM 层观测互相印证：`④vs③` 在 loop1M 上 −69%，量级一致
（VM 宿主代码形态与纯累加循环不同，故倍数不同）。

**根因指向**（与 P412/P414 分析一致）：
1. 寄存器着色仍被大面积限制（P414e 只放开「块内、不跨调用、非 Const」）
   ⇒ 31~48% 的指令是栈往返，形成串行 store-forwarding 依赖链，IPC ≈ 1~2；
2. 每函数固定 8 push 序言 + 帧超配（`+2048` blanket 边际）；
3. LLVM 侧 -O2 会做循环向量化/强度削减，Photon 侧无等价优化。

**优化空间估计**：若 Photon 后端能逼近 LLVM -O2 的循环代码质量，
纯计算场景还有 ~2.9× 的空间 —— 这正是 P1b（寄存器分配）继续放开的
收益上限所在。

---

## 五、意外发现：Int 溢出语义两端不一致（独立 bug）

做斜率实验时，首轮用了 N2 = 1.2e9，暴露语义分歧：

| 后端 | N=1.2e9 输出 | 说明 |
|---|---|---|
| Photon AOT | `1439999997600000000` | = N²−2N 数学精确值 |
| LLVM-AOT | `897725265122355200` | `i*2` 在 Int32 下回绕（i ≥ 2^30 时） |

`i * 2` 在 i > 1073741823 时超出 Int32 ⇒ 两端对溢出的处理不同
（LLVM 侧回绕、Photon 侧得正确值）。**这是一个待定案的语义 bug**
（需明确 Aura 的 Int 溢出语义：wrapping 还是提升），本轮仅记录，
斜率实验已改用 N ≤ 1e9 规避（两端输出一致，见校验）。

**校验**（600M / 1000M 两端输出均 = N²−2N）：
- 600M: `359999998800000000` ✓
- 1000M: `999999998000000000` ✓

---

## 六、注意事项（读表前必看）

1. **所有模式都含 ~0.65 s 进程启动开销**（空程序 AOT 亦然）。主表中
   ①②③④⑤ 一律未扣启动；③④ 的 .auc 装载也含在内。
2. **③ 的源码快照早于本会话优化**（10-05 构建）⇒ `④vs③` 对 ④ **偏保守**。
3. 表中数字为**取 min**（CPU 密集任务最稳健的口径），非中位数。
4. ⑤ 每次编译约 8~17 s（Photon 管线）；① 约 10 s。
5. 运行 ⑤ 会覆盖宿主产物 —— 用 `bench-5way.sh` 而非手工操作。

---

## 七、可复现

```bash
# 主表（五模式 ×2 基准，默认 5 轮取 min；含宿主自动保护）
scripts/photon/bench-5way.sh 5

# AOT 后端斜率（消启动开销；默认 600M/1000M）
scripts/photon/bench-aot-slope.sh

# 单独看 ④ 的会话净收益（五门回归 + 计时）
scripts/photon/vmopt.sh regress
scripts/photon/vmopt.sh bench
```
