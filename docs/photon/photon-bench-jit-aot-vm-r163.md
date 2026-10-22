# Photon AOT / JIT / VM 三方基准（R163，2026-10-22）

> 脚本：`scripts/photon/bench-jit-aot-vm.ps1`（每目标 3 次取中位）
> 驱动：`build/hat-native/PhotonHatCompile.exe` = `drv-r163`（含 R162 两条运行时修复）
> 结果 CSV：`build/jit-bench/results.csv`

## 一、原始结果（脚本口径）

| 目标 | 负载 | MedMs | PeakMB | 结果校验 |
|---|---|---|---|---|
| Photon-JIT | sum(1e9) | **250** | 3.8 | 499999999500000000 ✅ |
| Photon-JIT | fib(25) | **0**（<1ms，低于宿主 1ms 分辨率） | 3.8 | 75025 ✅ |
| Photon-AOT | sum(1e9) | **1059** | 4.2 | 499999999500000000 ✅ |
| Photon-AOT | fib(25) | **94** | 4.3 | 75025 ✅ |
| Aura-VM | sum(1e6) | **2610** | 3.8 | 499999500000 ✅（人工核对） |
| Aura-VM | fib(25) | **845** | 3.8 | 75025 ✅（人工核对） |
| Rust-VM（种子 VM） | sum(1e6) | **671** | 3.8 | ✅（含编译） |
| Rust-VM（种子 VM） | fib(25) | **327** | 4.4 | ✅（含编译） |

JIT 宿主自检：`现役 jit_vmhost 过门（sentinel），免重建` —— 三个 JIT 数字可信。

## 二、三个必须先说清的口径问题

### 1）本机**进程启动固定开销 ≈ 88.8 ms**

`build/_bench_probe.ps1` 构建了一个空负载 AOT exe（只 `Heap.init()` + 一次
`println`）并测 5 次中位：

| 二进制 | 中位墙钟 |
|---|---|
| Aura AOT 空负载 `probe0.exe` | **88.8 ms** |
| Aura AOT `fib.exe`（fib(25)） | 89.2 ms |
| Aura AOT `sum.exe`（sum(1e9)） | 1057.9 ms |
| **clang -O2 编译的 C `hello.exe`（对照）** | **89.7 ms** |

⇒ 88.8 ms **不是 Aura 运行时的开销**：一个纯 C 的 hello 也一样。
并且与 `AURA_HEAP_MB` 无关（16 / 64 / 256 / 1024 MB 都是 88–93 ms）。
它来自本机 CreateProcess + 控制台 + 实时防病毒扫描这一层。

**后果**：毫秒级负载在 AOT 上「测不到计算成本」——
`fib(25)` 名义 94 ms，扣掉基线后**计算只有约 0.4–5 ms**（与 JIT 同量级）。
JIT 目标的 250 ms 是**宿主内 `clockNowMs` 分项计时**（已经不含进程启动），
所以不能直接拿去和 AOT 的进程墙钟比。

### 2）sum 的负载规模不同

JIT / AOT 跑 `sum(1e9)`，两个 VM 跑 `sum(1e6)`（解释执行 1e9 不现实）。
跨目标只能比**每迭代成本**。

### 3）Rust-VM 的数字含编译

`aura run` 是「前端编译 → 字节码解释」。实测拆分（各 3 次中位）：

| | `run`（编译+解释） | `build`（只编译） | 解释部分 |
|---|---|---|---|
| sum(1e6) | 668 ms | 98 ms | **≈570 ms** |
| fib(25) | 303 ms | 99 ms | **≈204 ms** |

Aura-VM 用的是**预编译** `.auc`，不含编译。

## 三、扣除启动开销后的同口径对比

| 目标 | sum（每迭代） | fib(25)（计算部分） |
|---|---|---|
| **Photon-JIT** | 250 ms / 1e9 = **0.25 ns** | **< 1 ms** |
| **Photon-AOT** | (1059−89) / 1e9 = **0.97 ns** | ≈ **0.4–5 ms** |
| **Aura-VM** | (2610−89) / 1e6 = **2521 ns** | ≈ **756 ms** |
| **Rust-VM** | (671−89−98) / 1e6 = **484 ns** | ≈ **204 ms** |

倍数关系：

- **native vs VM（sum）**：AOT 比 Rust-VM 快 ≈ 500×，比 Aura-VM 快 ≈ 2600×。
- **JIT vs AOT（sum）**：AOT 是手编机器码的 **≈3.9×**（0.97 vs 0.25 ns/iter）。
  手编循环是 5 条指令、`add/inc/cmp/jcc` 单周期吞吐；AOT 版每个局部变量走栈槽，
  每迭代约 3 个周期 —— 这个差距是**代码生成质量**的真实体现（不是测量问题：
  sum 的 1e9 迭代已彻底盖过启动开销）。
- **Aura-VM vs Rust-VM**：Aura VM 的逐指令分派比 Rust 字节码解释器慢 **≈5×**
  （2521 vs 484 ns/iter；fib 756 vs 204 ms 也是 ≈3.7×）。
- **fib 上 native 与 JIT 同量级**（都 <1 ms）—— 之前的「AOT 94 ms vs JIT 0 ms」
  是启动开销造成的假差距，**不能**据此判断递归调用生成质量。

## 四、内存（峰值工作集）

四个目标都在 **3.7–4.4 MB**，与负载规模无关（语言运行时基线）：
JIT 3.8、AOT 4.2–4.3、Aura-VM 3.7–3.8、Rust-VM 3.8–4.4 MB。

⚠️ 注意 Aura-VM 宿主 `build/hat-vmhost/current.exe` **文件 67 MB**，但峰值 WS
只有 3.8 MB —— 说明它绝大部分是未被触碰的映射页（勿把文件大小当常驻）。

## 五、本轮顺带修掉的脚本 bug

`scripts/photon/bench-jit-aot-vm.ps1` 的 `Run-WithPeak` 用
`$psi.EnvironmentVariables[$k] = $v` 注入环境变量，在 PS 5.1 下该属性为 `$null`
⇒ 抛「无法对 Null 数组建立索引」⇒ **`AURA_BENCH=1` 根本没传给 `jit_vmhost`**。
后果：
- 宿主走非基准路径，不打印 `[BENCH-*]` 标记 ⇒ **JIT 两个目标的 MedMs 恒为 -1**；
- 更隐蔽的是「自检 + 哨兵门」也被判失败，脚本报
  `jit_vmhost 自检+哨兵均未通过（P5-① 布局漂移）` —— **把脚本自身缺陷误诊成
  发射器布局漂移**。

改为「进程级 `[Environment]::SetEnvironmentVariable` + 跑完恢复」后：
`现役 jit_vmhost 过门（sentinel），免重建`，JIT 两个目标出数（250 ms / <1 ms）。

## 六、复现命令

```powershell
# 三方基准（每目标 3 次取中位，写 build/jit-bench/results.csv）
powershell -File scripts\photon\bench-jit-aot-vm.ps1

# AOT 固定启动开销基线（空负载 vs fib vs sum，含 C hello 对照）
powershell -File build\_bench_probe.ps1
```
