# Photon AOT 自举编译 — 第五轮：link 阶段降 88%（R101/R102）

> 测量时间：2026-10-20 10:50–11:02（同驱动三连测）
> 输入：`aura/compiler/aura/lang/compiler/Main.aura`
> 复现：`scripts/photon/measure-bootstrap.ps1 -OutDir build\hat-bootstrap-r102 -Repeat 3`

---

## 一、结果

| 指标 | R93 基线 | 上轮 (R99) | 本轮 (R102) | 累计 |
|---|---:|---:|---:|---:|
| 总墙钟（中位数） | 739.5 s | 312.6 s | **225.7 s** | **−69.5%** |
| 前端 | 277.6 s | 229.6 s | **137.9 s** | −50.3% |
| ├ `link` | 128.2 s | 123.6 s | **16.7 s** | **−87%** |
| └ `ssaBuild` | 101.6 s | 110.6 s | 117.6 s | — |
| 后端 | 459.7 s | 81.9 s | 87.5 s | −81.0% |
| 峰值私有内存 | 21.78 GB | 11.71 GB | **10.27 GB** | **−52.8%** |
| 结果 | success | success | **success / BOOT-4** | — |

三连测墙钟：225.666 / 223.098 / 225.908 s（±1%），本机噪声带显著收窄。

分阶段（r102/run1，秒）：

```
前端  link=16.723  ssaBuild=117.635  hatSerialize=0.988  getterPatch=1.048  hatWrite=0.588
后端  hatReadParse=4.359  phaseB=2.248  phaseC=11.983  ra=23.391  emit=7.342
      writer=37.173  objWrite=0.739  runtimeObj+link=0.297
```

内存形态不变：前端涨到 10.5GB → t=136.7s 区域重置 **300 ms 内回落到 517MB** → 后端到 5.9GB。

---

## 二、怎么找到的：先把 link 拆到模块级

前几轮一直没能拆开 `link`（116–124 s），这一轮加了两行探针：

- `[modt-in] <µs> <path>` / `[modt-out] <µs> <path>`（入口/出口时间戳，**只加表达式不加局部量**，避免扰动这个递归热函数的栈布局）
- `[modt-parse] <µs> <path>`（parse 结束时刻）

事后把日志当成**括号序列**解析，就能算出每个模块的**自耗时** = total − Σ子模块。

结果非常集中（117 个模块，自耗合计 114.0 s）：

| 模块 | 自耗时 | 节点数 | µs/节点 |
|---|---:|---:|---:|
| **PhotonRuntime.aura** | **36.10 s** | 203,100 | **0.178** |
| Emit.aura | 11.16 s | 115,835 | 0.096 |
| SsaBuilder.aura | 5.90 s | 140,777 | 0.042 |
| X86Emitter.aura | 5.43 s | **212,627** | **0.026** |
| PhotonPipeline.aura | 4.05 s | **221,027** | **0.018** |
| HatParser.aura | 3.38 s | **216,983** | **0.016** |

两个结论：

1. **parse 完全不是问题**：PhotonRuntime 的 parse 只占 0.40 s，其余 35.7 s 全在
   `lowerTopLevel`（`HirLowerer`）。
2. **不是「文件大」也不是「节点多」**：X86Emitter / HatParser 的节点数比 PhotonRuntime
   还多，却只花 1/7 的时间。PhotonRuntime 的**单节点成本是同类模块的 7–10 倍**
   ⇒ 是内容特征触发的、与规模无关的常数因子异常。

内容特征也很清楚：PhotonRuntime 是**成员访问最密集**的模块（满屏 `Cpu.xxx` /
`Memory.xxx` / `ThreadOps.xxx`），而每次成员访问都会去查 lowering 的三张登记表。
表规模在自举规模下是：`typeMethods` **57 KB / ~2000 项**、`typeMemberTys` **12,871 项**、
`funcRetTys` **11,634 项**。

---

## 三、根因：三个 CSV 辅助函数「扫每一项都先 substring」

`HirLowerer` 的三张表都是 CSV 字符串，查表逻辑有三个（都在 `Hir.aura`）：

| 函数 | 旧实现 | 每次调用的分配次数 |
|---|---|---|
| `csvContains(csv, item)` | 每遇逗号 `csv.substring(start, i)` 再比 | **= 项数**（`typeMethods` 约 2000 次！） |
| `csvLookup(csv, key)` | 每项建 `pair` + 两次 `substring` | **= 3 × 项数** |
| `isKnownTypeName(name)` | **每个逗号位置**都 `substring(i+1, i+1+len)` | = 项数（且每个标识符都调一次） |

这些临时串在本项目里是**永不回收**的（AOT 无 GC，前端 era 内全部滞留），
而在成员密集模块上它们被放大成「每次成员访问 2000 次分配」。
这正好解释了 PhotonRuntime 的 10× 异常——它的成员访问次数是同规模模块的 10 倍。

**修法**：三者全部改为**原地 `charCodeAt` 比较、零分配**，
只有真正命中、需要返回子串时才 `substring` 一次（语义逐位等价：同样的首命中、
同样的空输入短路）。

---

## 四、验证

1. `link` 从 123.6 s → **14.9 s**（首测）/ 16.7 s（复测），模块自耗合计 114.0 s → **15.0 s**；
   PhotonRuntime 36.10 → **3.81 s**，Emit 11.16 → **2.60 s**。
2. 全链路 `===RESULT===success`、exit 0。复测的 `.hat`（md5 `0ebe9407…`，10,883,294
   字符）与 `.obj`（md5 `825139f9…`）**逐字节一致**；`.exe` 尺寸同为 3,061,248 B，
   仅 md5 不同（lld-link 写入时间戳），非代码差异。
3. 自举产物运行到 **BOOT-4**；hello world 冒烟输出 `Hello, World!`。
4. **内存同时下降** 11.71 → 10.27 GB（正是「少分配 → 少滞留」的直接后果），
   反证这批分配此前确实全部滞留。

---

## 五、下一轮（ssaBuild 现在是唯一大头，117.6 s / 52%）

`SsaBuilder` 有同族的两个结构性问题，都还没动：

1. **CSV 字符串用 `+=` 累积**：`classMethods` / `methodOwners` / `methodRetTypes` /
   `classFields` / `classMethodRetTypes` 都是
   `this.X = this.X + "\n" + ... + "\n"`，每次追加整串拷贝 ⇒ 对 N 条登记是 O(N²) 字节。
   应改成 `List<String>` + 需要时一次性 `join`（仓库里 `coffJoinList`/`joinPieces`
   已经有现成的「分块 + 两两归并」范式）。
2. **按复合键查长 CSV**：`classFields.indexOf("\n" + cls + "|" + field)`（3 处）、
   `methodOwners.indexOf("\n" + method + "|")`、`methodRetTypes`、
   `classMethodRetTypes`。每次先拼 key 再全串 `strstr`，而表可达 12K 项 / 200KB。
   建议照 `X86Encoder.labelTable` 的做法建开放寻址哈希（同名保留最早 = 现语义），
   或至少「命中即缓存」（只缓存命中，append-only 表上永不失效）。

方法学补充：本轮把 `-Repeat N` 加进了 `scripts/photon/measure-bootstrap.ps1`
（跑 N 次进 `run1..runN` 并打印中位数）——单次测量噪声此前约 ±5%，
现在同一二进制三连测只有 ±1%，后续改动建议至少 `-Repeat 2`。

---

## 六、改动清单

```
aura/compiler/aura/lang/compiler/hir/Hir.aura          csvContains / csvLookup / isKnownTypeName
                                                       改为原地零分配比较（core fix）
aura/compiler/aura/lang/compiler/aot/ModuleLink.aura   临时模块级探针（已移除，方法留档于报告）
scripts/photon/measure-bootstrap.ps1                  新增 -Repeat N（多次测量 + 中位数）
```
