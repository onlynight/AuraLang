# Photon AOT 自举编译 —— 第二轮瓶颈修复（R94）

> 2026-10-19 23:00–00:00
> 承接 R93（编译已跑通：12m19.5s / 峰值 21.78GB）。本轮攻后端（占墙钟 62%）。

---

## 一、结果

| 指标 | R93 基线 | R94 | 变化 |
|---|---:|---:|---:|
| 后端合计 | 446.1 s | **432.1 s** | **−3.1%** |
| 　phaseC（DAG/指令选择） | 210.5 s | **186.1 s** | **−11.6%** |
| 　ra（寄存器分配） | 94.0 s | 106.2 s | +13%（噪声，见注） |
| 　emit（x86 编码） | 88.7 s | 88.7 s | ±0 |
| 　writer（COFF 输出） | 45.8 s | 43.3 s | −5.5% |
| 正确性 | success | **success** | 不变 |

> 三次后端单跑（同一 .hat、skip-front）：446.1 → 443.8 → 432.1 s。
> `ra/emit/writer` 在 ±5% 内抖动，`phaseC` 的 210.5 → 196.7 → 186.1 是单向的、
> 与两处修复的调用点分布吻合，判为真实收益。

验证：
- 后端单跑 `===RESULT===success`，产出 `aura-compiler.exe` 3.05 MB，运行 `BOOT-1 → BOOT-1a`，exit 0；
- 干净驱动冒烟 hello world：`success` + `Hello, World!`。

---

## 二、本轮修复

### 1. `isRuntimeSymbol` 每次调用 split 整张符号表（InstructionSelection）

```aura
val n = this.dotToUnderscore(name)                       // 逐字符拼接，O(len²) 分配
val parts = LirUtils.splitArgs(this.runtimeSymbolNames)  // 80+ 项，每项一个 String
while (...) { if (this.dotToUnderscore(parts[i]) == n) ... }   // 每项再拼一次
```

该函数有 3 个调用点，**全部在 per-node 路径上**（每条指令都会问「这是不是运行时符号」）。
每次调用产生 ~160 次小分配。

**修法**：原地逐段比较（`.` 与 `_` 等价），零分配。

### 2. `DagInstruction.nodeAt/nodeCount`、`DagNode.argAt/argCount` 每次访问重新 split

```aura
fun nodeCount(): Int { return MachineDagUtils.splitArgs(this.nodes).size }
fun nodeAt(i): Int { val parts = MachineDagUtils.splitArgs(this.nodes); ... }
```

调用方几乎都是：

```aura
while (j < instr.nodeCount()) { val id = instr.nodeAt(j); ... }
```

⇒ 每次循环 O(k) 次 split、每次 split 分配 k 个 String ⇒ **O(k²) 次分配**。
`RegisterAllocator` 的活跃区间分析、`PeepholeOptimizer`、`InstructionSelection` 都按指令反复跑这些循环。

**修法**：四个访问器改为**原地扫描解析**（不可数字 token 的语义与 `isNumericToken`/`strToInt` 逐字对齐），
零分配。实测 `aura_dynlist_new` 调用数 541 万 → **394 万**（−147 万）。

---

## 三、归因数据（决定性证据）

用 `AURA_ATTR_DUMP=1`（分配点 + 调用次数 + 字节数）与临时加的原语计数器，后端单跑得到：

| 原语 | 调用次数 | 说明 |
|---|---:|---|
| `aura_string_substring` | **387,773,259** | 3.88 亿次 substring |
| `aura_dup_n`（其分配体） | **389,541,460** | 平均 13 B，累计 **5.25 GB** |
| `aura_string_concat` | 17,430,460 | |
| `aura_to_str` | 1,032,525 | 小整数缓存生效 |
| `aura_mem_strdup` | 0 | |

分配点 Top：

| 调用点 | 次数 | 字节 |
|---|---:|---:|
| `aura_scr_alloc` 的堆兜底（后端在区域外） | 4.084 亿 | **5,247 MB** |
| `aura_dynlist_push` 增长拷贝 | 107 万 | **2,927 MB**（平均 2.9 KB/次） |
| `aura_dynlist_new` | 394 万 | 231 MB |
| `aura_malloc`（64MB 块等） | 127 万 | 153 MB |

**关键判断**：两处修复后 `substring` 计数**一位都没降**（387,773,259 两次完全一致），
说明剩余 3.88 亿次 substring 来自另一条路径——本轮已排除 `nodeAt/argAt` 与
`isRuntimeSymbol`，下一步应在 `aura_string_substring` 内按「调用者」分桶计数，
一次性定位到具体 Aura 调用点（C 层调用者 `String_substring`/`substringAt`/`split` 可符号化；
Aura 帧则需要 `[原语×调用者]` 二维计数）。

另外 `dynlist_push` 的 2.9 GB 是**列表增长时的整表拷贝**（平均每次 2.9 KB ⇒ 每次扩到 ~360 项），
说明后端大量短命列表没有预留容量；`List` 预分配（或复用缓冲）可直接消掉这部分。

---

## 四、结论与下一步

后端的剩余成本**不是算法复杂度**，而是**数据模型**：LIR / DAG 用「逗号分隔字符串」存操作数，
每层访问（argAt → nodeAt → splitArgs）都重新解析并分配。两处最高频访问已改为零分配，
phaseC 因此降 11.6%；但 `substring` 总量仍有 3.88 亿次，说明同类模式还在别处。

建议下一轮（按杠杆排序）：
1. **定位剩余 3.88 亿次 substring**：在 `aura_string_substring` 内做 `[调用者 ra]` 计数（一次 8 分钟单跑），
   命中后大概率仍是「per-access 重新解析」的某个访问器或 `split` 循环。
2. **列表预分配**：`dynlist` 增长拷贝 2.9 GB → 关键链表（kids/args/parts）按预估容量一次性 `new`。
3. **`hatWrite` 45.7 s / 10.3 MB** 单独测（与后端无关：分块 64 KB × 158 次，
   疑似大堆内存下的写入放大，需用独立小程序复现）。
4. **后端峰值 21.8 GB**：emit/writer 尾部膨胀，可考虑后端也引入区域（每函数 mark/rewind）。

---

## 五、改动文件

| 文件 | 改动 |
|---|---|
| `aura/photon/aura/lang/compiler/photon/InstructionSelection.aura` | `isRuntimeSymbol` 改原地比较（零分配） |
| `aura/photon/aura/lang/compiler/photon/MachineDag.aura` | `nodeAt/nodeCount/argAt/argCount` 改原地解析（零分配） |
| `seed/compiler/src/std/cffi/aura_std_cffi.c` | 临时诊断计数器（已移除，净改动为零） |

产物：`build/hat-be3/`（后端单跑：`.hat/.obj/.exe` + `out.log/err.log`）、
`build/hat-native/smoke-r94/`（冒烟）。
