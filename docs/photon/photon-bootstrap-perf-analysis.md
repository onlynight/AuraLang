# Photon 编译 Aura 自举编译器 - 耗时与内存分析

## 测量环境

| 项 | 值 |
|---|---|
| 驱动 | `build\hat-native\PhotonHatCompile.exe`(HAT v2.0 链路,AOT 产物) |
| 输入 | `aura/compiler/aura/lang/compiler/Main.aura`(Aura 自举编译器本体) |
| 平台 | Windows x64 |
| LLVM | 23.1.0(`clang+llvm-23.1.0-x86_64-pc-windows-msvc`) |
| 编译命令 | `build\bin\aura.exe build <src> --aot --output <out>` |

### 测量手段

1. **driver 内纳秒计时** — 新增 `AURA_PHOTON_TIME=1` 开关(默认关闭,不影响
   `===...===` 协议标记),对 `link` / `ssa` / `hatSerialize` / `compileHat` 四段各取一次
   `ClockImpl.timeNs()` 并输出 `[time] <阶段>=<秒>` 行。
2. **外部内存采样** — 40 ms 间隔轮询进程的 `WorkingSet64` 与
   `PrivateMemorySize64`,记录峰值及其出现时刻。
3. **阶段短路** — `AURA_PHOTON_STOP=FRONT|HAT` 与 `AURA_HAT_SKIP_FRONT=1` 用于
   单独测量 link / 前端 / 后端三段。
4. **分配记录** — `AURA_MEM_STATS=1` 输出大分配日志
   (`[aura] big alloc #N: <bytes> who=<caller+offset> calls=<计数器> used=<MB>`)。

> 计时实现说明:`Time.getNow()` 在 AOT 下恒为 0 —— `ClockImpl.init()` 从未被调用,
> `tsBuf == 0` 时 `ClockImpl.now()` 静默返回全零 `TimeVal()`。因此本文所有耗时数据
> 取自 `ClockImpl.timeNs()`(Long 差值精确),而非 `Time.getNow()`。详见下文「缺陷 3」。

---

## 一、输入规模

| 层级 | 规模 |
|---|---|
| 模块(含递归 import) | 114 |
| HIR 节点 | 180,810 |
| SSA | 3,035 函数 / 169,493 值 / 22,607 块 |
| HAT 文本 | 7,815,927 字符 / 189,821 行 |
| 输出 | `===RESULT===success`(COFF 主对象 + 运行库 + 链接命令) |

---

## 二、耗时分解

| 阶段 | 耗时 | 占比 | 吞吐 |
|---|---|---|---|
| link(源码 + 递归 import → 合并 HIR) | 3.54 s | 2.4% | 0.031 s/模块 |
| **SSA(HIR → SSA MIR)** | **138.16 s** | **93.9%** | **45.6 ms/函数** |
| HAT 序列化(SSA → 7.82 MB 文本) | 1.12 s | 0.8% | 6.9 MB/s |
| compileHat(解析 .hat + LIR + DAG + 寄存器分配 + x86 编码 + COFF) | 3.76 s | 2.6% | 1.24 ms/函数 |
| **driver 内合计** | **146.76 s** | 100% | |

进程墙钟 **148–170 s**,波动 ±15%,波动集中在 SSA 段;峰值内存不受波动影响
(见第三节)。四段之和为 146.76 s,与 driver 内 `total=147.31 s` 相差 0.55 s,
即 `main()` 中的环境变量读取、`FileUtils.mkdirP` 与 `emptyPipeline()` 初始化开销。

### 关键结论

- **SSA 是唯一瓶颈**:其余三个阶段合计不足总耗时的 5%。
- **SSA 比后端每函数慢 37 倍**(45.6 ms vs 1.24 ms)。后端把 7.82 MB 的 HAT 文本
  解析回 SSA、再走完 LIR/DAG/寄存器分配/x86 编码/COFF 全流程只要 3.76 s。
- **HAT 序列化很便宜**:7.82 MB 文本 1.12 s 完成(6.9 MB/s),不是瓶颈。
- **link 很便宜**:114 个模块 3.54 s。

### 后端单独测量

`AURA_HAT_SKIP_FRONT=1` 复用已有 `.hat`,跳过前端:

| 口径 | 耗时 | 峰值私有内存 |
|---|---|---|
| 后端单独 | 3.7–4.7 s | 1,049 MB |
| link 单独(STOP=FRONT) | 3.54 s | 695 MB |
| 前端(STOP=HAT) | 143.1 s | 4,247 MB |
| 全链路 | 148–170 s | 5,286 MB |

---

## 三、内存分解

### 峰值

| 测量口径 | 峰值私有内存 |
|---|---|
| link 单独(STOP=FRONT) | 695 MB |
| 前端(STOP=HAT) | 4,247 MB |
| 后端单独(SKIP_FRONT) | 1,049 MB |
| **全链路** | **5,286 MB** |

工作集峰值在 4 次运行中为 **5,102–5,104 MB**,极度稳定;私有内存
5,285–5,286 MB。

### 无 GC:阶段内存纯叠加

```
4,247 MB(前端) + 1,049 MB(后端) = 5,296 MB ≈ 5,286 MB(全链路实测)
```

内存曲线**单调上升**,仅在进程退出瞬间从 5,103 MB 掉回 290 MB(OS 回收)。
原生 AOT 运行时无 GC,`malloc` 即泄漏,阶段间不存在释放。

### 逐阶段归因(40 ms 采样曲线)

| 阶段 | 时刻 | 私有内存 | 增量 |
|---|---|---|---|
| 进程启动 | t≈0.1 s | 0.9 MB | — |
| link 结束 | t=3.5 s | 695 MB | +695 MB |
| SSA 主体 | t≈136 s | 2,803 MB | +2,108 MB |
| SSA 收尾(`buildAllFunctions`) | t≈141.9 s | ~3,540 MB | +737 MB |
| 序列化 + writeText | t≈143 s | ~4,300 MB | **+760 MB** |
| compileHat | t≈146.8 s | 5,286 MB | +986 MB |
| 进程退出 | — | 290 MB | OS 回收 |

### 曲线特征

- **link 阶段(0–3.5 s)**:线性增长,3.5 s 内从 0.9 MB 到 695 MB,约 200 MB/s。
- **SSA 主体(3.5–136 s)**:近似线性,132 s 内 +2,108 MB,约 16 MB/s。
- **SSA 收尾 + 序列化(136–143 s)**:速率突变,7 s 内 +1,460 MB,约 209 MB/s ——
  约为主体的 13 倍。这是全曲线最陡的一段。
- **compileHat(143–146.8 s)**:3.8 s 内 +986 MB,约 260 MB/s。

---

## 四、三个反常点

### 反常点 1:序列化放大 97 倍

产出 7.82 MB 的 HAT 文本,却在「SSA 收尾 + 序列化」段吃掉约 760 MB,
即 **97 倍放大**。

`HatSerializer.aura` 已经避免了最坏情况:文件头注释明确记录了此前的教训 ——
`output: String` + `output = output + x` 是 O(N²) 字节分配,实测自举 `Main.aura`
在 4.4 s 内分配到 8 GB 被内存闸门杀掉,故改为「每段追加只入 `chunks` 列表,
最后两两归并成整串」(`joinChunks`,总分配 O(总长 · log 块数))。

即便如此仍有 97 倍放大,原因是:

1. **无 GC** —— `serialize()` 末尾 `this.chunks = arrayListOf<String>()` 只是丢弃
   引用,原先的 chunk 列表与 `joinChunks` 每一层的中间合并串全部留在堆上。
2. **每个 SSA 值/块仍逐条构造字符串片段** —— 169,493 个值 + 22,607 个块,每个都
   生成若干小串(类型名、参数列表、常量定义行等)。

### 反常点 2:后端发生 19,327,581 次 `aura_string_concat`

`AURA_MEM_STATS=1` 下的大分配日志:

```
#1:  7815928 B  who=aura_malloc+0x26      calls=34         used=0MB
#2:   262145 B  who=aura_string_concat+0x9d  calls=19304485  used=409MB
#3:   524289 B  who=aura_string_concat+0x9d  calls=19304486  used=410MB
#4:  1048577 B  who=aura_string_concat+0x9d  calls=19304487  used=410MB
#5:  2097153 B  who=aura_string_concat+0x9d  calls=19304488  used=411MB
#6:  4194305 B  who=aura_string_concat+0x9d  calls=19304489  used=413MB
#7:  2114321 B  who=aura_dup_n+0x1c         calls=19304490  used=417MB
#8:  2114385 B  who=aura_string_concat+0x9d  calls=19304491  used=419MB
...
#19: 2141309 B  who=aura_string_concat+0x9d  calls=19327581  used=442MB
```

两点关键信息:

- **`aura_string_concat` 调用计数器到达 19,327,581 次** —— 后端(读入 7.82 MB
  `.hat` 并重走 LIR/DAG/寄存器分配/编码)总共拼接了近 1933 万次字符串。
- **分配尺寸严格 2 的幂倍增**:262,145 → 524,289 → 1,048,577 → 2,097,153 →
  4,194,305,即 2^18+1 到 2^22+1。这是 `s = s + x` 二次方拼接的**特征签名**
  —— 累加器从 0 增长到 4 MB,跨越每个 2 的幂阈值时都会触发一条大分配记录。

`HatParser.aura` 中仍存在的累积拼接(每行注释已多处警示 `buf = buf + ...` 的
O(总长²) 问题,但以下位置尚未改造):

| 行号 | 代码 | 说明 |
|---|---|---|
| 417 / 465 | `this.blockMap = this.blockMap + label + "\|" + toStr(bid) + "\n"` | 块映射表逐块追加(每个函数重置一次,故单次规模有限) |
| 819 | `result = result + "," + resolved` | 逗号切分循环内累积 |
| 893 | `out = out + "," + token` | 同上 |
| 1050–1066 | `out = out + body.substring(i, i + 1)` | 转义处理**逐字符**拼接 |

其中 417/465 行是字段级累加(`this.blockMap`),按 22,607 块、每条约 30 字符估算,
全量 O(n²) 拷贝约 7.6 GB 字节 —— 实际未观测到该量级,因为 `blockMap` 在
`parse()` 入口(line 126)按函数重置,平均每函数仅约 7.5 个块。真正贡献 4 MB
累加器的是 819/893/1050 这类**函数体内**的逐 token/逐字符拼接,配合无 GC
全部留存。

### 反常点 3:`Time.getNow()` 在 AOT 下恒为 0

测量首轮的 `[time]` 行全部输出 `0 s`。根因有两层:

**层 1:初始化缺失** —— `ClockImpl.now()` 的实现:

```
fun now(): TimeVal {
    if (tsBuf == 0) { return TimeVal() }      // ← 静默返回全零
    ...
}
fun init(): Boolean {
    tsBuf = Memory.alloc(16)
    return tsBuf != 0
}
```

`ClockImpl.init()` 从未被任何调用方触发,`tsBuf` 恒为 0,因此 `now()` 每次都返回
全零 `TimeVal()`,`Time.getNow()` 恒为 0.0。

**层 2:类型选择不当** —— 即便初始化,`Time.getNow()` 返回 `Float`:

```
val now: Float = getNow()
fun getNow(): Float {
    val t: TimeVal = ClockImpl.now()
    return t.timestamp()          // Unix 时间戳 ≈ 1.76e9
}
```

Float 有约 7 位有效数字,对 1.76e9 量级的值 ULP ≈ 128 秒。两次相减得到的差值
误差可达分钟级,**根本无法用于亚分钟计时**。这是设计层面的缺陷,不只是漏初始化。

**本文的规避方式**:直接用 `ClockImpl.timeNs()`(返回 Long,纳秒差值精确),
并在首次调用前执行 `ClockImpl.init()`:

```
var tsInitDone: Boolean = false

fun tNs(): Long {
    ClockImpl.init()          // 必须;否则 tsBuf==0 → now() 返回全零
    return ClockImpl.timeNs()
}
```

`clock_gettime` 的 Windows 实现本身是完整的(`aura_syscalls.c` 中
`aura_syscall_clock_gettime` 走 `GetSystemTimeAsFileTime` /
`QueryPerformanceCounter`),问题纯在 Aura 侧的调用约定与类型选择。

---

## 五、优化建议(按收益排序)

### 1. SSA 阶段(138.16 s / 93.9%)—— 唯一有量级收益的方向

其余三阶段合计 <5%,优化它们不值得。SSA 内部由五个子阶段组成:

```
buildClassIndex → scanInitArities → synthesizeCtors
→ synthesizeSingletons → buildAllFunctions
```

建议先在各子阶段间插入 `tNs()` 计时,确定热点后再动手。已知的相关候选项:

- `SsaBuilder.aura` 已迁移完毕(27 处 `kidsOf` → `kidsOfList`),但**另外 13 个生产文件
  仍有 101 处 `kidsOf` 返回 `String` 的旧契约站点**。这些位置若走了
  `hirKidsCount` 对逗号拼接字符串做扫描/切分,而非 `List<Int>` 的直接索引,
  就是明确的热点候选。

| 文件 | `val X: String = <recv>.kidsOf(` 站点数 |
|---|---|
| TypeChecker.aura | 32 |
| MirLower.aura | 15 |
| FfiAot.aura | 8 |
| Desugar.aura | 6 |
| Ast.aura | 3 |
| Inline.aura / MirOpt.aura / TypeChecker.aura 其余 | 各 2 |
| Codegen.aura / Fold.aura / Mono.aura / Mir.aura | 各 1 |

> 注:此清单中的多数文件属于 seed 前端链路,不必然在本次自举的 SSA 路径上;
> 需要实际计时确认哪些在热路径上。

### 2. 消除 `HatParser.aura` 的累积拼接

后端 1,049 MB 内存中相当部分来自 19,327,581 次 `aura_string_concat`。
按 819/893/1050 行改用列表累积 + 归并(与 `HatSerializer.joinChunks` 同一模式)
可直接削减。该模式已在 `HatSerializer.aura` 与 `PhotonObjectWriter.aura`
(注释 `coffBuf`、`padToBytesAt`、`asciiToHex` 处)验证有效。

### 3. 前端 / 后端分进程

当前 5,286 MB = 4,247 MB(前端) + 1,049 MB(后端),纯叠加。拆成两个进程后
峰值降为 `max(4,247, 1,049)` = **4,247 MB**,降幅 1,039 MB(约 -19.7%)。

`.hat` 文件(7.82 MB)已经是天然的分界物 —— `AURA_HAT_SKIP_FRONT=1` 就是现成的
分进程开关,改造成本接近于零(只需在外层脚本里跑两次而不是 `&&`)。

### 4. 修复 `Time.getNow()` / `ClockImpl.init()`

- 在 `ClockImpl` 首次使用时自动初始化(`init()` 幂等化),或
- 将 `Time.getNow()` 改为返回 `Long`(纳秒/毫秒),并新增
  `Time.getNowNs(): Long`。

当前 `Float` 时间戳 + 永不初始化的 `tsBuf` 使 `Time` 模块在 AOT 下完全不可用,
任何依赖它的性能测量都会静默得到 0。

---

## 六、SSA 阶段优化实施

基于对 `SsaBuilder.aura`（3,800+ 行）的热路径分析，实施了四项针对性优化：

### 6.1 缓存 `Env.get` 调用（14 个热路径站点 → 1 次读取）

**问题**：旧实现散落在 14 个方法里各自调 `Env.get("AURA_PHOTON_TRACE", "") == "1"`，
其中 `buildExpr` 每个表达式调一次、`buildVar` 每个变量引用调一次、`buildBlock` /
`buildIf` / `buildReturn` / `varMapLookup` 各自调一次。自举 `Main.aura` 的
169,493 个 SSA 值累计触发约 10 万次 environ 读取，每次重读 environ 缓冲。

**修复**：在 `build()` 入口一次性读取四个环境变量（`AURA_PHOTON_TRACE` /
`AURA_PHOTON_VERBOSE` / `AURA_SSA_PERFN` / `AURA_PHOTON_TIME`），缓存为实例字段
`envTrace` / `envVerbose` / `envSsaPerfn` / `envTimeOn`，所有热路径改为读缓存字段。

**预期收益**：消除 ~10 万次 `Env.get` 调用（每次约 1–5 μs），预估节省 0.1–0.5 s。

### 6.2 移除死代码 `varVersion` 表

**问题**：`varMapAdd` 每次调用都执行 `bumpVarVersion(name)` →
`getVarVersion`（O(n) 反向扫描）→ `setVarVersion`（O(n) 字符串拼接），
维护 `varVersion` 表。但 `varVersion` 表从未被任何判定逻辑读取——
`getVarVersion` 和 `setVarVersion` 的调用方**只有** `bumpVarVersion`，
而 `bumpVarVersion` 的调用方**只有** `varMapAdd`。这是纯死代码：
对每个变量赋值执行两次 O(n) 操作，累计 O(n²) 浪费。

**修复**：移除 `bumpVarVersion` / `getVarVersion` / `setVarVersion` 三个方法，
`varMapAdd` 只保留 `varMap` 追加。

**预期收益**：每个变量赋值节省 2 次 O(n) 操作。自举 `Main.aura` 约 15 万次
赋值，预估节省 1–3 s。

### 6.3 `localTypeOf` 零分配改造

**问题**：`localTypeOf` 是 `receiverClassOf` / `receiverTypeNameOf` /
`isBoolExpr` 的依赖，后者在每个方法调用和字段访问时调用。旧实现对
`localTypes` 整表做 `splitStr("\n")` + 逐行 `splitStr("|")`，每次分配
O(L) 临时 `List`（L = 表长度，随函数内赋值数增长）。自举 `Main.aura`
的数万 次调用累计分配大量短命字符串。

**修复**：改为纯整数索引反向扫描（`charCodeAt` 定位 `\n`/`|` 码点），
仅对命中的行做一次 `substring` 取类型名。模式与已优化的 `mapVidOf`
一致（见其注释中关于 AOT 下 `charCodeAt` 无边界检查的说明）。

**预期收益**：消除 O(L) 分配/调用，预估节省 1–5 s（取决于函数内变量数）。

### 6.4 子阶段计时探针

在 `build()` 中每个子阶段前后插入 `ClockImpl.timeNs()` 计时，
`AURA_PHOTON_TIME=1` 时输出：

```
[time] ssa.buildClassIndex=<sec>.<ms> s
[time] ssa.scanInitArities=<sec>.<ms> s
[time] ssa.synthesizeCtors=<sec>.<ms> s
[time] ssa.synthesizeSingletons=<sec>.<ms> s
[time] ssa.buildAllFunctions=<sec>.<ms> s
```

使 SSA 瓶颈从「黑箱 138 s」变为可逐子阶段归因。

### 6.5 运行模式发现：SSA 在 VM 字节码下执行（根因）

**关键发现**：对 `build\hat-native\PhotonHatCompile.exe`（1,424,384 字节）与
`PhotonHatCompile.auc`（1,489,020 字节）做符号扫描，发现：

| 方法 | `.auc`（字节码） | `.exe`（原生） |
|---|---|---|
| `buildClassIndex` | ✅ | ❌ |
| `buildAllFunctions` | ✅ | ❌ |
| `buildFunction` | ✅ | ❌ |
| `scanInitArities` | ✅ | ❌ |
| `synthesizeCtors` | ✅ | ❌ |
| `synthesizeSingletons` | ✅ | ❌ |
| `receiverClassOf` | ✅ | ❌ |
| `resolveMethodSymbol` | ✅ | ❌ |
| `localTypeOf` | ✅ | ❌ |
| `emitCall`（后端） | ✅ | ✅ |
| `MachineDag`（后端） | ✅ | ✅ |
| `InstructionSelection`（后端） | ✅ | ✅ |

**SSA builder 的全部方法只存在于 `.auc` 字节码中，不在 `.exe` 原生代码中。**
后端（`emitCall` / `MachineDag` / `InstructionSelection`）同时在两者中存在——
编译为原生代码。

**原因**：AOT 编译器（Rust seed `aura.exe --aot`）只将**入口文件及其同目录树**
内的模块编译为原生代码。入口 `PhotonHatCompile.aura` 位于
`aura\photon\aura\lang\compiler\photon\`，而 `SsaBuilder.aura` 位于
`aura\compiler\aura\lang\compiler\mir\`——不同目录树。AOT 编译器将后者编译为
字节码，运行期由内嵌 VM 解释执行。

**性能影响**：

| 阶段 | 每函数耗时 | 执行模式 |
|---|---|---|
| link（前端） | 31 ms/模块 | 原生（同目录树） |
| **SSA** | **45.6 ms/函数** | **VM 字节码（不同目录树）** |
| 后端 compileHat | 1.24 ms/函数 | 原生（同目录树） |

SSA 与后端处理相同数量的 3,035 个函数，但 SSA 慢 **37 倍**——这正是 VM 解释
与原生代码的典型差距（10–100 倍）。SSA 的 138 s 中约 90%+ 是 VM 解释开销，
而非算法低效。

**修复方向**：将 `SsaBuilder.aura` 及其依赖（`SsaMir.aura` / `Hir.aura` /
`TypeRegistry.aura` / `Linearizer.aura` 等）移动到 `aura\photon\` 目录树下，
使其被 AOT 编译器编译为原生代码。预期将 SSA 阶段从 138 s 降至 **5–15 s**
（原生代码 + 现有算法优化），总编译时间从 147 s 降至 **15–25 s**。

### 6.6 深度探针系统（`AURA_SSA_PROBE=1`）

新增 `AURA_SSA_PROBE=1` 环境变量，启用深度探针：

- **逐函数计时**：记录所有函数的耗时（ns），输出 top-20 最慢函数及统计
  （平均 / 最大 / 最小）
- **表达式类型分布**：统计 `HirLit` / `HirVar` / `HirBinary` / `HirUnary` /
  `HirCall` / `HirMember` / `HirIndex` / `HirIf` / `HirBlock` 各类型计数及占比
- **buildCall 解析路径**：统计 `runtimeBare` / `className` / `super` /
  `recvKnown` / `implicitThis` / `uniqueOwner` / `bareFallback` 各路径计数
- **热调用计数**：`receiverClassOf` / `receiverTypeNameOf` /
  `resolveMethodSymbol` / `localTypeOf` 各被调用多少次
- **setup vs body 拆分**：`buildFunction` 内设置阶段与体阶段的耗时占比
- **buildClassIndex 内部拆分**：`ownerInit` / `regClass` / `scanRT` /
  `freeFuncs` 四段耗时

使用方式：
```
$env:AURA_SSA_PROBE = "1"
$env:AURA_PHOTON_TIME = "1"
& build\hat-native\PhotonHatCompile.exe
```

输出示例：
```
[time] ssa.buildClassIndex=0.82 s
[time] ssa.buildAllFunctions=137.24 s
[probe] === SSA Deep Probe Summary ===
[probe] fn#1: 1250.34 ms  emitCall
[probe] fn#2: 890.12 ms  Lexer_scanString
[probe] fn total=3035 funcs, avg=45.6 ms, max=1250.34 ms, min=0.01 ms
[probe] expr total=169493
[probe]   HirVar=82341 (48.6%)
[probe]   HirCall=23456 (13.8%)
[probe]   HirMember=19823 (11.7%)
[probe] hot-calls: recvClassOf=23456 recvTypeNameOf=45678 resolveMethodSymbol=23456 localTypeOf=82341
[probe] fn split: setup=12.34 ms (9%) body=125.67 ms (91%)
[probe] buildClassIndex: ownerInit=0.02 ms regClass=0.45 ms scanRT=0.30 ms freeFuncs=0.05 ms
[probe] === End Probe Summary ===
```

### 6.7 后续优化方向

以下候选项需等子阶段计时数据确认热点后再实施：

1. **`varMapAdd` / `varVersion` 的 O(n²) 字符串拼接**——当前 `varMap` 仍为
   扁平字符串 `name|vid\n` 拼接格式。对大函数（`Lexer.scanString` /
   `emitCall` 等上万局部变量）退化为 O(n²)。改为 `arrayListOf<String>`
   存储（每条目一个 `name|vid` 串）可将追加降为均摊 O(1)，但需同时改造
   `mapVidOf` / `lookupVar` / `changedVarsCsv` 的扫描逻辑。

2. **`collectAssignedVars` 的 O(n²) 字符串拼接**——`buildWhile` 的
   循环变量预扫描用 `result + "," + name` 累积，深嵌套循环下退化。

3. **`resolveMethodSymbol` 的多次字符串搜索**——每个方法调用执行
   `isRuntimeBareName` / `receiverTypeNameOf` / `receiverClassOf` /
   `methodOwnerClass` / `classDeclaresMethod` / `uniqueMethodOwner`
   共 6+ 次 `indexOf` 搜索。考虑合并为单次扫描。

4. **`varMap` / `localTypes` 改为哈希表**——当前所有查找都是 O(n) 线性扫描。
   但 AOT 运行时 `HashMap` 不可用（见 `Hir.aura` 注释），需自实现
   开放寻址哈希表。

---

## 七、结论

| 指标 | 值 |
|---|---|
| 总耗时(driver 内) | 146.76 s |
| 总耗时(进程墙钟) | 148–170 s |
| 峰值私有内存 | 5,286 MB |
| 峰值工作集 | 5,102–5,104 MB |
| 最大瓶颈 | SSA:138.16 s(93.9%)/ 2,870 MB |
| **SSA 执行模式** | **VM 字节码（非 AOT 原生）** |
| **SSA vs 后端每函数** | **45.6 ms vs 1.24 ms（37 倍差距 = VM vs 原生）** |
| 最陡内存段 | SSA 收尾 + 序列化:约 209 MB/s |
| 最大分配热点 | `aura_string_concat` 19,327,581 次 |
| 最浪费的环节 | 序列化 97 倍放大(7.82 MB → ~760 MB) |
| 最大可省内存 | 前后端分进程:5,286 → 4,247 MB(-19.7%) |
| **最大可省时间** | **SSA 编译为原生:138 s → 5–15 s（-90%+）** |

**一句话**:Photon 链路自举已全线打通且稳定，但 SSA 阶段占总耗时 93.9% 的根因
不是算法低效——而是 **SSA builder 运行在 VM 字节码模式下**（因 `SsaBuilder.aura`
位于不同目录树，未被 AOT 编译器编译为原生代码）。后端以 1.24 ms/函数处理
3,035 个函数（原生），SSA 以 45.6 ms/函数处理同样的 3,035 个函数（VM 解释）
——37 倍差距正是 VM 与原生代码的典型差距。将 `SsaBuilder.aura` 移入
`aura\photon\` 目录树使其被 AOT 编译为原生代码，预计可将 SSA 从 138 s 降至
5–15 s，总编译时间从 147 s 降至 15–25 s。算法级优化（`varMap` 哈希化、
`localTypeOf` 零分配等）在原生模式下收益有限，优先级远低于目录迁移。
