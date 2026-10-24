# Photon AOT 自举编译 · 第八轮：内存占用分析与治理（R131–R142）

> 测量时间：2026-10-20 16:32–16:37　驱动/产物：`build/hat-bootstrap-r142/run1..run2`

## 结论

| 指标 | R93 基线 | 上轮 (R130) | 本轮 (R142) |
|---|---:|---:|---:|
| 峰值私有内存 | 21.78 GB | 8.46 GB | **5.89 GB（−30.4%）** |
| 总墙钟（2 连测中位） | 739.5 s | 155.2 s | **147.6 s（2m28s，再快 5%）** |
| 前端峰值（RegionReset 前） | — | 8.47 GB | **6.03 GB** |
| RegionReset 回落 | 11.93GB→31MB | 8.47→50MB | **6.03GB → 24MB（300ms）** |
| 结果 | success | success | **success / exit 0 / BOOT-4** |

累计相对 R93 基线：**时间 −80.0%，内存 −73.0%**。`.hat`（md5 `763fe39d`）与
`.obj`（`04ddd167`）两轮逐字节一致；hello 冒烟通过。

内存曲线（run1）：link+ssaBuild 涨至 6.03GB（t=55s）→ `RegionReset` 一步降到
**24MB** ⇒ 前端峰值全部是可回收垃圾；后端涨到 5.32GB 平台期（活数据 + 后端分配），
进程退出时释放。

## 前端 8.4GB 垃圾的构成（分配归因，Top 项）

升级归因设施后拿到 Aura 级调用点（`AURA_ATTR_MAX` 64→16384 哈希桶 + concat/substring/
strdup 三个入口记录 `ra0`，`ra0 − imagebase` 离线对照 `llvm-nm hat-native.obj` 解析）：

| 函数 | concat 次数 | 体积 | 性质 |
|---|---:|---:|---|
| `SsaBuilder.changedSuffixCsv` | 537,654 | **2,641MB** | R103 快速路径里 namesCsv/result 双重 O(k²) concat 链 |
| `Lowering.lowerFunction`（后端） | 13,316 | **1,258MB** | `phiPredMap += ";" + rec`（map 最终 202KB）×1.3 万条 |
| `SsaBuilder.varMapAdd` | 151,900 | 761MB | `varMap +=` 整表拷贝（未修，见遗留） |
| `scanOverloads` | 6,336 | 332MB | CSV += 族 |
| `HatSerializer.joinChunks` | 314,468 | 194MB | 归并范式本体（正常） |
| `initAritiesOf` | 10.5M | 160MB | 每调用一次 substring |
| `setVarVersion`/`synthesizeCallStubs`/`addClassMethod*` | — | ~510MB | CSV += 族 |

## 本轮修掉的（−2.6GB 前端 + −1.3GB 后端）

1. **`changedSuffixCsv` 重写**（前端 −2.6GB）：倒序扫后缀行 + `List<String>` 收集去重
  （`List.add` 走现行 AOT 的 dynlist 通道，可靠性已由 parseIdCsvLenient 验证）+
   两两归并拼接 O(R·log k)。语义逐位一致（最新优先 + 去重）。
2. **`phiPredMap` 改走 `phiPredRecs: List<String>`**（后端 −1.3GB）：Lowering 两个写入点
   改 `recs.add(rec)`；唯一读者 `InstructionSelection.ensurePhiMap` 直接从列表逐条解析
   「phiVid|body」建索引——**完全跳过 202KB CSV 中间串**；旧 CSV 解析保留为回退。

## 过程中的两次重大教训（都写进了代码注释）

1. **归因表 64 桶假象**：`AURA_ATTR_MAX=64` 被 C 内部地址瞬间占满，其余全落
   `overflow` 丢弃——此前所有 "who=aura_scr_alloc" 的归因都严重失真。已改 16K 哈希桶。
2. **归因开销拖慢 6 倍**：扩桶时把堆分配路径的 `ra1` 线性扫描（`for (_a1 < AURA_ATTR_MAX)`）
   从 64 次迭代放大到 **16384 次/每次分配**；后端 ~4 亿次堆分配 ⇒ 后端 96s → 586–704s。
   连没改过的 `hatReadParse` 都"慢了 24 倍"（该相位代码根本没动——这是识别出
   「归因开销」而非「算法退化」的关键线索）。修复：归因记录与 ra1 块整体加
   `AURA_ATTR_DUMP` 开关（不调试时零开销），ra1 扫描上限固定 64。
   ⚠️ 教训：**调试探针本身必须受开关保护**，且扩任何 O(分配数) 结构前先查它的
   每分配成本。

## 遗留（下一轮）

1. **后端 ~5.3GB 平台期**：活数据（MIR+LIR+DAG）估计 ~1GB，其余是后端字符串临时物
   （Lowering/ISel/emit 的 concat）——后端也有自己的「changedSuffixCsv」待清。
2. **`varMapAdd` 761MB**：`varMap +=` 整表拷贝，需把 varMap 存储改分段/列表（读取方多，动静大）。
3. **`scanOverloads` 332MB、`addClassMethod*` 232MB、`setVarVersion` 144MB**：同族 CSV +=。
4. **writer 37.7s**：`buildSymbols` 5.7 万常量逐条 toStr+4 concat（时间与内存双热点）。
5. 前端 6.03GB 里活数据仅 ~50MB（.hat 才 10.4MB）——剩余 ~4GB 仍是 varMap/CSV 累积垃圾，
   可继续按本轮方法归因清理。

## 产出

- 报告：`docs/photon/photon-aot-round8-memory-r142.md`（本文件副本）
- 数据：`build/hat-bootstrap-r142/run1..run2/`（summary.json / timeline.csv / 产物三件套）
- 代码：SsaBuilder.changedSuffixCsv 重写；Lir/Lowering/InstructionSelection 的
  phiPredRecs 通道；CFFI 归因设施（16K 哈希桶 + 开关化 + imagebase 解析）
