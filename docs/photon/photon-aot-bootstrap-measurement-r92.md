# Photon AOT 编译 Aura 自举编译器 —— 耗时与内存实测

> 测量时间：2026-10-19 20:48–20:52
> 被测：`D:/Code/AuraLang` 工作区（HEAD `3a274a4` + 未提交 R90/R91/R92 改动）
> 结论：**当前 R92 工具链未能完成自举编译**——3m24.8s 后撞上内置 8 GiB 内存记账上限中止，
> 此时进程私有内存已达 **20.71 GB**，且仍以 ~105 MB/s 线性增长。

---

## 一、结论先行

| 指标 | 数值 |
|---|---|
| 墙钟耗时 | **3m24.837s**（204.84 s） |
| 结果 | **失败**：`exit 70`，无 `.hat` / `.obj` / `.exe` |
| 中止原因 | `[aura] memory limit exceeded: used ~8191 MiB … limit 8192 MiB`（驱动内置 AURA_MEM_LIMIT_MB=8192） |
| 峰值工作集 | **19.75 GB**（21,206,327,296 B）@ t=203.4 s |
| 峰值私有内存 | **20.71 GB**（22,238,883,840 B）@ t=203.4 s |
| 死亡位置 | 前端模块链接 **107/114**（`JitDispatch.aura` 开始后未完成），HIR 节点累计 230,483 |
| 内存曲线形态 | **近似完美线性** ~105 MB/s；2140 个 50 ms 样本中仅 14 个微小负增量 ⇒ **全程无回收** |
| 与历史基线对比 | 基线（旧源码树）146.76 s / 5,286 MB 完成；本轮**内存至少 4× 且未跑完** |

**一句话**：R90 的「scratch 逐模块回收」在当前驱动里实质失效——内存不是"用得多了点"，
而是**线性泄漏**；同时 8 GiB 记账上限只是一个提前触发的闸门（实际已提交 20.7 GB，约为记账值的 2.5×）。

---

## 二、测量环境与方法

| 项 | 值 |
|---|---|
| 输入 | `aura/compiler/aura/lang/compiler/Main.aura`（36,383 B，自举编译器入口） |
| 驱动 | `build/hat-native/PhotonHatCompile.exe` 1,760,768 B，2026-10-19 20:46 重建 |
| 工具链状态 | 测量前按标准流程**全量重建**：seed 2m22s（Rust `--features llvm`，纳入未提交的 emit.rs/linker.rs/native.rs/aura_std_cffi.c 改动）→ 驱动 31s（纳入 Aot.aura/PhotonPipeline.aura R91/R92 改动） |
| 备份 | 旧驱动 `PhotonHatCompile.exe.20261018.bak`、旧 seed `aura.exe.20261018.bak` |
| 机器 | Windows x64，物理内存 **31.8 GB**（测量时空闲 20.9 GB） |
| 采样 | `WorkingSet64` / `PrivateMemorySize64`，**50 ms** 间隔，共 2141 个样本 |
| 计时 | 进程墙钟（Stopwatch）＋驱动内置 `AURA_PHOTON_TIME=1` 分段 |
| 脚本 | `scripts/photon/measure-bootstrap.ps1`（本轮新增，异步双管道读避免死锁） |
| 产物目录 | `build/hat-bootstrap-r92-run1/`（`summary.json` / `timeline.csv` / `run-stdout.log` / `run-stderr.log`） |

> 说明：内置分段计时 `[time]` 行**没有输出**——驱动在前端阶段就中止，未到达
> `PhotonHatCompile.aura` 里打印 `[time] link=…` 的位置。因此本轮只有"到死为止"的总耗时。

冒烟验证（重建后的驱动）：`tests/photon/P1/01_hello_world.aura` → 编译→链接→运行输出
`Hello, World!`、exit 0，产物 `01_hello_world.exe` 54,272 B。**小输入完全正常**，故障只在自举规模上出现。

---

## 三、主测结果（R92 当前树）

### 3.1 内存曲线（私有内存，每 ~10 s 取样）

| t (s) | 工作集 (MB) | 私有 (MB) | 说明 |
|---:|---:|---:|---|
| 9.7 | 1,204.6 | 1,267.0 | 已超基线 link 阶段峰值(695 MB) |
| 19.7 | 2,538.9 | 2,682.4 | |
| 29.6 | 3,698.1 | 3,904.5 | |
| 39.5 | 4,715.0 | 4,979.1 | |
| 49.3 | 5,759.2 | 6,077.6 | 已超基线**全链路**峰值(5,286 MB) |
| 58.7 | 6,683.9 | 7,060.0 | |
| 68.5 | 7,720.7 | 8,147.2 | 记账上限(8 GiB)附近——但实际已 8 GB+ |
| 78.3 | 8,661.8 | 9,133.0 | |
| 88.3 | 9,466.9 | 9,975.2 | |
| 98.0 | 10,345.1 | 10,889.4 | |
| 107.3 | 11,128.9 | 11,709.6 | |
| 116.7 | 11,892.8 | 12,510.0 | |
| 126.0 | 12,664.2 | 13,317.6 | |
| 135.4 | 13,448.5 | 14,137.9 | |
| 144.6 | 14,586.0 | 15,322.5 | 单段跳升 +1,137 MB/9.2 s（大模块） |
| 153.8 | 15,789.3 | 16,574.6 | |
| 163.2 | 16,980.1 | 17,813.6 | |
| 172.5 | 17,759.5 | 18,627.4 | |
| 181.9 | 18,529.8 | 19,435.1 | |
| 191.3 | 19,285.3 | 20,223.8 | |
| 200.9 | 20,052.6 | 21,028.0 | |
| 204.8 | 925.2 | 972.0 | 进程中止，OS 回收 |

平均增速 **≈105 MB/s**，斜率全程不衰减；样本间最大跳变仅 38.7 MB（t=11.6 s），
**没有任何"模块结束→内存回落"的锯齿**——这正是"回收从未生效"的直接证据。

### 3.2 中止点

```
stderr:
[aura] memory limit exceeded: used ~8191 MiB, requesting ~1 MiB more, limit 8192 MiB.
[aura] aborted to avoid exhausting physical memory. Set AURA_MEM_LIMIT_MB (MiB; 0 or off = unlimited) to change the limit.
```

标准输出最后三行：

```
[mod-start] aura/compiler/aura/lang/compiler/jit/JitOpt.aura
[mod-start] aura/compiler/aura/lang/compiler/jit/JitLower.aura
[mod-start] aura/compiler/aura/lang/compiler/jit/JitDispatch.aura     ← 死在这里（第 107 个）
[mod] nodes=230483 aura/compiler/aura/lang/compiler/jit/JitLower.aura ← 最后一个完成
```

- 107 个 `[mod-start]`，仅 105 个 `[mod]` 完成 ⇒ 前端（link）阶段尚未走完。
- 死时 HIR 节点 230,483（历史基线整棵树才 180,810，当前源码树已显著变大）。

### 3.3 为什么没跑完也要记录

用户诉求是「编译自举编译器的耗时和内存」。当前工具链的**真实测量值**就是：
**3m24.8s 内烧掉 20.7 GB 私有内存仍未完成**。这不是噪声或偶发——是稳定可复现的
线性泄漏（见 §五根因），换机器只会推迟撞墙时间，不会改变结论。

---

## 四、A/B 对照（10-18 备份驱动，未完成）

为区分"源码树变大"与"工具链退化"，用同一输入、同一脚本跑了 10-18 备份驱动
（其内嵌 CFFI 处于 `[BISECT] scratch 关闭` 状态——字符串临时物全部落 retain）：

| 项 | R92 新驱动 | 10-18 旧驱动 |
|---|---|---|
| 观察时长 | 204.8 s（到中止） | ~19.5 min（用户取消，未跑完） |
| 私有内存 | 20.71 GB @ 204.8 s | 1.99 GB @ 19.5 min |
| 增速 | ~6.1 GB/min | ~0.013 GB/min（慢 ~480×） |
| 工作集 | 19.75 GB | 1.66 GB |
| CPU 时间 | — | 1,169 s（单核满载） |
| 结果 | 8 GiB 记账上限中止 | 未中止，仍在缓慢推进 |

**两点判读**：
1. 旧驱动**没有泄漏**（1.99 GB / 19.5 min），但比历史基线慢约 8×（基线 150 s 完成）——
   说明 R90 的 Aura 侧改动（Hir 每个节点 3 次 `strdupRetain`、ModuleLink 每模块 mark/rewind）
   在 scratch 关闭时属于纯开销。
2. R92 打开 scratch 后，内存形态从"慢但可控"变成"线性爆炸"——**退化来自回收链路的实现，不是源码树变大**。

> 该 A/B 因会话中断被取消，未留下 summary/timeline 产物，仅保留上述过程观测值。

---

## 五、根因分析（二进制 + 源码证据）

### 5.1 证据 A：两个目标文件重复定义整套区域符号

`llvm-nm --defined-only` 对比驱动链接时的两个 COFF：

```
aura_link_fix.obj   : 34 个定义符号
aura_std_cffi.obj   : 894 个定义符号
交集（13 个）:
  RegionEnter  RegionReset  RegionScratchTarget  RegionRetainTarget
  ScratchMark  ScratchRewind  StrdupRetain
  g_rgn_active  g_rgn_ret  g_rgn_target  g_scr   （连状态变量也重复！）
  @feat.00  ??_C@_00CNPNBAHC@?$AA@
```

驱动链接命令（2026-10-19 20:46 构建日志）：

```
clang hat-native.obj aura_link_fix.obj aura_std_cffi.obj aura_syscalls.obj -o PhotonHatCompile.exe -O2 -Wl,/STACK:67108864 -Wl,/FORCE:MULTIPLE
```

`/FORCE:MULTIPLE` 使重复定义不报错，**链接器取命令行中靠前的定义**（`aura_link_fix.obj`）。
结果是一个"嵌合体"：驱动的 `@ScratchMark`/`@ScratchRewind`/`@StrdupRetain` 走 `aura_link_fix.c`，
而实际把字符串临时物塞进 arena 的 `aura_scr_alloc` 在 `aura_std_cffi.c` 里；
两者靠"谁先定义谁赢"的状态变量（`g_scr` 等）**碰巧共享同一份内存**才没有立刻崩溃。

### 5.2 证据 B：`scratchRewind` 的 mark=0 全重置路径泄漏 chunk（主因）

`aura_link_fix.c:116`（`aura_std_cffi.c` 的 R87 版同构）：

```c
int64_t scratchRewind(int64_t mark) {
    int64_t idx = mark >> 40;
    if (idx <= 0 || idx > g_scr.n) {      // ← mark==0 走这里
        g_scr.cur = NULL; g_scr.off = 0; g_scr.cur_cap = 0;
        g_scr.n = 0;                       // ← 丢弃全部 chunk 指针
        g_scr.used = 0;
        return 0;
    }
    ...
    g_scr.n = idx;                         // 截断列表，同样不 VirtualFree
}
```

只有 `regionReset()` 会 `VirtualFree`；`scratchRewind` 任何一个分支都**从不释放**被丢弃的 chunk。

而 `scratchMark()` 在 arena 为空时返回 0：

```c
int64_t scratchMark(void) {
    if (g_scr.n <= 0 || !g_scr.cur) return 0;   // ← 空 arena ⇒ mark = 0
    ...
}
```

**归纳链**（ModuleLink 的既有纪律：入口 mark → 切 scratch → parse → 切 retain → lower → 末尾 rewind）：

1. 第 1 个模块入口：arena 初始为空 ⇒ `mark = 0`；parse 期间分配 chunk A、B、C。
2. 模块末尾 `scratchRewind(0)` ⇒ 全重置分支 ⇒ `n = 0`，A/B/C 的指针被丢弃，**内存仍驻留**。
3. 第 2 个模块入口：arena 又是空的 ⇒ `mark = 0` ⇒ 重复第 2 步。
4. ⇒ **每个模块都泄漏自己的 scratch 高水位**，设计中的"chunk 保留复用"永不触发。

R87 的 `ScratchReset()` 注释写明了本意——"区块**保留复用**不 VirtualFree"（`cur=chunks[0]; off=0;`
但 **`n` 不清零**）。`scratchRewind(0)` 却把 `n` 清零，恰好抹掉了复用所需的 chunk 列表。
这是"空 arena 上 mark 恒 0"与"mark=0 被当作全重置"两个设计缺口叠加的结果。

### 5.3 证据 C：8 GiB 记账与 20.7 GB 实际值的分裂

- `aura_link_fix.c` 的 `rgn_bump()` **完全不更新** `g_aura_mem_used`（无检查、无记账）；
- `aura_std_cffi.c` 的 `aura_rgn_bump()` / `aura_mem_alloc()` 才记账。

⇒ 区域分配（`strdupRetain` 幸存者 + scratch chunk）在记账中不可见；
`used ~8191 MiB` 只统计到堆侧，而私有内存已 20.7 GB。**8 GiB 闸门是被堆记账触发的提前中止**，
即使把它调大，也只是让进程在 30 GB 物理内存耗尽后才被 OS 杀掉（外推全程需 ~35 GB+）。

### 5.4 为什么"启用 scratch"反而退化

| 状态 | 行为 | 结果 |
|---|---|---|
| scratch 关闭（10-18 驱动，BISECT） | 所有字符串临时物 → retain arena，一次 `mark/rewind` 全部空转 | 慢（8×基线），但内存=真实高水位，无泄漏 |
| scratch 开启（R92） | 临时物进 `g_scr`，每模块 rewind 走 mark=0 全重置 ⇒ 丢弃 chunk | 快一点，但**线性泄漏**，20.7 GB 才中止 |

⇒ R90 的"启用 scratch"把一个**未修好的回收原语**放上了热路径。

---

## 六、修复建议（按优先级）

1. **消除重复符号（必须先做）**
   `aura_link_fix.c` 只保留真正缺口的符号（`alloc` / `copy` / `StringOps_strcmp` 等桥接），
   删除它自己那一套 `Region*/Scratch*/StrdupRetain` 与 `g_*` 状态定义；让
   `aura_std_cffi.c` 成为唯一实现。然后**去掉 `/FORCE:MULTIPLE`**，让重复定义变回硬错误——
   否则同类事故还会以"链接器悄悄选错实现"的形式复发。

2. **修 `scratchRewind` 的 mark=0 路径**
   空 arena 不应被当成"全重置"：保留 chunk 列表（`n` 不清零，`cur=chunks[0]`，`off=0`），
   与 `ScratchReset` 同语义；或让 `scratchMark` 在空 arena 时返回一个可区分"第 0 块偏移 0"的
   非零标记，避免与"空标记"撞码。

3. **统一记账**
   `rgn_bump` 同步 `g_aura_mem_used`，`rewind` 按 drop 扣回；否则 AURA_MEM_LIMIT_MB 永远
   低估 2~3×，无法作为有效的安全阀。

4. **补回归门（防止复发）**
   现有冒烟只测单模块 hello，**测不出这个 bug**。需要一个"多模块 + 大模块"的内存门：
   例如编译 `tests/photon/P1` 下带 10+ 传递依赖的程序，要求峰值 < 2 GB 且曲线出现回落锯齿；
   自举全量则作为周级门（预算：峰值 < 8 GB，耗时 < 300 s）。

5. **修复后的预期**
   按 R87 设计意图，前端 link 峰值应回到 1 GB 量级、全链路 5 GB 上下（历史基线 5,286 MB），
   配合 R90 的逐模块回收应进一步低于基线；届时 `AURA_PHOTON_TIME=1` 的四段计时才有意义。

---

## 七、复现步骤

```powershell
# 1) 重建 seed（纳入工作区改动；约 2.5 min）
cd D:\Code\AuraLang\seed
$env:CARGO_TARGET_DIR = "..\rust\target"
cargo build --release --bin aura --features llvm

# 2) 重建驱动（约 30 s，需 AURA_LINK_FIX_OBJ 指向 build/aura_link_fix.obj）
cd D:\Code\AuraLang
rust\target\release\aura.exe build --aot aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura --output build\hat-native\PhotonHatCompile.exe

# 3) 测量（约 3.5 min 到中止）
powershell -ExecutionPolicy Bypass -File scripts\photon\measure-bootstrap.ps1 `
    -OutDir build\hat-bootstrap-r92-run1

# 产物：build\hat-bootstrap-r92-run1\{summary.json,timeline.csv,run-stdout.log,run-stderr.log}
```

---

## 八、产物清单

| 路径 | 内容 |
|---|---|
| `build/hat-bootstrap-r92-run1/summary.json` | 机器可读结果（墙钟/峰值/退出码/产物大小） |
| `build/hat-bootstrap-r92-run1/timeline.csv` | 2141 个 50 ms 样本（t_ms, ws, priv） |
| `build/hat-bootstrap-r92-run1/run-stdout.log` | 完整驱动输出（107 个 `[mod-start]`、105 个 `[mod]`） |
| `build/hat-bootstrap-r92-run1/run-stderr.log` | 8 GiB 上限中止原文 |
| `scripts/photon/measure-bootstrap.ps1` | 本轮新增的测量脚本（可复用） |
| `build/hat-native/PhotonHatCompile.exe` | R92 重建驱动（旧版备份 `.20261018.bak`） |
| `rust/target/release/aura.exe` | R92 重建 seed（旧版备份 `.20261018.bak`） |
