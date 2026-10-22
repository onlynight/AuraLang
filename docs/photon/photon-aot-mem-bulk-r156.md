# Photon AOT —— 内存大头消减分析（2.6GB 的指令选择器记账表）

> 日期：2026-10-21　上游：`photon_aot_arc_p4_2026-10-21.md`（R151 归因）、`arc_implementation_plan.md` P4-①
> 结论先说：**这不是 ARC 问题，是一个 O(n²) 的索引口径问题（已源码确认）**。
> `nodeMapIds` / `labelNames` 每函数都按**程序级**的 vid / blockId 线性补齐，
> 第 k 个函数的表长 ≈ 此刻全局 id 数 ⇒ 总量 quadratic。
> 把它改成「按函数内偏移索引」即可，**不需要 ARC、不需要 free、不需要新原语**，
> 预期把 ≥64KB 大块从 2867MB 压到 ~450MB，**同时**把 1.8 亿次 append 降到 ~420 万次。

---

## 一、大头在哪（R151 实测，不再猜）

自举退出时后端堆载荷 **4334MB**，其中 **2867MB 落在 4216 个 ≥64KB 的块上**，构成是：

| 分配点 | 存活 | 单块上限 | 维护的表 |
|---|---|---|---|
| `InstructionSelector.registerNode` → `listAppend` | **1125 块 / 1441 MB** | 2048 KB | `nodeMapIds: List<Int>`（vid → dagId） |
| 同上（另一条调用路径） | 683 块 / 797 MB | 2048 KB | 同上 + `slotAliasFrom/To` |
| `InstructionSelector.registerLabel` → `listAppend` | **1616 块 / 344 MB** | 512 KB | `labelNames: List<String>`（blockId → 标签名） |
| `analyzePhiPreds` → `listSet` | 462 块 / 129 MB | 4096 KB | `phiEntryMap*` / `phiClaim*` |
| 其余（SB/拼接等） | — | — | **< 60 MB** |

**平均每块 1.28MB。** 关键事实：`List<Int>` / `List<String>` 的元素都是 **8 字节**
（`Collections.push` 是 `Memory.write64(items + len*8, v)`）⇒ 1.28MB ≈ **16 万个元素**。
按每个函数各建一张表算，等于「平均每个函数有 16 万个 vid」——**这与「117 模块 / 1992 函数」的规模严重不匹配**，
所以下面三个假设里，H2/H3 至少有一个成立。

## 二、三个假设与判别方法

### H1（乐观）表确实是满的 —— 也就是真的需要这么多
若每个 vid 都真有节点，那这是**活数据**，只能靠「缩小元素宽度」（见 L3）或改变算法（不建表）。
判别：`nodeMapIds.size` 与实际 `registerNode` 调用次数之比。

### H2（结构）每次 `select()` 重建 → 旧的成垃圾，且全程无释放
`select()` 开头 `this.nodeMapIds = arrayListOf<Int>()`，每函数一份；
AOT 驱动侧**根本没有 free**（C 运行时 malloc 只分配不释放）⇒ 1125 份旧表全部滞留。
判别：表份数 == 函数数（1125 ≈ 可达函数数），这与实测 1125 块**完全吻合**。

### H3（口径）索引是「全局 id」而不是「函数内 id」→ 二次膨胀 —— **已源码确认**

`registerNode` 写法是 `while (size <= lirId) { add(-1) }`，`registerLabel` 是
`while (labelNames.size <= blockId) { add("") }` —— **按绝对 id 线性补齐**。

这些 id 是**程序级**的（源码级证据）：

| 事实 | 出处 |
|---|---|
| `LirProgram.valueCount`（vid 计数器）在 `LirProgram` 上，不在 `LirFunction` 上 | `Lir.aura:284`（`class LirProgram` 在 255 行，`class LirFunction` 在 208 行） |
| `LirProgram.blockCount`（块计数器）同样在 `LirProgram` 上 | `Lir.aura:287` |
| `v.id = this.valueCount` ⇒ vid 全程序单调递增 | `Lir.aura:312` |
| `LirFunction` 只有 `blocks: List<Int>`，没有任何 id 计数器 | `Lir.aura:208-255` |

⇒ 第 k 个函数建的 `nodeMapIds` 长度 ≈ **此刻的全局 vid 数**，于是
**总量 O(n²)**：1125 个函数 × 平均 16 万项 ≈ 1.8 亿项 ≈ 1.4GB —— 数量级与实测完全对上。
`labelNames` 同理按全局 blockId 补齐 ⇒ 344MB。
（`InstructionSelection.aura:1002` 的注释声称「各函数 vid 从 0 起编号」，
**与 `Lir.aura` 的实际实现矛盾**；注释不可信，以 `LirProgram` 的计数器为准。）

**这条同时解释了时间**：每函数都要 `add` 到 16 万次 ⇒ 全程序约 **1.8 亿次 append**，
每次还带翻倍扩容拷贝 —— 没有回收机制时它是内存大头，有回收机制时它是时间大头。

## 三、可以动的四个杠杆（按优先级）

> H3 已在源码层确认，所以 **L2 是首选**（收益最大、不需要新原语、时间内存同降）；
> L1 变成「L2 之后仍有余量时的补充」；L3 正交；L4（ARC）最后。

### L2　索引改口径（消 H3，**收益最大、且不需要新原语**，风险中）
在 `selectFunction` 入口记下本函数的 `vidBase` / `blockBase`（= 此刻的全局计数器值），
把四处「按绝对 id 索引」改成「按 `id - base` 索引」：

| 方法 | 现在 | 改成 |
|---|---|---|
| `registerNode(lirId, dagId)` | `while (size <= lirId) add(-1)` | `while (size <= lirId - vidBase) add(-1)` |
| `lookupNode(lirId)` | `lirId >= size` 边界 | 同上偏移 |
| `registerLabel(blockId, s)` | `while (size <= blockId) add("")` | `while (size <= blockId - blockBase) add("")` |
| `lookupLabel(blockId)` | 同上 | 同上 |
| `analyzePhiPreds` 的 `phiEntryMap*` | 按 vid | 同上 |

预期：表长从「全局 vid 数」降到「本函数 vid 数」。
按 4.2MB `.obj` 约 420 万个 LIR 值估算，`nodeMapIds` 总量 ≈ 420万 × 8B ≈ **34MB**
（现在是 1441MB）⇒ **−1.4GB**；`labelNames` 同法 ≈ −340MB。
时间：1.8 亿次 append 降到 ~420 万次。

- 前提：同一函数的值 id 是**连续分配**的（LIR 按函数逐个发射）⇒ `id - base` 是紧凑下标。
  落地前用第 0 步的测量确认「本函数 `maxId - vidBase + 1` ≈ 值个数」；若发现跨函数交错，
  退回 base = 本函数见过的 `minId`。
- 风险：四处索引必须**对称**改；漏一处就是静默串号（P390 注释记载过这类 bug：
  vid 撞号导致 `0xd0` 被当对象指针 → #GP）。改完必须逐样例比对产物。

### L1　表可复用（消 H2，约 −1.4GB，风险低）
把 `select()` / 每函数重置处的 `this.nodeMapIds = arrayListOf<Int>()` 换成
**复用一个数组、把长度置 0**（保留 capacity）。语义等价：`lookupNode` 用
`lirId >= size` 兜底、`registerNode` 会重新补齐。
- 需要新原语 `listTruncate(handle, len)`：**C 侧 5 行**（写 `[handle+0] = len`），
  但必须**两侧都加方法拦截**（`seed/emit.rs` + Photon 机器码后端），否则驱动编出来的产物与自己分叉。
- 预期：1125 份 → 1 份，省 **~1.4GB**；`labelNames` 同法再省 **~344MB**；`analyzePhiPreds` 同法 ~129MB。
- 这是**唯一「不需要 ARC、不需要 free」就能拿到的大头**。

### L2　索引改口径（消 H3，收益可能最大，风险中）
若 H3 成立：让 `nodeMapIds`/`labelNames` 用**函数内序号**（或按段 base 偏移），
表长从 O(全局 id) 降到 O(本函数)。内存与时间同降，且不需要新原语。
- 风险：`labelNames` 的 blockId 由 LIR 分配，若 blockId 是程序级，要在 `selectFunction` 入口记 base 并整体偏移；
  涉及 `registerLabel`/`lookupLabel` 两处对称改动，必须逐样例比对产物。
- 若 H3 成立，L2 会让 L1 的收益部分失效（表本来就小了），所以**先查 H3，再决定 L1/L2 的先后**。

### L3　元素宽度 8B → 4B（正交，约 −50%，风险低但需要新类型）
`List<Int>` 的 Int 是 64 位。`nodeMapIds` 存的是 dagId（节点数 < 2^31），
`labelNames` 存的是**字符串指针**（不能窄化）。
⇒ 只对 `nodeMapIds`/`phi*Ids` 这类**纯 id 表**引入 32 位数组（或把两个 id 打包进一个 64 位字），
把 1441MB 压到 ~720MB。
- 代价：需要一个新的「32 位元素」列表原语（同样要双侧拦截），且所有 `[i]` 读写要换成显式 get32/set32。

### L4　真释放（P5 的 ARC）
只有在 L1/L2/L3 之后还有余量才值得做。按 P3 实测外推，String 槽释放只有数十 MB 量级，
**不要指望它解决 2.6GB**。

## 四、建议的执行顺序

| 步 | 动作 | 判据 / 验收 |
|---|---|---|
| **1** | **L2（索引口径）** —— H3 已源码确认，直接做 | 样例 `.obj` 逐字节一致；全链自举 `.hat/.obj` 逐字节一致 + `BOOT-4`；`AURA_BIG_CENSUS` 的 `live` 从 2867MB 降到 <200MB；墙钟应同时下降 |
| **2** | L1（复用）—— 若 L2 后 `nodeMapIds` 仍是按函数计的大块 | 同上 |
| **3** | L3（32 位 id 表） | 同上 + 峰值 WS 下降 ≥15% |
| **4** | 才轮到 P5（机器码 ARC） | — |

> 第 0 步（测量）可以和第 1 步合并：L2 的验收指标本身就是测量结果
> （`AURA_BIG_CENSUS` 的 `live big blocks` 行），不必单独插一轮。

## 五、预期账

| 项 | 现在 | L2 之后（估） |
|---|---|---|
| `nodeMapIds` | 1441 MB | **~35 MB** |
| `registerLabel` 的 `labelNames` | 344 MB | ~10 MB |
| `analyzePhiPreds` 的 phi 表 | 129 MB | ~5 MB |
| 其余（`registerNode` 第二路径的 alias 表等） | 797 MB | 视是否同源，保守估 −50% |
| **合计 ≥64KB 大块** | **2867 MB（占 4334MB 的 66%）** | **~450 MB（−84%）** |
| 峰值 WS | 4810 MB | 估 **~2.5 GB** |
| 墙钟 | 801 s | **应下降**（少 1.8 亿次 append + 更少缺页） |

## 七、L2 第一次实测（R157）：收益验证成功，但口径不完整，已回滚

按 L2 改了 4 处下标（`registerNode`/`lookupNode`/`registerLabel`/`lookupLabel`），
base = 本函数 `blocks` 内 phi/instr/term 的最小 vid、最小 blockId。整链自举实测：

| 指标 | 基线（drv-r152） | **L2 首版（drv-r157）** | 变化 |
|---|---|---|---|
| 峰值工作集 | 4810.2 MB | **3824.4 MB** | **−20.5%** |
| 后端堆载荷 | 4334 MB | **1692 MB** | **−61%** |
| ≥64KB 大块 | 2867 MB / 4216 块 | **225 MB / 687 块** | **−92%** |
| 墙钟 | 787.1 s | 798.1 s | 持平（±1.4%） |
| 存活块数 | 41,275,574 | 41,447,073 | +0.4% |
| `.hat` / `.obj` | 基线 md5 | **变了**（10,952,840 / 4,232,060） | ❌ 语义改变 |
| `BOOT-4` | PASS | PASS | — |

**结论：O(n²) 判断得到证实，且收益比原估计更彻底（1441+344MB 几乎全部消失，
大块从 2867MB 掉到 225MB）。** 但**产物不再逐字节一致** ⇒ 口径有漏：

- 症状：`.hat` +39,515 B、`.obj` +18,564 B —— 是「某些 `lookupNode` 从命中变成未命中、
  ISel 退化到兜底路径」的典型特征（不是崩溃，是代码变多）。
- 原因：有少量被 `registerNode` 使用的 vid **小于本函数 blocks 内的最小 vid**。
  补上 `func.params`（形参 vid 在块之前分配）后，样例**直接段错误** ⇒ 说明范围仍不对，
  还存在其它在本函数 blocks 之外分配的 vid（跨函数引用 / 常量 vid / 其它哨兵）。
- 处理：**已 `git checkout` 回滚** `InstructionSelection.aura`，重建驱动确认恢复基线
  （`14-string-interp.obj` = `d0e2670b…` ✅）。主干保持在绿色状态。

### 下一轮要补的（明确、可执行）

1. **先量出真实 id 范围**，不要再猜：用 `selProbe`（写文件的诊断通道）在 `selectFunction`
   入口/出口各记一行 `func minVid maxVid nParams sizeReg sizeLookup missBelow missAbove`，
   跑一次样例矩阵即可定位「哪些 id 落在块范围外」。
2. 备选口径（任一成立即可）：
   - 若越界 id 只来自**形参**：把 `params` 并入 base 计算（已试，会崩 ⇒ 不止形参）；
   - 若越界 id 是**跨函数引用**：说明这些引用本来就查不到（旧代码靠「表长到全局 id」才命中），
     那就该把 base 取成 **`min(本函数最小 id, 所有被 lookup 的 id 的最小值)`**，
     这需要在 `selectFunction` 之前先扫一遍本函数内所有指令的 `args`；
   - 最保守可行版：base = 本函数 blocks 的最小 id，但 `idx < 0` 时**回落到绝对下标**
     （只在极少数越界 id 上退化成旧行为），这样语义与旧实现逐位一致，
     而 99% 的 id 仍走相对下标 ⇒ 内存收益保留绝大部分。
3. 每版都必须过「样例矩阵 `.obj` 逐字节一致 → 全链自举 `.hat`/`.obj` 逐字节一致 → `BOOT-4`」。

> 口径 3（回落）是**风险最低的收尾方式**：它在数学上等价于旧实现（越界走旧路径），
> 只是把「表长随全局 id 增长」这一条限制在被越界的少数 id 上。

## 八、为什么优先这一块而不是 ARC

- 这 2.6GB 是**编译期记账表**，生命周期天然是「一个函数」——**作用域明确、别名风险极低**，
  与 P3 撞过的「编译器不可见的别名」完全不同类问题；
- 它**不需要任何释放机制**就能回收（复用/L1），因此不受「AOT 驱动侧没有 free」的限制；
- 它同时是**时间热点**（二次 append），改对了时间也会降 —— 这是 P3 之后唯一「时间内存同降」的方向。

**风险提示**：`InstructionSelection` 是 4000+ 行、自举正确性的核心路径。任何改动都必须以
「样例 `.obj` 逐字节一致 + 全链自举 `.hat/.obj` 逐字节一致 + `BOOT-4`」为唯一验收口径，
`bad_magic`/退出码这类弱信号不足以判定（P3 已证明会出现「数据静默损坏但计数为 0」）。
