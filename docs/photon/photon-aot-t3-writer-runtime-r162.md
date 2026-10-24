# Photon AOT 第 11 轮 —— T3：writer 128.5 s 到底耗在哪（并推翻 T3 原命题）

> 日期：2026-10-21　负载：整链自举（`Main.aura`，118 模块，`.hat` 10.96 MB）
> 开关：`AURA_PHOTON_TIME=1`（阶段计时）+ 新增 writer 分段打点 `[wtime]`

## 一、结论先说

T3 立项时的判断（"writer 的 128.5 s 是 71 处 `coffAppend` 生成 hex 片段、
上百万次小 String 分配造成的"）**是错的**。实测分段显示：真正吃掉这 128 s 的
是**运行时里的两个缺陷**，与 hex 中转几乎无关。

| # | 根因 | 位置 | 单价 | 影响面 |
|---|---|---|---|---|
| **R1** | 每次堆分配调 2 次 `getenv` | `aura_mem_init`（`aura_std_cffi.c`） | **12.2 µs/次 getenv ⇒ 24 µs/次分配** | 全编译器 |
| **R2** | 每次 `substring` 先对**整串** `strlen` | `aura_string_substring` | 大串切 N 刀 = O(N·\|s\|) | 全编译器（前端尤重） |

修掉两条之后：

- **writer 段 136.7 s → 1.79 s（76×）**，产物 `.obj` **md5 逐字节不变**；
- **整链自举 1,241.9 s → 92.6 s（13.4×，同时段对照）**，`BOOT-1…BOOT-4` 全过，
  `.hat` / `.obj` 与修复前**逐字节一致**；
- T3 原方案（hex → 字节缓冲）**不再需要**：writer 只剩 1.8 s，天花板就是它本身。

## 二、writer 128.5 s 的实测分段

`PhotonObjectWriter` 新增 `wMark()`（`AURA_PHOTON_TIME=1` 时才打印，默认零输出），
在 16 个可独立优化的边界打点。自举一次 writer 段（`compileFromSsa` 路径）：

| 段 | 耗时 | 占 writer | 做什么 |
|---|---|---|---|
| pipe0.setup | 1,816 ms | 1.4% | 赋 `textSection` / `parseFuncNames` / `joinComma`（含转储） |
| sym1.funcs | 482 ms | 0.4% | 2,007 个函数符号 |
| sym2.strConsts | 4,006 ms | 3.1% | 8k 条 `@str.N` 符号 |
| sym2b.dataBss | 1 ms | – | – |
| **sym3.externals** | **24,094 ms** | **18.5%** | 63,692 条重定位的外部符号去重 |
| sym4.join | 604 ms | 0.5% | 18,721 条符号拼成 `symbols` 串 |
| **= buildSymbols** | **29,192 ms** | 22.4% | |
| coff2.rdata | 6,573 ms | 5.1% | 188,320 B `.rdata` → hex |
| **coff4.counts** | **42,069 ms** | **32.3%** | `splitDoubleSemi(symbols)` + `composeRelocations()` + 调试 println |
| **coff5.strtab** | **11,295 ms** | 8.7% | 18,721 条长符号名 → 字符串表 hex |
| coff6.headers | 3 ms | – | 20 B 文件头 + 4×40 B 节头 |
| coff7.text | 0 ms | – | 2,861,123 B `.text`（**hex 已现成，零成本**） |
| coff8.rdataData | 1 ms | – | |
| **coff9.relocTable** | **23,270 ms** | **17.9%** | 63,692 条 × 10 B 重定位项 |
| **coff10.symTable** | **14,514 ms** | 11.2% | 18,721 条 × 18 B 符号项 |
| coff11.strTable | 11 ms | – | |
| **= emitPieces** | **97,786 ms** | 75.2% | |
| pipe3.countPieces | 11 ms | – | 累加 322,027 个片段的字符数 |
| pipe4.writeObj | 5,691 ms | 4.4% | 322,027 片段逐个解码落盘 |

**关键观察**：`coff7.text`（2.86 MB，占产物 67%）**耗时 0 ms** —— `.text` 的 hex
由 `X86Emitter` 早已物化，在 `coffBuf` 里只是引用。所以"6.16 M 个 hex 字符的
生成"这个假设从一开始就不成立：真正被逐字节写的是**符号表 + 重定位表 + `.rdata` +
字符串表**这一小片（约 1.2 MB），而它们的单价是 **~45 µs/字节**。

## 三、根因 R1：每次分配两次 getenv（24 µs/次）

`build/_obw/ObwBench.aura`（AOT -O2 微基准，20 万次迭代）实测：

| 原语 | 修复前 | 修复后 | 倍数 |
|---|---|---|---|
| `Memory.alloc(16)` | **24,492 ns** | **52 ns** | 471× |
| 2 字符字符串拼接 | 24,376 ns | 89 ns | 274× |
| `substring(3,9)` | 24,762 ns | 67 ns | 370× |
| `arrayListOf()` + `add`×3 | **49,383 ns** | 120 ns | 411× |
| `toStr(int)` | 25,079 ns | 99 ns | 253× |
| `Env.get`（= `getenv`） | **12,157 ns** | 6,354 ns | – |
| `list.add(共享串)` | 14 ns | 10 ns | – |

`aura_mem_init()` 在 Windows 分支**先** `getenv("AURA_ARC_DEBUG")`，**后**才
`if (g_aura_mem_limit >= 0) return;` 快返回；而 `aura_mem_alloc` 每次分配要调它
**两次**（函数体开头一次、`g_rgn_active` 判断后又一次）。UCRT 的 `getenv` 是线性扫
environ ⇒ 24 µs/次分配，恰好等于观测到的"每次小分配 ~24 µs"。

**修法**：闸门前置（首次调用仍完整初始化，之后立刻返回）+ 删掉重复的 init 调用。
语义完全不变，只是少了两条 `getenv`。

## 四、根因 R2：substring 先 strlen 整串（O(N·|s|)）

```c
const char *aura_string_substring(const char *s, int64_t start, int64_t end) {
    size_t len = strlen(s);                 // ← 对 2.29 MB 的重定位串也照扫
    if (end > (int64_t)len) end = len;
    ...
}
```

`sliceOf()` 是所有切分原语（`splitSemi` / `splitDoubleSemi` / `splitPipe` /
`splitComma`）的唯一底层切片，于是"大串切 N 刀"= O(N·|s|)。writer 里重定位串
2.29 MB、切 63,692 条，且 `parseRelocEntries` 与 `composeRelocations` 各扫一遍：

| 段 | 修复前 | 修复后 |
|---|---|---|
| `parseRelocEntries`（ext1） | 20,306 ms | **95 ms** |
| `composeRelocations`（coff4b） | 20,516 ms | **340 ms** |

**修法**：源串长度的**单条 memo**（指针相等 + `s[len] == '\0'` 校验），
同一源串连续切 N 刀只算一次 `strlen`。语义与旧实现逐位一致（`end` 的夹取口径不变），
`AURA_SS_MEMO=0` 可回退旧行为。

## 五、验证（产物逐字节一致）

### 5.1 writer 级：离线重放（新增设施）

`ObwReplay.aura` 只重放 writer：读 `build/_obw/dump-<tag>/` 里的
`text.hex` / `relocs.txt` / `consts.txt` / `functions.txt` / `funcoffsets.txt`
（由 `AURA_OBJW_DUMP=<dir>` 在真实自举中转储），跑
`buildSymbols → emitPieces → writeObjectFilePieces`。

同一份输入、**未改动的 writer 源码**，只换运行时：

| | 修复前 | 修复后 |
|---|---|---|
| buildSymbols | 30,872 ms | **218 ms** |
| emitPieces | 100,223 ms | **878 ms** |
| writeObj | 5,544 ms | **694 ms** |
| **合计** | **136,706 ms** | **1,791 ms（76×）** |
| 产物 `.obj` md5 | `4f261810…aa47d` | `4f261810…aa47d` ✅ |

⇒ writer 内部（71 处 `coffAppend`、hex 片段、322,027 个片段）**一行没改**，
时间却降了两个数量级 —— T3 原方案的目标已经被这两条运行时修复提前达成，
再改字节缓冲最多只能再省 ~1 s。

### 5.2 整链自举（**同一时段的对照跑**，机器负载同口径）

同一份编译器源码，只差 `aura_std_cffi.c` 的两条修复：

| 阶段 | **基线**（无修复，drv-t3base2） | **本轮**（修复后，drv-t3h） | 倍数 |
|---|---|---|---|
| link | 63.1 s | 13.0 s | 4.9× |
| ssaBuild | 36.9 s | 28.4 s | 1.3× |
| hatReadParse | 358.3 s | **3.19 s** | **112×** |
| phaseB（Lowering） | 185.5 s | **1.33 s** | **139×** |
| phaseC（ISel） | 262.4 s | **9.23 s** | **28×** |
| ra（寄存器分配） | 37.8 s | 24.0 s | 1.6× |
| emit（X86） | 86.9 s | **7.55 s** | 11.5× |
| **writer（COFF）** | **193.4 s** | **2.22 s** | **87×** |
| objWrite | 8.72 s | 0.64 s | 13.6× |
| backend 合计 | 1,135.5 s | **48.7 s** | 23.3× |
| **wall** | **1,241.9 s** | **92.6 s** | **13.4×** |
| 峰值 WS | 3,822.8 MB | 3,791.4 MB | −0.8% |

`aura-compiler.exe` 无参自检 `BOOT-1 … BOOT-4` 全过、退出码 0。

> 历史基线（R160 记录的 779 s / writer 130.1 s）是另一时段的机器状态；本节两个
> 数字是**同一小时内、同一负载**背靠背跑的，倍数以此为准（绝对值比 R160 那天高，
> 因为本轮跑测期间机器同时在编译）。

### 5.3 产物一致性（同源码，仅运行时修复）

| 产物 | 基线 md5 | 修复后 md5 | |
|---|---|---|---|
| `aura-compiler.hat` (10,962,555 B) | `b486c4399c80a2edb7c04196f147f338` | `b486c4399c80a2edb7c04196f147f338` | **逐字节一致** ✅ |
| `aura-compiler.obj` (4,235,567 B) | `35ec3fbbea1cb48845baf97b39b5263c` | `35ec3fbbea1cb48845baf97b39b5263c` | **逐字节一致** ✅ |
| `aura-compiler.exe` (3,086,336 B) | `4f32f12d…` | `8c7e306a…` | 仅 2 字节不同 |

`.exe` 的 2 字节差异在偏移 129–130（PE 头的时间戳域），`cmp -l` 全文件只报这 2 处 ——
lld-link 写入的构建时间戳，与代码无关。`.obj` 一致 + `BOOT-4` 通过即等价性成立。

**writer 侧另有更强的单点证据**（§5.1）：168 MB→4.2 MB 的同一份 writer 输入，
未改一行 writer 源码，产物 md5 逐字节相同。

## 六、遗留：一个**与 T3 无关**的既有越界读

本轮试图整链验证时，**带 Writer 分段插桩的驱动**在自举前端稳定崩在
`SsaBuilder.csvLineHas` → `aura_lang_std_String_charCodeAt`：

```
CRASH AV at ... kind=0 addr=...001        ← 恰好越过 region 分块末页 1 字节
#0 aura_lang_std_String_charCodeAt+0x3e
#1 aura_user_SsaBuilder_csvLineHas+0xdb
#2 aura_user_SsaBuilder_synthesizeCallStubs+0xc7
#3 aura_user_SsaBuilder_build+0x423
#4 aura_user_SsaBuilderUtils_build+0x2ba
```

- `charCodeAt` 的 C 实现**不做上界检查**（靠"读到 NUL 返回 -1"），撞上
  region 分块末页时就是 AV；
- `csvLineHas` 的循环上界是 `csv.length`（strlen），只要求 `csv` 本身有效；
- **对照实验**：用**未修改的运行时**（drv-t3a，早前跑通过同一个负载）重跑，
  同样崩在同一处 ⇒ 这是**既有的、布局敏感的**潜伏缺陷，不是本轮引入的；
  去掉我加的插桩（改变分配布局）后同一份源码就能跑完。

⇒ 结论：插桩代码本身只多了几十个字符串分配，却足以把 region 布局推到
这个边界。**该缺陷建议单独立项**（候选修法：给 `charCodeAt` 加上界校验 /
给 region 分块尾部留哨兵页 / 排查 `csvLineHas` 引用的串是否在
`ScratchRewind` 后悬垂）。

据此，本轮已把 `PhotonObjectWriter` / `PhotonPipeline` 的插桩**回退**
（补丁存 `build/_t3_instr.patch`），主干只保留两条运行时修复。
分段打点需要时用 `git apply build/_t3_instr.patch` 恢复。

## 七、本轮新增/改动的文件

| 文件 | 作用 |
|---|---|
| `seed/compiler/src/std/cffi/aura_std_cffi.c` | **R1** `aura_mem_init` 闸门前置 + 删重复 init；**R2** `aura_string_substring` 走长度 memo（`AURA_SS_MEMO=0` 回退） |
| `aura/photon/aura/lang/compiler/photon/ObwReplay.aura` | 新增：writer 离线重放器（诊断用，不被编译器 import；输入取自 `build/_obw/dump-t3a/`） |
| `build/_obw/ObwBench.aura` | 新增：运行时原语微基准（分配/拼接/substring/到 str） |
| `build/run-bootstrap-t3.ps1` | 新增：自举跑测 + `[time]`/`[wtime]` 提取 + 产物 md5 |
| `build/_t3_instr.patch` | writer/pipeline 分段打点 + `AURA_OBJW_DUMP` 转储钩子（本轮已回退，`git apply` 可恢复） |

> 为什么插桩要回退：见 §6。插桩只是几十个字符串分配，却足以把 region 布局推到
> 那个既有越界读的边界上。诊断能力用 `build/_t3_instr.patch` + 已采集的
> `dump-t3a/` 保留，主干保持干净。

## 八、下一轮建议（时间优化的新排序）

修复后各阶段占比彻底变了（数字取修复后那次）：

| 阶段 | 现耗时 | 占 backend |
|---|---|---|
| ssaBuild | 28.4 s | 58% |
| ra（寄存器分配） | 24.0 s | 49% |
| link | 13.0 s | – |
| phaseC（ISel） | 9.2 s | 19% |
| emit | 7.6 s | 16% |
| hatReadParse | 3.2 s | – |
| writer | 2.2 s | 4.5% |
| phaseB | 1.3 s | 2.7% |

- **T4（新首选）：ssaBuild 28.4 s + ra 24.0 s = backend 的 2/3**。这两项本轮
  只享受了"分配变快"的间接收益（1.3× / 1.6×），说明它们的成本在**算法/数据结构**
  而不是分配次数 —— 下一个 10 万行级的收益点。`ra` 尤其可疑：寄存器分配在
  37.8 → 24.0 s 之后几乎不动，说明它有自己的热点（而不是分配开销）。
- **T2（`.hat` 二进制）降级**：hatReadParse 已从 198 s → 3.2 s，占比从 27% 掉到 3%。
- **T3（writer 字节缓冲）关闭**：目标已达成（193 s → 2.2 s），再改只剩 1 s 量级。
- 另建议把 §6 的越界读单独立项 —— 它现在是**任何源码改动都可能触发的**稳定性风险，
  比再省几秒更重要。

## 九、2026-10-22 复测（当前主干，两条运行时修复已在内）

驱动重建（`build/build-driver.ps1 -Tag drv-r163`）：**30.6 s**，产物 1,763,328 B。

同一负载（`Main.aura` → `aura-compiler.exe`）连跑三次：

| 指标 | 值 |
|---|---|
| **wall** | **87.9 s**（另两次 88.7 s / 108.2 s〔开 `AURA_MEM_STATS=1`，多付链路诊断开销〕） |
| **backend** | **45.9 s** |
| **峰值工作集（WS）** | **3,827.9 MB** |
| 峰值提交（Pagefile/Private） | 3,935.9 MB |
| 峰值虚拟地址 | 8,380.2 MB |
| region 高水位 | `retain=3714 MB`（29 个 128 MB 块，`scratch=0`） |
| 产物 | `.hat` 10,962,555 B / `.obj` 4,235,567 B / `.exe` 3,086,336 B |
| md5 | `.hat` `b486c439…`、`.obj` `35ec3fbb…` —— **与修复前逐字节一致**，三次跑完全相同 |

**内存结论（诚实）**：两条修复是**纯 CPU 路径修复**，峰值内存基本不动
（基线 3,822.8 MB → 3,827.9 MB，+0.1%）。WS 的构成仍是 region arena
（`retain 3714 MB`，AOT 无 GC 全驻留），本轮没有改善它 —— 那是 T1(R160) 与
P4 的战场，不在本轮的靶点上。

阶段明细（`AURA_PHOTON_TIME=1`，wall 88.7 s 那次）：

| 阶段 | 耗时 | 归属 |
|---|---|---|
| **ssaBuild** | **27.40 s** | 前端 HIR→SSA |
| **ra（寄存器分配）** | **22.04 s** | 后端 |
| **link（模块链接）** | **13.09 s** | 前端 |
| phaseC（ISel） | 8.65 s | 后端 |
| emit（X86） | 7.08 s | 后端 |
| hatReadParse | 3.02 s | 后端 |
| writer（COFF） | 2.94 s | 后端 |
| phaseB（Lowering） | 1.09 s | 后端 |
| hatSerialize / hatWrite / getterPatch / objWrite | 1.03 / 0.39 / 0.16 / 0.66 s | |
| backend 合计 | 45.89 s | |

⇒ **前端 ≈ 42.8 s（wall − backend）已经与后端 45.9 s 相当**，且前端内部
`ssaBuild 27.4 s` 是当前全流程的第一大项。下一轮的靶点应更新为
**ssaBuild → ra → link**（三者合计 62.5 s，占 wall 的 71%）。
