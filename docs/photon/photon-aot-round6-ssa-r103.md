# Photon AOT 自举编译 — 第六轮：SSA 阶段首次拆解 + `changedVarsCsv` 快速路径（R103）

> 测量时间：2026-10-20 11:30–11:41
> 输入：`aura/compiler/aura/lang/compiler/Main.aura`
> 复现：`scripts/photon/measure-bootstrap.ps1 -OutDir build\hat-bootstrap-r103`

---

## 一、结果

| 指标 | R93 基线 | 上轮 (R102) | 本轮 (R103) | 累计 |
|---|---:|---:|---:|---:|
| 总墙钟 | 739.5 s | 225.7 s | **215.1 s（3m35.1s）** | **−70.9%** |
| 前端 `ssaBuild` | 101.6 s | 117.6 s | **104.0 s** | — |
| 峰值私有内存 | 21.78 GB | 10.27 GB | **8.46 GB** | **−61.2%** |
| 结果 | success | success | **success / BOOT-4** | — |

`ssaBuild` **117.6 → 104.0 s（−11.6%）**；峰值内存 **10.27 → 8.46 GB（−17.6%）**
——内存同步下降 18%，说明少掉的正是「逐变量 substring」那批一次性分配（无 GC 全部滞留）。

分阶段（r103，秒）：

```
前端  link=16.047  ssaBuild=104.008  hatSerialize=1.113  getterPatch=1.022  hatWrite=0.506
后端  hatReadParse=4.508  phaseB=2.276  phaseC=12.489  ra=25.156  emit=7.900
      writer=38.007  objWrite=0.720  runtimeObj+link=0.301
```

---

## 二、第一次把 SSA 阶段拆开（此前一直是黑盒）

在 `build()` 的 8 个阶段边界各打一行时间戳（沿用「只加表达式、不加局部量」的做法，
避免扰动这个 15 万节点级递归构建的栈布局）。前端单独跑（`AURA_PHOTON_STOP=B`）后得到：

| 阶段 | 耗时 | 占比 |
|---|---:|---:|
| `buildClassIndex` | **25.10 s** | 22% |
| `scanInitArities` | 0.34 s | — |
| `scanOverloads` | 0.24 s | — |
| `synthesizeCtors` | 1.35 s | 1% |
| **`synthesizeSingletons`** | **42.67 s** | **37%** |
| **`buildAllFunctions`** | **40.42 s** | **35%** |
| `synthesizeDissolvedGetters` | 0.03 s | — |
| `synthesizeCallStubs` | 4.80 s | 4% |
| 合计 | 114.94 s | 100% |

再往下拆 `buildAllFunctions`：给 3274 个函数各打一对时间戳后，**两个巨型函数吃掉一半**：

| 函数（被编译源码里的） | 耗时 |
|---|---:|
| `Emit_emitCall` | **12.03 s** |
| `Emit_callOrBuiltin` | **7.13 s** |
| `buildRuntimeObject` | 1.30 s |
| `writeRuntimeObjectHexFile` ×2 | 2.06 s |
| 其余 3268 个函数合计 | ~17 s |

两者都是 Emit.aura 里的**巨型 if/else 链**（14 万级节点、上千分支）。

---

## 三、一个被证伪的假设（重要，避免下轮再走）

上轮报告写的下一轮计划之一是「`MirBlock.blocks` / `.instrs` 用 `+=` 累积 ⇒ O(N²)」。
**实测证伪**：`HatSerializer` 的注释里明确记载 `SsaBuilder` 走 `program.addValue(...)`
只登记值 + `value.block`，**从不写 `MirBlock.instrs`**；`grep` 也确认 `instrs` 只在
`SsaProgram.addInstr`（本构建不用）里被写。所以那条方向不成立。

真正的机制是另一条：`insertPhisAtMerge` 在**每条 `if`** 上调用两次 `findChangedVars`，
而 `changedVarsCsv` 的通用路径对 `after` 里的**每个变量**都要在 `before` 全表里搜一遍
——自注释即写着 `O(表长 × 变量数)`。对「上千个 if 分支 + 数千个局部变量」的
`emitCall`，这就是实打实的二次项，12 s 完全对得上。

---

## 四、修法：`changedVarsCsv` 的前缀快速路径

关键观察：**`varMap` 只追加**。`varMapAdd` 与各处 `this.varMap = this.varMap + …`
都是尾部追加；唯一的「整体替换」是 `this.varMap = varMapBefore`（从更早快照恢复）
之后继续追加。因此 `after` 几乎总是以 `before` 为**前缀**，变化集恰好是**后缀那几行**。

```aura
if (after.length > before.length) {
    // 前缀判定 O(表长)
    …
    if (samePrefix) { return this.changedSuffixCsv(after, before.length) }  // O(后缀)
}
// 前缀不成立 ⇒ 原通用路径，行为完全不变
```

`changedSuffixCsv` 按「最新优先 + 去重」输出，与通用路径逐位一致
（名字从后缀末尾往前收，重复项只留最新一次）。

**为什么认为安全**：前缀不成立时原路径一字未改；成立时收集的正是通用路径会判为
「已改变」的那些行。验证见下节（产物自举、BOOT-4、冒烟全绿）。

---

## 五、验证

1. `===RESULT===success`、exit 0；`.hat` 10,891,384 字符 / `.obj` 4,207,671 B / `.exe` 3,063,296 B。
2. 自举产物 `aura-compiler.exe` 运行到 **BOOT-4**。
3. 冒烟：`tests/photon/P1/01_hello_world.aura` 编译→链接→运行输出 `Hello, World!`。
4. 旁证：峰值内存同步降 18% —— 若快速路径没被命中，不可能出现这个降幅。

---

## 六、遗留与下一轮

`ssaBuild` 仍是最大单项（104 s / 48%），剩余可攻点按数据排序：

1. **`synthesizeSingletons` 42.7 s / 108 个单例（每个 ≈394 ms，与类大小无关）**。
   在 getter 内部打的三段时间戳显示**几乎不耗时**，且这些戳的时间基准与
   `[ssab]` 阶段戳对不上（同一表达式却相差 4 倍，疑 `ClockImpl.timeNs()` 在
   递归/内联路径上返回了不同基准）。**这是本轮唯一没吃透的地方**，下一轮应改用
   「同一函数内连打两次戳做差分」或直接对 `addBlock`/`addValue`/`fromTypeName`
   做计数统计来定位，而不是再依赖跨函数的绝对时间戳。
2. **`buildClassIndex` 25.1 s**：150K 节点里只有数百个类/对象声明，却花 25 s
   ⇒ 成本在 `registerClass`/`registerClassBody` 内部（`classFields`/`classMethods`/
   `classParents` 的 `+=` 累积，以及 `classBaseIndex`/`ownFieldCount` 的链式扫描）。
3. **后端 `writer` 38.0 s**：`buildSymbols` 里 5.7 万字符串常量逐条
   `toStr + 4 次 concat`，可改一次性拼装；`emitPieces` 20.6 s 的 COFF 组装。
4. 量测工具：`-Repeat N` 已可用；建议后续改动统一用 `-Repeat 2` 取中位数。

---

## 七、改动清单

```
aura/compiler/aura/lang/compiler/mir/SsaBuilder.aura
    changedVarsCsv        新增「前缀快速路径」+ changedSuffixCsv 辅助函数
    （临时阶段/函数/单例探针已全部移除，仅保留仓库原有的 AURA_PHOTON_TRACE 分支）
```
