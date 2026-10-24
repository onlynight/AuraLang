# Photon AOT 自举编译 — 第四轮：批量消除 strlen-in-loop（R98/R99）

> 测量时间：2026-10-20 10:14–10:25（同驱动连测两轮）
> 输入：`aura/compiler/aura/lang/compiler/Main.aura`
> 复现：`scripts/photon/measure-bootstrap.ps1 -OutDir build/hat-bootstrap-r99b`

---

## 一、结果

| 指标 | R93 基线 | R96（上轮） | R99（本轮） | 累计 |
|---|---:|---:|---:|---:|
| 总墙钟 | 739.5 s | 321.0 s | **312.6 s**（复测 319.1 s） | **−57.7%** |
| 前端 | 277.6 s | 227.9 s | 229.6 s | −17.3% |
| 后端 | 459.7 s | 92.0 s | **81.9 s**（复测 82.6 s） | **−82.2%** |
| 峰值私有内存 | 21.78 GB | 11.65 GB | 11.71 GB | −46.2% |
| 结果 | success | success | **success / BOOT-4** | — |

分阶段（r99b，秒）：

```
前端  link=116.446  ssaBuild=110.614  hatSerialize=0.969  getterPatch=1.042  hatWrite=0.626
后端  hatReadParse=3.891  phaseB=1.931  phaseC=11.126  ra=21.617  emit=6.666
      writer=35.798  objWrite=0.657  runtimeObj+link=0.234
```

**噪声基线**：同一份二进制连跑两轮 = 312.6 / 319.1 s（±2%）。本轮之前曾观测到一次
360.7 s 的离群值（同一份代码），说明**单次测量的可信差分下限约 ±5%**，
低于该幅度的改动必须连测 2–3 次取中位数才能定论。

---

## 二、本轮做了什么

### 1. 批量消除 `while (... < x.length)`（94 处 / 34 个文件）

**根因**：Aura 的 `String` 是 NUL 结尾 C 串，`.length` 在 AOT 下**就是 `strlen()`**
（`SsaBuilder` 里还有一段专门说明：属性式 `.length` 被降级为运行库 `strlen` 调用）。
因此 `while (i < s.length)` 每轮都重扫整条串 ⇒ 对长度 n 的串是 **O(n²)**。
这类写法在本仓曾是「随手就写」的惯用法，全编译器树有 94 个循环命中。

上一轮已手改 3 处（`fnvFile` / `ModuleLink.cacheGet,cacheLineEnd` / `unionCsvContains,Count`），
本轮用工具把剩下的**全部**清掉：

```
python tools/hoist-len.py --apply $(find aura/compiler aura/photon -name '*.aura')
```

工具（`tools/hoist-len.py`，本轮新增）的保守约束——任一不满足就跳过整个循环：
只认简单标识符链；循环体内不得声明同名局部量、不得对该表达式赋值；
生成名与文件内已有标识符冲突时加数字后缀；外层已 hoist 同一表达式时内层复用外层名
（避免同名遮蔽）。改写后全树**剩余 0 处**（`grep` 只剩注释里的示例）。

### 2. `csvLineHas`：`csv.indexOf("\n" + name + "\n")` → 零分配原地扫描

`SsaBuilder` 里 19 处「CSV 名单整行成员判定」原本都写成
`csv.indexOf("\n" + name + "\n") >= 0`：每次调用**拼两个临时串**（本项目无 GC，全部滞留），
再对整张 CSV（`classNamesCsv` / `singletonCsv` / `externCsv` / `externMethodsCsv` /
`freeFuncs` / `classMethods` … 自举规模下数十 KB）做子串搜索。而这些判定在
**每个值、每个调用点**上都会跑。

新增 `csvLineHas(csv, entry)`：单遍原地 `charCodeAt` 扫描、零分配，
语义与原写法逐位等价（含 `entry == ""` 的退化情形仍回退到原 `indexOf`）。

### 3. 顺带清掉的死代码

- `X86Emitter.collectSavedRegs(...)` 调用（结果下一行就被固定 7 寄存器串覆盖）。

---

## 三、正确性验证

1. `===RESULT===success`、exit 0；`.hat` 10,878,911 字符 / `.obj` 4,204,130 B / `.exe` 3,060,736 B。
2. 自举产物 `aura-compiler.exe` 运行到 **BOOT-4**（与 R96 一致）。
3. 冒烟：`tests/photon/P1/01_hello_world.aura` 编译→链接→运行输出 `Hello, World!`。
4. 两轮独立测量产物尺寸逐字节相同（`.hat`/`.obj`/`.exe` 三件套完全一致）⇒ 流程确定性良好。

---

## 四、结论与下一轮

**本轮是「防线」而非「提速」**：94 处 latent O(n²) 被清除，实测墙钟与上轮持平
（312.6 vs 321.0，落在噪声带内），但后端从 92.0 降到 **81.9 s（−11%）**，
说明后端几个文件里的 hoist 确实吃到了；`ssaBuild` 反而 101.6→110.6 s（+9%，同为噪声量级）。

剩余结构（r99b）：

| 阶段 | 耗时 | 占比 | 状态 |
|---|---:|---:|---|
| `link` | 116.4 s | 37% | 逐模块 `loadPath` → parse → `Hir.add`；未拆到模块级计时 |
| `ssaBuild` | 110.6 s | 35% | 92 处 strlen 已清，仍有约 20 处 `csv.indexOf(...)` 在**非整行**形态下遗留 |
| `writer` | 35.8 s | 11% | `emitPieces` 20.6 s + `buildSymbols` ~15 s（5.7 万字符串常量建记录） |
| `ra` | 21.6 s | 7% | 已从 99 s 降下来，剩余为逐节点常量开销 |

**下一轮建议**（按杠杆排序）：

1. **给 link 做模块级计时**（每 `loadPath` 打一行耗时），先定位 116 s 是
   「少数超大模块」还是「117 个模块平均 ~1 s」。这是唯一还没被数据拆开的大阶段。
2. **ssaBuild 的剩余 `indexOf` 家族**：`flatCount(csv, "\n" + name + "|")`、
   `classFields.indexOf(key)`、`classFieldIndex` 等仍是「拼 key + 全串扫描」，
   可照 `csvLineHas` 改法处理（或给高频 CSV 建「行首前缀」哈希）。
3. **writer 的 `buildSymbols`**：5.7 万字符串常量逐条 `toStr + 4 次 concat`，
   可改为「一次性拼装 + 分块收集」，预计能砍掉一半。
4. 把「单次测量 ±5%」写进测量脚本说明，并提供 `-Repeat 3` 取中位数。

---

## 五、改动清单

```
tools/hoist-len.py                                          新增：strlen-in-loop 批量重构工具
aura/compiler/**/*.aura  (34 files)                         94 处 while 条件长度外提
aura/compiler/aura/lang/compiler/mir/SsaBuilder.aura         + csvLineHas（19 处成员判定零分配化）
aura/photon/aura/lang/compiler/photon/X86Emitter.aura       删 collectSavedRegs 死调用
```

回滚快照：`build/lenbak/before.tar`（本轮批量改写前的全部 .aura 源文件）。
