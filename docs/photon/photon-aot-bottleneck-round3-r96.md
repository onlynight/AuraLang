# Photon AOT 自举编译 — 第三轮瓶颈修复（R96）

> 测量时间：2026-10-20 09:50–09:56
> 输入：`aura/compiler/aura/lang/compiler/Main.aura`（自举编译器入口）
> 驱动：`build/hat-native/PhotonHatCompile.exe`（R96 重建，1,735,168 B）
> 复现：`scripts/photon/measure-bootstrap.ps1 -OutDir build/hat-bootstrap-r96-final`

---

## 一、结果

| 指标 | R93 基线 | R96 | 变化 |
|---|---:|---:|---:|
| 总墙钟 | 739.5 s（12m19.5s） | **321.0 s（5m21.0s）** | **−56.6%** |
| 前端合计 | 277.6 s | **227.9 s** | −17.9% |
| 后端合计 | 459.7 s | **92.0 s** | **−80.0%** |
| 峰值私有内存 | 21.78 GB | **11.65 GB** | **−46.5%** |
| 结果 | success | **success** | — |
| .hat / .obj / exe | 10.3MB / 4.0MB / 2.9MB | 10.88M 字符 / 4,206,665 B / 3,060,736 B | — |
| 自举产物 | BOOT-4 | **BOOT-4** | 不变 |

分阶段（秒）：

```
前端  link=123.613  ssaBuild=101.584  hatSerialize=1.060  getterPatch=1.012  hatWrite=0.630
后端  hatReadParse=4.689  phaseB=2.323  phaseC=12.620  ra=25.251  emit=7.898
      writer=38.192  objWrite=0.733  runtimeObj+link=0.291
```

内存曲线仍是设计预期形态：前端一路涨到 11.93GB → t=227.7s `RegionReset` 在 **300 ms 内回落到 19MB** → 后端涨到 6.8GB → 收尾掉到 780MB。

---

## 二、本轮修掉的 8 处平方级缺陷

### 后端（459.7 s → 92.0 s）

| # | 位置 | 缺陷 | 单项收益 |
|---|---|---|---|
| 1 | `InstructionSelection.phiPredFind` | 每查一个 phi 都扫整张 `phiPredMap`（202KB），且**每条记录都 `substring`**；`substring` 内部先 `strlen(整串)` ⇒ 14,331,200 次 substring × 202KB = **2.77 TB** 的 strlen 扫描 | phaseC **172.9 → 10.6 s（−94%）** |
| 2 | `RegisterAllocator.buildIntervalInterference` | 为每个着色节点都扫 `0..nodeCount`（149,547）全部节点建干扰集 ⇒ O(N²) | ra **99.0 → 21.4 s**（step2 92.9 → 14.8 s） |
| 3 | `X86Encoder.labelOffset` | 每回填一个跳转都线性扫整张 `labels`，且对**每个候选**用 `fieldOf` 现解析 `name\|offset` 记录 ⇒ `resolveLabels` 单点 80 s | emit **87.3 → 6.9 s（−92%）** |
| 4 | `PhotonObjectWriter.composeRelocations` | 每个重定位条目都 `indexOfName(symNames, …)` 线性扫整张符号表（~6 万条）⇒ O(重定位数 × 符号数) | writer 46.8 → 38.4 s |
| 5 | `PhotonObjectWriter.collectExternalSymbols` | 每个重定位条目都线性扫 `funcs`（数千函数）判定「是否本模块定义」 | 同上 |
| 6 | `X86Emitter.walkExtraNodes` | 用不断增长的 CSV 串 `indexOf("\|id\|")` 做闭包去重，并把 id `+` 到串尾（整串拷贝） | 实测仅 0.18 s（隐患已拆除） |
| 7 | `X86Emitter.collectSavedRegs` | 结果在下一行就被固定 7 寄存器串覆盖的**纯死代码**，却要按函数扫完全部指令 | 小 |

修法统一为「**预建索引 / 标记数组 / 哈希表**」，并严格保持遍历顺序与首次命中语义：

- #2 用 `colorPut` 维护**已着色节点的升序索引** `coloredIds`，扫描域从「全部节点」缩到「已着色节点」，遍历顺序与原 `0..nodeCount` 升序**完全一致**（干扰串内容与早停时机不变）。
- #3 建 32 位 FNV 风格的开放寻址表（容量 = ≥2×标签数 的 2 的幂），同名保留最早者。
- #4/#5 同一套哈希（`buildSymIdx` / `symIdxGet`），下标逐位不变。

### 前端（277.6 s → 227.9 s）

| # | 位置 | 缺陷 | 单项收益 |
|---|---|---|---|
| 8 | `PhotonHatCompile.fnvFile` | `while (i < text.length)` —— AOT 下 `.length` 就是 `strlen()`，**每轮重扫整个文件** ⇒ 上百个源文件的 FNV 哈希退化为 O(n²) | hatWrite **55.0 → 0.62 s（89×）** |
| 9 | `ModuleLink.cacheGet` / `cacheLineEnd` | 循环条件里写 `this.cacheBlob.length`，而 blob 是全部预加载 std 源码的拼接（MB 级）⇒ 每次找换行都 O(行长 × blob) | link 138.5 → 123.6 s |
| 10 | `SsaBuilder.unionCsvContains` / `unionCsvCount` | 同上（联合查找名字表可达数十 KB，每个值都要判定） | ssaBuild 124.7 → 101.6 s |

> 说明：link/ssaBuild 的降幅有 ±10% 的run-to-run 波动（曾观测到同一份改动 link=155 s），hatWrite 与后端各项是稳定可复现的。

---

## 三、正确性验证

1. **逐字节等价**：在前端源码不变的前提下，后端三处索引化改动（#2/#3/#4/#5）产出的 `aura-compiler.obj` 与改动前 **md5 完全相同**（`796d3b69ac22d6e025e7a3b6b224e65c`）。
2. **全链路**：`===RESULT===success`、exit 0、`.hat` 10.88M 字符 / `.obj` 4,206,665 B / `.exe` 3,060,736 B 全部产出。
3. **自举产物可运行**：`aura-compiler.exe` 打印 `BOOT-4`（与改动前一致）。
4. **冒烟**：`tests/photon/P1/01_hello_world.aura` 编译→链接→运行输出 `Hello, World!`。

---

## 四、剩余瓶颈（下一轮）

前端已占总耗时 **71%**，且集中在两个老阶段：

| 阶段 | 耗时 | 占比 | 已知嫌疑 |
|---|---:|---:|---|
| `link` | 123.6 s | 38% | 逐模块 `loadPath` → parse → `Hir.add`（每节点 3 次 `strdupRetain`）；`Hir.aura:1833` 等逐字符循环 |
| `ssaBuild` | 101.6 s | 32% | `SsaBuilder` 里 28 处 `while (i < xxx.length)`；`classNamesCsv` / `singletonCsv` / `externCsv` / `externMethodsCsv` 的 `indexOf("\n"+name+"\n")` 成员判定（每次调用都拼 key + 全串扫描） |
| `writer` | 38.2 s | 12% | `emitPieces` 20.6 s（COFF 组装）；`buildSymbols` ~17 s（5.7 万字符串常量建符号记录） |
| `ra` | 25.3 s | 8% | 已从 99 s 降下来，剩余为 `isColorableLocal` 的 blockStarts 扫描与逐节点开销 |

**系统性根因**：本项目 Aura `String` 是 NUL 结尾 C 串，`.length` ≡ `strlen`。因此
`while (i < s.length)` 是**随处可踩的 O(n²) 陷阱**——全仓仍有 60+ 处这种写法
（`SsaBuilder` 28、`Emit` 6、`ModuleLink` 3、`InstructionSelection` 9 …）。
建议下一轮做**批量修**（把长度提到循环外），并加一条静态检查规则；
同理 `csv.indexOf("\n" + name + "\n")` 这类「拼 key 再全串扫描」的成员判定，
应统一改为「预建哈希 / epoch 标记数组」。

## 五、改动文件

```
aura/photon/aura/lang/compiler/photon/InstructionSelection.aura   phiPredMap 预建索引 + isRuntimeSymbol 零分配（R94）
aura/photon/aura/lang/compiler/photon/RegisterAllocator.aura      coloredIds 升序索引；dfs/buildInterference 用 argIds（R95）
aura/photon/aura/lang/compiler/photon/X86Emitter.aura             walkMark/epoch；删 collectSavedRegs 死调用
aura/photon/aura/lang/compiler/photon/x86_64/X86Encoder.aura      labelTable 哈希索引
aura/photon/aura/lang/compiler/photon/PhotonObjectWriter.aura     symIdx 哈希；isDefinedSymbolFast
aura/photon/aura/lang/compiler/photon/PhotonHatCompile.aura       fnvFile 长度外提
aura/photon/aura/lang/compiler/photon/PhotonPipeline.aura         清理临时计时
aura/compiler/aura/lang/compiler/aot/ModuleLink.aura              cacheGet/cacheLineEnd 长度外提
aura/compiler/aura/lang/compiler/mir/SsaBuilder.aura              unionCsvContains/Count 长度外提
seed/compiler/src/std/cffi/aura_std_cffi.c                        归因符号表补充（aura_scr_alloc/StrdupRetain/aura_string_concat）
```
