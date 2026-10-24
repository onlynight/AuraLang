# Photon AOT —— P4/R151：后端堆载荷归因（大块普查）与峰值采样修复

> 日期：2026-10-21　阶段：P4 第一步（**度量先行**，为「阶段 region 回收」与「大临时物分块化」排序）
> 上游：`arc_implementation_plan.md`、`photon_aot_arc_p3_2026-10-21.md`
> 结论一句话：**4.3GB 后端堆载荷里 2.87GB 落在 4216 个 ≥64KB 的大块上，其中约 2.6GB 是
> `InstructionSelector` 每个函数重建的记账列表**——不是字符串临时物，也不是 `.hat` 拼接缓冲。

---

## 一、为什么先做度量

P3 的结论是「692 万个 ≤64B 小块只值 36MB，想降 RSS 必须打大块」，但**不知道大块是谁的**：

- 既有的 `AURA_MEM_STATS` 大块 trace 有 `g_aura_mem_big_trace < 40` 的采样上限，
  自举里 ≥64KB 的分配有 **14,650 次**，前 40 条只能看到开头几个缓冲区；
- `AURA_ATTR_DUMP` 按 `ra0` 归因，但 `ra0` 落在 C 包装 `aura_malloc` 上，**所有
  Aura 级分配被合并成一个桶**；`__builtin_return_address(1)` 在无帧指针的 -O2 产物里
  取到垃圾（实测 `caller=(below-all)+0x0`）。

所以 R151 先把「谁在吃内存」这件事做成可复现的度量，再谈优化。

## 二、R151 做了什么

### 2.1 大块普查（`AURA_BIG_CENSUS=1`，默认关闭）

`seed/compiler/src/std/cffi/aura_std_cffi.c`：

- 对**所有** ≥ 阈值（`AURA_BIG_MIN_KB`，默认 64KB）的分配按**分配点栈帧**建账；
- 维护存活集合（`free` / `realloc` 时摘除），退出时按「存活字节」降序打印 top-20，
  再按「累计字节」打印 top-10；
- 分配点用 `CaptureStackBackTrace(2, 8, …)` 取样（只在稀有的大块分配上做，成本可忽略），
  键取 `fr[2..4]`，因此能区分「同一个 C 包装、不同 Aura 调用点」；
- 只在开启时记录：**默认路径零开销**（本次验收默认自举 787.1s，与 P3 基线 727.6s 的差异
  在既有波动范围内）。

### 2.2 离线符号化（`build/arc-bigsite.py`）

AOT 产物被 strip，运行时只能给「最近 C 符号 + 原始地址」。构建驱动时带
`AURA_LINK_MAP=1` 产出 `<module>.map`，脚本按 `RVA = 运行地址 − imagebase` 二分反查，
即可把栈帧落到 `aura_user_InstructionSelector_registerNode+0x49` 这种粒度。

```bash
# 1) 带 map 建驱动
powershell -c "$env:AURA_LINK_MAP='1'; & .\build\build-driver.ps1 -Tag drv-r151"
# 2) 整链自举（脚本内已开 AURA_BIG_CENSUS=1）
& .\build\arc-acceptance-p3.ps1 -Driver 'build\hat-native\drv-r151\PhotonHatCompile.exe' -OutDir 'build\hat-bootstrap-r151'
# 3) 反查
python build/arc-bigsite.py build/hat-bootstrap-r151/run-stderr.log build/hat-native/drv-r151/drv-r151.map
```

### 2.3 峰值工作集采样修复

`PeakWorkingSet64` 恒为 0 的两个原因都找到了：

1. `arc-acceptance-p3.ps1` 在采样循环**之后**有一句 `$driverPeak = 0`，把结果清零；
2. 更根本的是 `System.Diagnostics.Process` **缓存属性值**——循环里不 `Refresh()` 就永远
   读到首次快照。

两处都修了（`arc-acceptance-p3.ps1` / `hat-one.ps1` / `build-driver.ps1`），并加
`build/arc-peakprobe.ps1` 做定点验证：**20 秒采样到 69.8MB**（修复前同样时长读 0MB）。
> 注：0.2s 级的样例跑不出峰值（轮询周期 500ms > 进程寿命），只有长跑进程才有效。

## 三、验收结果

驱动 `drv-r151`（= P3 默认档，未开 ARC），整链自举：

| 指标 | 结果 |
|---|---|
| 墙钟 | **787.1 s**（P3 基线 727.6 s；同一档位波动区间内） |
| 退出码 / RESULT | 0 / `success` |
| 产物自检 | BOOT-1 / 1a / 1b / 2 / 3 / **4 PASS** |
| `.hat` | 10,913,325 B，md5 `263a3bbd…` **逐字节一致** |
| `.obj` | 4,213,496 B，md5 `54b33c02…` **逐字节一致** |
| 存活堆块 / 载荷 | 41,275,574 块 / **4,334 MB**（与 P3 基线一致） |
| `bad_magic` | 0 |

普查本身（同一轮）：

```
big allocs: 14650 calls / 5433 MB ; live: 4216 blocks / 2867 MB ; overflow=0
```

## 四、归因结果

### 4.1 大块存活前 10 名（按存活字节）

| # | 分配点（反查符号） | 存活 | 累计 | 单块上限 |
|---|---|---|---|---|
| 1 | `InstructionSelector.registerNode` → `Collections.listAppend` | **1125 块 / 1441 MB** | 2812 MB | 2048 KB |
| 2 | 同上（另一条调用路径 `emitPhi`） | **683 块 / 797 MB** | 1553 MB | 2048 KB |
| 3 | `InstructionSelector.registerLabel` → `listAppend` | 1616 块 / 344 MB | 587 MB | 512 KB |
| 4 | `InstructionSelector.analyzePhiPreds` → `listSet` | 154 块 / 54 MB | 105 MB | 2048 KB |
| 5 | 同上（另一处） | 154 块 / 40 MB | 69 MB | 4096 KB |
| 6 | 同上（另一处） | 154 块 / 35 MB | 60 MB | 4096 KB |
| 7 | `StringBuilder.sbReserve` ← `joinSb` | 2 块 / 16 MB | 24 MB | 16384 KB |
| 8 | `StringBuilder.sbReserve` ← `File.readText` | 1 块 / 16 MB | 31 MB | 16384 KB |
| 9 | `StringBuilder.sbReserve` ← `X86Encoder.writeByte` | 1 块 / 10 MB | 15 MB | 8192 KB |
| 10 | `string_concat` ← `X86Emitter.joinPairwise` | 48 块 / 13 MB | 13 MB | 279 KB |

**合计 4216 块 / 2867 MB = 全部 4334 MB 载荷的 66%。**

### 4.2 三条结论

1. **主体不是「字符串临时物」**。所有 `sbReserve` / `string_concat` 站点加起来不到 60 MB，
   P4 原计划②「大临时物分块化」的收益上限就是这几十 MB —— **可以降级**。
2. **主体是 `InstructionSelector` 的记账列表**（#1–#6 ≈ 2691 MB）。`registerNode` 的写法是
   ```aura
   while (this.nodeMapIds.size <= lirId) { this.nodeMapIds.add(-1) }
   ```
   即**按下标线性补齐**；而 `select()` 开头 `this.nodeMapIds = arrayListOf<Int>()`
   **每次调用都重建**。于是：旧数组成为无人回收的垃圾（无 ARC 时永不释放），
   而新数组又要从头 `add` 到最大下标。**存活 1125 个平均 ~1.3MB（≈16 万项）的旧数组**
   ⇒ 总量 ≈ 1125 × 1.3MB ≈ 1.4GB，且累计分配 2812MB ≈ 存活的两倍（教科书式翻倍扩容）。
3. **「阶段 region 回收」的靶点因此变了**：不是「整段代码生成」的临时物，而是
   **每函数一次的选择器状态**。若这些 1125 份状态在函数发射完成后即失效（`select()` 被
   逐函数调用、返回的 DAG 已独立），它们就是纯垃圾 ⇒ 可回收 ~1.4GB。

## 五、下一步（R152）

**第一优先：`nodeMapIds` 的「原地重置 + 一次性扩容」**（低风险、可量化）

- `select()` 里把 `this.nodeMapIds = arrayListOf<Int>()` 改为**复用同一数组并置长度 0**
  （保留 capacity），语义等价（`lookupNode` 用 `lirId >= size` 兜底，`registerNode`
  会重新补齐），但消除了 1125 次重建 ⇒ 内存从 1.4GB 降到「一个数组」；
- `registerNode` 的 `while (size <= lirId) add(-1)` 改为**批量扩容 + 一次填充**
  （对应 `listResize(list, n, fill)` 原语），消除 1.6 亿次 append 的翻倍拷贝。

**第二优先：确认 `select()` 的调用粒度**（决定上面能省多少）

- 一条 `AURA_SEL_PROBE` 计数即可：`select()` 被调用次数、每次的 `src.functionCount`
  与最大 `lirId`。若确认「逐函数调用」，则第 1 项直接回收 ~1.4GB；若确认「整模块一次」，
  则需要看 `registerLabel`/`analyzePhiPreds` 的 1616 + 462 份列表为何并存。

**第三优先（保留但降级）**：`sbReserve` 的大缓冲（≤60MB）、weak 引用、多线程原子档。

**不再优先**：`x = <expr>` 之外的释放通道（P3 已二分证伪）、按对象 ARC 全面开启。
