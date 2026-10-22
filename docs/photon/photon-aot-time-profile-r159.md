# Photon AOT 编译耗时分解（R159 实测）

> 日期：2026-10-21　负载：整链自举（编译 Main.aura，118 模块，`.hat` 10.94MB）
> 开关：`AURA_PHOTON_TIME=1`（仓库**已有**的计时设施，`PhotonPipeline.aura:1038`）
> 本轮 wall = 1302.3 s（该次机器负载偏高，绝对值不可与 798 s 直接比；
> **看占比**：同一进程内的相对关系是可靠的）

## 一、阶段耗时（单次自举）

| 阶段 | 耗时 | 占比 | 说明 |
|---|---|---|---|
| **hatReadParse** | **356.3 s** | **27.4%** | 读写 `.hat`（10,942,763 字符文本）并解析回 HIR |
| **phaseC（ISel）** | **297.9 s** | **22.9%** | LIR → Machine DAG（`InstructionSelection`） |
| **writer（COFF）** | **202.8 s** | **15.6%** | `PhotonObjectWriter` 组装目标文件（含 hex 解码） |
| **phaseB（MIR→LIR）** | **187.2 s** | **14.4%** | `Lowering` |
| **emit（X86）** | **99.1 s** | 7.6% | `X86Emitter` 指令编码 |
| link | 63.8 s | 4.9% | 系统链接（lld） |
| ssaBuild | 36.9 s | 2.8% | HIR → SSA MIR |
| ra（寄存器分配） | 38.2 s | 2.9% | `RegisterAllocator` |
| objWrite | 9.2 s | 0.7% | 落盘 |
| runtimeObj+link | 2.4 s | 0.2% | runtime 对象（已缓存） |
| peephole | 0.0 s | 0% | 可忽略 |
| hatSerialize / getterPatch / hatWrite | 1.8 / 1.2 / 0.4 s | 0.3% | |
| **backend 合计** | **1193.1 s** | | 与其它阶段有少量重叠记账 |

## 二、结论：三个大头，顺序明确

1. **`.hat` 往返 27%**（356 s）。这是**前端→后端之间的一次文本序列化/反序列化**：
   10.9MB 文本，读回后要重新解析成 HIR。它比「真正干活」的 ISel 还贵。
2. **ISel 23%**（298 s）。这正是 R157 里 O(n²) 记账表所在的地方
   （每函数 `while (size <= 全局vid) add(...)`，全程序约 1.8 亿次 append）。
   ⇒ **L2 修好之后这一项会显著下降**，而且同时降内存（实测载荷 4334→1692MB）。
3. **COFF writer 16%**（203 s）。组装 + hex 解码，纯字符串/字节搬运。

## 三、优化清单（按 收益/风险 排序）

| # | 动作 | 目标项 | 预期 | 风险 |
|---|---|---|---|---|
| **T1** | **L2 索引口径**（已实测：内存 −61%，此处 phaseC 也会降） | ISel 298 s | 大（去掉 1.8 亿次 append + 大量扩容拷贝） | 中：需解决「少数 vid 越界」的回落口径 |
| **T2** | **`.hat` 走二进制**（或后端直连前端内存、省掉一次序列化往返） | hatReadParse 356 s | 大（文本→二进制通常 3~5×） | 高：格式兼容/自举两代互读 |
| **T3** | **writer 的 hex 中转消除**（`outputHex: String` → 直接字节缓冲） | writer 203 s | 中（省一次 2.8MB hex 的生成+解码） | 中 |
| **T4** | phaseB（Lowering）187 s 的分配热点 | phaseB | 中 | 中 |
| **T5** | emit 99 s：`emitHexByte`/`sbReserve` 已优化过，边际收益小 | emit | 小 | 低 |

**建议顺序：T1 → T3 → T2**。T1 一举两得（时间+内存）且不需要新机制；
T3 是局部改造、可独立验证；T2 收益最大但涉及自举两代互读 `.hat`，风险最高，放最后。

## 五、T1 落地后的实测（R160）

T1 = `InstructionSelection` 索引改「函数内相对下标 + 越界侧表回落」（语义与旧实现逐位等价）。

| 指标 | 基线 drv-r152 | **T1 drv-t1** | 变化 |
|---|---|---|---|
| wall | 787.1 s | **779.1 s** | −1.0%（噪声内） |
| 峰值 WS | 4810.2 MB | **3827.7 MB** | **−20.4%** |
| 后端堆载荷 | 4334 MB | **1694 MB** | **−61%** |
| ≥64KB 大块 | 2867 MB / 4216 块 | **225 MB / 687 块** | **−92%** |
| 样例 `.obj` | 基线 | **逐字节一致（3/3）** | ✅ |
| `BOOT-4` | PASS | PASS | ✅ |

阶段耗时（同一进程内）：

| 阶段 | 负载偏高那次（R159） | **T1 这次（R160）** | 占 backend 比 |
|---|---|---|---|
| hatReadParse | 356.3 s | 197.7 s | — |
| phaseC（ISel） | 297.9 s | **174.4 s** | **25.0%**（基线也是 25.0%） |
| phaseB | 187.2 s | 103.6 s | 14.8% |
| writer | 202.8 s | 128.5 s | 18.4% |
| emit | 99.1 s | 54.9 s | 7.9% |
| backend 合计 | 1193.1 s | 698.7 s | |

**诚实结论**：R159 那次绝对值偏大是机器负载所致；与 R151（787.1 s）同口径比，T1 的时间收益
**基本为零**（−1.0%，噪声内）。且 phaseC 占 backend 的比例**仍是 25.0%**（174.4/698.7），
说明「消掉 1.8 亿次 append」并没有让 ISel 变快 —— ISel 的主要成本在别处
（估计是 `MachineDag` 节点/指令的构建与 `emitValue` 的分派）。

⇒ **T1 的价值是内存（峰值 −20.4%），不是时间。** 时间优化的下一个目标应转向
hatReadParse（27%→ 现占 wall 的 25%）与 writer（18%），即 T2、T3。

## 七、R161：两个坑的最终结论

- **真 bug（已修）**：`Lowering.aura:120` `lfunc.params = srcFunc.params` ——
  `MirFunction.params` 是**字符串**、`LirFunction.params` 是 `List<Int>`，
  AOT 下同为 8 字节句柄故不报错，字段里留下字符串指针 ⇒ 任何 `params.size`/`params[i]`
  都会把字符串头当长度 → 野指针 → 段错误。已删除该赋值。
- **误判（已澄清）**：`List<Int>` 当方法参数本身没问题（`build/_repro/` 两个最小复现全通过）。
  当初与上面那个 bug 在同一个补丁出现，被一起归因了。热路径上仍保留内联循环（更快）。

## 四、注意（方法论）

（原方法论条目见下节前的说明）

## 六、T1 落地踩到的坑（重要）

把 `List<Int>` 当参数传给本类方法（`overIndexOf(ids: List<Int>, id: Int)`）会让
**驱动在编译任何样例时立刻段错误**。改成内联 `while` 循环后正常。
⇒ 该 AOT 路径上「集合句柄跨函数传参」不可靠，与 `ArrayList.add` 注释记载的
`Collections.listAppend(data, item)` 不可直调是同一类约束。
另外 `func.params`（`LirFunction` 的形参 vid 列表）**读一次就段错误**，
所以 `vidBase` 只按 blocks 计算，形参 vid 由越界侧表兜住（正确性不受影响）。


- **`.hat`/`.obj` 的 md5 基线只对「不改编译器源码」的改动有效**。自举是「编译器编译自己」，
  任何 `aura/` 下的源码改动都会改变产物 ⇒ 那种情况下的验收要看
  「样例 `.obj` 逐字节一致 + `BOOT-4` + 语言测试集」，不能看 md5 基线。
  （R157 的 L2 实验里 `.obj` 变化有一部分正是这个原因，但 `11-imports` 样例不一致 +
  加形参基准后段错误仍证明了存在真实的语义差异。）
- `AURA_PHOTON_VERBOSE=1` 只打进度不打耗时；耗时一定要用 **`AURA_PHOTON_TIME=1`**。

## 八、T3 设计（writer 128.5 s / 18% 的靶点）

### 已核实的事实
- `PhotonPipeline` 的 writer 段做四件事：`writer.textSection = machineCodeHex` →
  `parseFuncNames` → `buildSymbols` → **`emitPieces()`**。
- `emitPieces()` = `buildCoffFile()` 在 `piecesMode` 下把**十六进制片段**逐段
  `coffAppend` 进 `coffBuf: List<String>`（`PhotonObjectWriter.aura:1188/1196`）。
- 该文件里有 **71 处 `coffAppend`**；产物 3.08MB ⇒ 约 **6.16M 个 hex 字符**、
  按字段粒度切分意味着**上百万次小 String 分配**。
- 而「hex → 字节」的解码只花 **9.2 s**（`objWrite` 段）——所以**瓶颈在 hex 的“生产”，
  不在“解码”**。`parseFuncNames` / `joinComma` 早前已改成两两归并（不是二次复杂度）。

### 结论：T3 的正确落点
把 `PhotonObjectWriter` 的输出从「hex 片段列表」改成**字节缓冲**：
- 新增 `coffBytes`（`Memory.alloc` 支撑的可增长缓冲）+ `coffU8/coffU16/coffU32/coffBytesOf(s)`，
  71 处 `coffAppend(hexText)` 逐一替换为对应的字节写；
- `PhotonNativeWriter.writeObjectFilePieces` 直接写缓冲（**零解码**），
  连带把 9.2 s 的解码也省掉；
- 产物逐字节一致是硬要求（COFF 结构不变，只是不再经过文本）。

### 预期
- writer 段：6.16M 字符拼接 + 上百万次小分配 → 纯字节写，估 **−50%~70%**（128.5 s → 40~65 s）；
- objWrite：9.2 s → ~1 s；
- 合计约 −80 s（wall ~779 s → ~700 s，−10%）。
- **风险中等**：71 处替换点，任一处字段宽度写错都会破坏 COFF；必须逐样例比对 `.obj` 字节。

### 为什么这轮没做
71 处替换 + 字节缓冲原语 + 全链验证，一次改不完且无法中途交付；
上下文余量不足时强推会让主干停在「COFF 结构可能错」的状态，风险高于收益。
