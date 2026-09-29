# Photon 编译器性能优化方案

> **定位**：本文是对「Photon 编译耗时长」的根因分析与优化路线。
> 与 `docs/photon/photon-mem-analysis.md`（聚焦**内存**）互补：本文聚焦**时间**，
> 但两者共享同一批根因——Aura AOT 运行时无 GC（`free_calls=0`），
> 任何不可变字符串拼接的中间产物都会永久驻留，因此**内存优化项与时间优化项高度重叠**。
>
> **状态时间点**：2026-09-29，`git log` 最新提交 `e207d9b`（完成 photon P0–P4 开发）。
> 本文所有结论均基于源码静态阅读 + 源码注释里作者自测过的实测数字，**未执行任何修改**。

---

## ✅ 实施状态（2026-09-29 更新）

### P0 — Rust VM 层（全部完成，`cargo build --release --features llvm` 通过 ✓）

| 项 | 文件 | 状态 | 说明 |
|---|---|---|---|
| P0.1 | `interp.rs:1585, 1784` | ✅ | `eprintln!` 套 `trace_call_enabled()` 守卫 |
| P0.2 | `InstructionSelection.aura:107` | ✅ | `forceReachable` 10→5 项 |
| P0.3 | `interp.rs` | ✅ | `BytecodeNative.clone()` → 字段提取 |
| P0.4 | `mod.rs` + `interp.rs` | ✅ | `Instr` 加 `Copy` + 去 `.clone()` |
| P0.5 | `interp.rs` (5 处) | ✅ | `.cloned()` → `&Value::Ref(h)` |
| P0.6 | `interp.rs:1997` | ✅ | `pop()` clone 移入错误分支 |

### P1 — Aura 后端层（全部完成）

| 项 | 文件 | 状态 | 说明 |
|---|---|---|---|
| P1.1 | `PhotonPipeline.aura` | ✅ | `traceMark` 有界截断（O(n²)→O(1)） |
| P1.2 | `PhotonPipeline.aura` | ✅ | `phirEnvSet` 快路径（新变量不重建） |
| P1.3 | `MachineDag` + `InstructionSelection` | ✅ | `Env.get` 缓存为字段 |
| P1.4 | `MachineDag.aura` | ✅ | `roots`/`chains` → `List<Int>` |
| P1.5 | `RegisterAllocator.aura` | ✅ | `callStack` → `List<Int>` |
| P1.6 | `InstructionSelection.aura` | ✅ | `phiMovByBlock` → `List<List<String>>` |
| P1.7 | `PhotonPipeline.aura` | ✅ | `buildRuntimeObject` 文件缓存 |

### P2 — 中间层

| 项 | 状态 | 说明 |
|---|---|---|
| P2.3 | ✅ | Phase 边界释放点（SSA/LIR 置空） |
| P2.1 | ⏳ 后续 | HIR.kids → `List<Int>`（283+ 引用） |
| P2.2 | ⏳ 后续 | LIR.blocks/instrs/args → `List<Int>` |

### P3 — 架构级

| 项 | 状态 | 说明 |
|---|---|---|
| P3.1 | ⏳ 后续 | 后端移出 VM（需独立编译流程） |
| P3.2 | ⏳ 后续 | arena allocator（需新增原生函数） |
| P3.3 | ⏳ 后续 | 对象字段 HashMap→Vec（影响全 VM） |

---

## 〇、一句话结论

Photon 慢是**三层叠加**，且最大单一嫌疑点**不在 `aura/photon/` 目录里**：

1. **VM 层（Rust 解释器）** —— 每条字节码 clone Instr、每次原生调用 clone 描述符、
   **每次 stdlib-aura 派发无条件 `eprintln!`**。`photon-mem-analysis.md` §八 列为 P0 的五项
   **全部未落地**。
2. **Aura 后端层（Phase A–E）** —— 大量「逗号分隔字符串当容器」，每次追加整段 O(n) 拷贝；
   作者已在注释里用实测数字标出多处平方级 / 立方级退化，**已修一批、仍剩一批**。
3. **调试设施自身** —— `traceMark` 每条追加 + 整文件重写（O(行数²)），`Env.get` 出现在 69 处循环里。

---

## 一、头号嫌疑：VM 层 P0 五项全未落地

### 1.1 逐项核对

`docs/photon/photon-mem-analysis.md` §八.6.1–6.5 列了五项 P0 优化。当前代码状态：

| # | 优化项 | 状态 | 代码位置 / 证据 |
|---|---|---|---|
| 6.1 | 去掉热路径无条件 `eprintln!` | ❌ **未做** | `seed/compiler/src/vm/interp.rs:1575` 与 `:1771` 的 `eprintln!("[vm] stdlib-aura: …")` 仍**无条件**执行 |
| 6.2 | `natives[idx].clone()` → 借用 | ❌ 未做 | `interp.rs:1455`、`:1665` 仍 clone `Vec<u8>` + `Option<String>` + `String` |
| 6.3 | `pop()` 的 func_name clone 移到错误路径 | ❌ 未做 | `interp.rs:1957-1966` 每次栈弹出都 clone 一次函数名 |
| 6.4 | `const_to_value()` 消除双重字符串分配 | ❌ 未做 | `interp.rs:2313-2320` |
| 6.5 | `IncRef/DecRef/DropRef` 的 `.cloned()` → `.copied()` | ❌ 未做 | `interp.rs:442-458` |
| — | 每条指令 `code[ip].clone()` | ❌ 未做 | `interp.rs:147` |

### 1.2 为什么 6.1 是头号嫌疑

关键细节：**`trace_call_enabled()` 已经存在**（`interp.rs:2249`，默认关闭），
并且在 `interp.rs:1469` 与 `:1670` **已被正确使用**——但**没被套到 `:1575` 和 `:1771` 这两处**。
也就是说，作者知道这个开关，只是漏了这两个调用点。

`do_call_native` 的派发路径：

```rust
// interp.rs:1570-1578
let std_lookup = self.find_stdlib_func(&native.name, param_count).filter(|&(idx, _)| {
    let func = &self.module.funcs[idx];
    !func.is_native
});
if let Some((std_func_idx, needs_self)) = std_lookup {
    eprintln!(                                              // ← 无条件！
        "[vm] stdlib-aura: {} → Aura compiled func #{} (self={})",
        native.name, std_func_idx, needs_self
    );
    ...
}
```

`do_call_native_args`（`:1770-1774`）是同款代码的第二份拷贝。

**命中频率**：Photon 后端跑在 VM 里，而它大量调用 `mutableListOf` / `String.split` /
`Collections.set` / `String.indexOf` 等 stdlib-aura 函数——每一次调用都命中这两个分支。
编译器自举一次要跑 50–100 万次这类调用，每次构造 format buffer 写入 stderr。

**源码里已经记录过同源事故**（`interp.rs:13-16`，`warn_unlinked_once` 函数注释）：

> 「原实现每次调用都 `args.iter().map(|v| v.to_string())`：当实参里含大列表时，
> 单条日志就要构造数百 KB 字符串。实测 4 万次 `list.get(i)`（裸名 `get` 未注册 →
> 走本兜底）峰值内存 **23GB**、耗时超过一分钟——「未链接告警」自己变成了 OOM 元凶。」

`warn_unlinked_once` 修复了**未链接函数**的告警，但 stdlib-aura 派发路径上的这两处
`eprintln!` 是同一类病、从未被修。

### 1.3 验证方法（不需要改代码）

```powershell
# 方法 1：measure.exe 看 stderr 写入量
measure.exe -Timeout:900 -ShowCounters .\build\hat-native\PhotonHatCompile.exe `
  > out.txt 2> err.txt
Get-Item out.txt, err.txt | Select Name, @{N='MB';E={[math]::Round($_.Length/1MB,2)}}
# err.txt 达几十 MB 即为命中

# 方法 2：PowerShell 计时 + 内存峰值
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p  = Start-Process .\build\hat-native\PhotonHatCompile.exe -PassThru -NoNewWindow `
        -RedirectStandardOutput out.txt -RedirectStandardError err.txt
$p.WaitForExit(900000) | Out-Null; $sw.Stop()
"elapsed=$($sw.Elapsed.TotalSeconds)s  err=$(Get-Item err.txt | %{$_.Length})"
```

---

## 二、Phase C：`forceReachable` 清单与注释自相矛盾

`InstructionSelection.aura:95-101`：

```
// ⚠️ 清单本身也要克制：这里每加一个名字，都会连带拉进它的传递闭包……
// 实测把 Ast_add/Hir_add/Mir_add/ArrayList_add/HashMap_keys/EnvOps_keys 一起加进来后，
// 自举耗时 43s → 265s、Main.obj 1.11MB → 1.73MB —— 收益为零（add/keys 的调用点
// 仍未被映射，仍是未定义），代价却是 6 倍编译时间。
// 只保留确实解决未定义的三个（且都是叶子小函数）。
private var forceReachable: String =
    "MirUtils_mirNoKids,MirUtils_mirKidsAdd,Linearizer_mirNoKids,Linearizer_mirKidsAdd," +
    "FileUtils_mkdir,ArrayList_add,HashMap_keys,Ast_add,Hir_add,Mir_add"
```

**问题**：注释说"只保留三个"，实际清单 **10 项**，其中包含注释自己点名
"收益为零、代价 6 倍"的 `ArrayList_add` / `HashMap_keys` / `Ast_add` / `Hir_add` / `Mir_add`
共 **5 项**。每个名字都会拉进整个传递闭包（它们调用的函数、再下一层……）。

### 修复

把清单砍到注释建议的 3 项（保留注释确认"确实解决未定义"的叶子小函数）：

```
private var forceReachable: String =
    "MirUtils_mirNoKids,MirUtils_mirKidsAdd,Linearizer_mirNoKids"
```

> ⚠️ 砍完若出现新 `undefined symbol`，按"哪个名字被加回来就带来哪批未定义"逐个补回，
> 不要一次加回全部 5 项——注释明确说过这些名字的调用点映射本来就没修好。

---

## 三、各阶段已标注的实测瓶颈（作者自己量过的数字）

以下全部来自源码注释，是作者边修边测时记录的真实数字。

| Phase | 位置 | 实测数字 | 当前状态 |
|---|---|---|---|
| A 解析 | `PhotonPipeline.aura:1063-1085` | 旧 `phirSigLookup`："每 34 个函数产生 117 万次分配，内存冲向 4 GB 后 OOM" | ✅ 已改 `indexOf` |
| A 解析 | `:1038` `phirEnvSet` | 每次声明**重建整个环境串** → 平方级 | ❌ **未改** |
| A→B | `Lowering.aura:167` | 旧实现"2647 函数 × 14 万值 = 4×10⁸ 次循环 + 10⁹ 次小字符串分配" | ✅ 已改区间遍历 |
| C | `InstructionSelection.aura:92` | 旧后缀匹配回退"43s → **394s**、内存破 10 GB" | ✅ 已回退为显式清单 |
| C | `:112` | 旧 `stringConstants = s + "," + lit` "Phase C 吃掉 20 GB" | ✅ 已改分块归并 |
| D | `RegisterAllocator.aura:737` | `splitComma` 被调 **41,424,177 次**、连带 **402,152,493 次** `aura_substr_dup` | ✅ 已改首尾逗号 + `indexOf` |
| D | `:98` | 旧 `labelIndexMap` 四步拼接链**各占 1551 MB ⇒ 6.2 GB（占总分配 78%）** | ✅ 已改 List |
| D | `:64-67` | 旧 `spilled` 逗号串 ≈ 10 GB，`aura_string_concat` 占 92% | ✅ 已改 List |
| D | `:107` | 旧 `nodeSpan` + `splitNewline` → **O(节点数³)**，2831 节点下 ~4×10¹⁰ 次分配 | ✅ 已改按下标直存 |
| E | `PhotonObjectWriter.aura:1101` | 旧 `chEq` 内取 `s.length`（AOT 下是 `strlen`）→ **`composeRelocations` 单次 84 秒（被调两次）、`collectExternalSymbols` 43 秒，COFF 组装合计 212 秒** | ✅ 已改 `charCodeAt` |
| E | `:1090` | 旧 `s[i]`（AOT 下 `aura_dup_n` 新建单字符串）全管线 **累计 3.67 亿次分配 / 963 MB 常驻** | ✅ 已改 `charCodeAt` |
| E | `:1121` | 旧 `sliceOf` 逐字符拼接 O(L²) → 17,066 重定位 × 2,647 符号，Phase E 单独吃 **~7 GB** | ✅ 已改 `substring` |
| C | `MachineDag.aura:429` | `patternCache`："到第 405 个函数（~3.7 万条指令）时已吃进 ~2 GB" | ✅ 已**刻意 no-op** |

**读表要点**：E 的三项和 D 的四项**已经改完**（改成 `charCodeAt` / `substring` / `List`），
说明作者确实在系统性地做这类优化。**但 Phase A 的 `phirEnvSet` 和 Phase C 的
`forceReachable` 被漏掉了**——代码和注释打架，正是本文要指出的两个具体缺口。

---

## 四、仍残留的"逗号分隔字符串当容器"

**已修**：`colors` / `spilledNodes` / `labelIndexIdxs` / `spanFirst` / `spanLast` /
`backEdges` / `nodeMapIds` / `stringConstChunks` / `availDelim`。

**仍残留**（同一类病，应照同样方法改掉）：

| 位置 | 字段 | 追加方式 |
|---|---|---|
| `MachineDag.aura:445-460` | `roots` / `chains` | `this.roots = this.roots + "," + toStr(nodeId)` |
| `RegisterAllocator.aura:72` | `callStack` | DFS 栈，逗号分隔 nodeId |
| `RegisterAllocator.aura:81` | `frameLayout` | `"spillSlot\|offset\n"` |
| `InstructionSelection.aura:929` | `phiMovByBlock` | `blockId → "phiVid,incomingVid\n"` |
| 整个 HIR | `kids` | `HirUtils.hirKidsAdd(list, child)` 返回新串 |
| 整个 LIR | `blocks` / `instrs` / `args` | 逗号分隔 |

### 根因

Aura AOT 运行时**无 GC**（源码注释里反复出现 `free_calls=0`）。因此：

- 任何 `s + "," + x` 都会 `malloc` 一个新串，旧串**永不释放**；
- 循环里每次追加的成本是 `O(当前串长)`，累积成 `O(n²)` 分配量。

### 修法

照 `RegisterAllocator.colors` 的既成模式（`RegisterAllocator.aura:53-59`）：
**键是整数下标时直接用 `List<T>`**，字符串形态只在导出时按需拼一次。

> 注意：HIR 的 `kids` 是最难改的——它是跨文件共享的数据结构（`Hir.aura` 被前端、
> SSA、Lowering、指令选择、发射器全部消费）。改它要一次性把
> `hirKidsAdd` / `hirKidsCount` / `hirKidsAt` / `hirKidsConcat` / `hirKidsPrepend` /
> `hirKidsReplace` 全改成 `List<Int>` 上的操作，并改 `Hir.kids: List<String>` 字段。
> 建议放在最后一批做。

---

## 五、探针设施本身也是分配源

### 5.1 `traceMark` 仍是 O(行数²)

`PhotonPipeline.aura:196-202`：

```aura
private fun traceMark(msg: String): Unit {
    if (Env.get("AURA_PHOTON_TRACE") != "1") { return }
    this.traceBuf = this.traceBuf + msg + "\n"              // ← 整段拷贝
    FileUtils.writeText(this.outDir + "/photon_trace.log", this.traceBuf)  // ← 整文件重写
}
```

Phase C 的 `selProbe`（`InstructionSelection.aura:479-486`）、Phase D 的 `raProbe`
（`RegisterAllocator.aura:709-723`）、Phase E 的 `peProbe`
（`PhotonPipeline.aura:220-234`）**都已经改成有界截断**（只保留尾部 2.5KB，
每次重写一个 ≤3KB 小文件），作者自己也写了注释说明为什么。

**唯独 `traceMark` 没改**。它被 `parsePhirText` 里每 20–25 个函数调一次
（`:931-939`）、`compileHir` 的每个 Phase 调一次（`:273, 289, 305, 316, 359`）。

**修法**：照抄 `peProbe` 的实现（`:220-234`），把 `traceMark` 改成有界截断。

### 5.2 `Env.get` 出现在 69 处循环里

`PhotonPipeline.aura:178-180` 作者自己写了警告：

> ⚠ 必须在入口处算一次并缓存到字段：`Env.get` 每次调用都要重新读一遍 environ 缓冲
> （`EnvOps.readEnviron` + `Allocator.free`），放进循环是持续分配源。

作者也确实照此做了（`verboseOn` 字段 + `readVerboseFlag()`），但仍有若干处
`Env.get` 在循环里：

- `PhotonPipeline.aura:924` — `parsePhirText` 的函数循环里每函数一次
  `Env.get("AURA_PHOTON_SKIPFUN")`
- `MachineDag.aura:352, 380` — `addInstr` / `selectPattern` 里每条指令一次
  `Env.get("AURA_PHOTON_TRACE")`
- `InstructionSelection.aura:904` — `selectBlock` 里每块一次

**修法**：统一提升到构造函数里读一次缓存成字段。

### 5.3 `PhotonRuntime.buildRuntimeObject()` 每次编译都重新生成

`PhotonPipeline.aura:440, 787`：runtime 对象（180KB 源码）**每次编译都重新走一遍
代码生成 + 落盘**，没有任何缓存。runtime 内容对给定平台是常量，应当缓存为
静态 hex 文件，首次生成后直接 `writeObjectFile` 已有文件。

---

## 六、结构性根因（无法靠局部优化消除）

| # | 根因 | 影响 | 解法 |
|---|---|---|---|
| 1 | **AOT 运行时无 GC**（`free_calls=0`） | 所有不可变拼接的中间产物永久驻留；无 GC 也让**时间**优化与**内存**优化强耦合——分配量即耗时 | 只能把所有累加容器换成 `List<T>`；长期方案见 `photon-mem-analysis.md` §8.6.10 的 arena allocator |
| 2 | **VM 解释执行后端** | 每个 Phase 都跑数百万条 VM 指令，每条 `clone` Instr + `pop` 时 `clone` func_name | `interp.rs:147` 改借用；长期把后端移出 VM（`photon-mem-analysis.md` §8.6.13） |
| 3 | **单进程全量串行** | Phase A→E 全在一个 `PhotonPipeline` 实例里；`MachineDag` / `InstructionSelector` 整个模块共用、永不回收（`addNode` / `addInstr` 从不回退计数） | 阶段边界插"释放点"：Phase A 完成后 drop HIR，Phase B 完成后 drop SSA，依此类推 |
| 4 | **`traceMark` / `Env.get` 类设施** | 诊断代码本身成为热路径开销 | §五 已列 |

---

## 七、优化路线（按投入 / 产出排序）

### P0 —— 立即做（投入 < 2 小时，预计消除主要瓶颈）

| # | 改动 | 文件 / 行 | 预期收益 |
|---|---|---|---|
| P0.1 | `eprintln!` 套 `trace_call_enabled()` | `interp.rs:1575, 1771` | **头号嫌疑**。消除已记录的 23 GB 同源事故；减少 50–100 万次 stderr I/O |
| P0.2 | `forceReachable` 砍掉 5 个零收益项 | `InstructionSelection.aura:101` | 注释自测 **6 倍编译时间**（43s → 265s） |
| P0.3 | `natives[idx].clone()` → `&self.module.natives[idx]` | `interp.rs:1455, 1665` | 减少数十万次 `Vec<u8>` + `String` clone |
| P0.4 | `code[ip].clone()` → `&self.module.funcs[func].code[ip]` | `interp.rs:147` | 减少数百万次 Instr clone（~16 B/次 + 分支预测污染） |
| P0.5 | `IncRef/DecRef/DropRef` 的 `.cloned()` → `.copied()` | `interp.rs:442-458` | `Value::Ref(usize)` 是 `Copy`，直接消除克隆 |
| P0.6 | `pop()` 的 func_name clone 移到错误路径 | `interp.rs:1957-1966` | 消除约 500 万次无效 String clone |

> **P0 全部完成后才量第一次基线**——前面几项不测，后面所有收益数字都会失真
> （P0.1 一项就可能让"慢"从分钟级变成秒级）。

### P1 —— 短期（投入 0.5–1 天，消除已量化的平方级退化）

| # | 改动 | 文件 / 行 | 依据 |
|---|---|---|---|
| P1.1 | `traceMark` 改有界截断 | `PhotonPipeline.aura:196-202` | 照抄 `peProbe`（`:220-234`） |
| P1.2 | `phirEnvSet` 改按名索引（不再重建整串） | `PhotonPipeline.aura:1038` | 当前 O(环境大小) × 每个声明 = 平方级 |
| P1.3 | 循环里的 `Env.get` 提升到构造 | `PhotonPipeline.aura:924`、`MachineDag.aura:352, 380`、`InstructionSelection.aura:904` | 作者自己在 `:178-180` 警告过 |
| P1.4 | `MachineDag.roots` / `chains` 改 `List<Int>` | `MachineDag.aura:445-460` | 与已 no-op 掉的 `patternCache` 同款病 |
| P1.5 | `RegisterAllocator.frameLayout` / `callStack` 改 `List` | `RegisterAllocator.aura:72, 81` | 同族 |
| P1.6 | `InstructionSelection.phiMovByBlock` 改 `List` | `InstructionSelection.aura:929` | 同族 |
| P1.7 | `PhotonRuntime.buildRuntimeObject()` 结果缓存为静态 hex | `PhotonPipeline.aura:440, 787` | runtime 对给定平台是常量，不该每次重算 |

### P2 —— 中期（投入 2–5 天，需重构数据结构）

| # | 改动 | 说明 |
|---|---|---|
| P2.1 | HIR 的 `kids: List<String>` → `List<Int>` 数组 | 跨文件共享，影响 `Hir.aura` + SSA + Lowering + 指令选择 + 发射器；`hirKidsAdd/Count/At/Concat/Prepend/Replace` 全部改写。**建议放最后一批做** |
| P2.2 | LIR 的 `blocks` / `instrs` / `args` 改 `List` | 与 P2.1 同款 |
| P2.3 | Phase 边界插释放点 | Phase A 完成后 drop HIR；Phase B 完成后 drop SSA；依此类推。需要确认 Aura VM 的 ARC 语义真的会回收（`photon-mem-analysis.md` §8.6.14 提到阶段式 arena 批量回收） |
| P2.4 | 阶段并行化 | 函数级并行（Phase C/D/E 对函数间无依赖）——需要先做 P2.3 保证内存不炸 |

### P3 —— 长期（投入 2–4 周，架构级）

| # | 改动 | 依据 |
|---|---|---|
| P3.1 | 后端移出 VM，改为 Rust 原生管线 | `photon-mem-analysis.md` §8.6.13，预期内存降 5–10× |
| P3.2 | 引入 arena allocator 管理 VM 堆 | `photon-mem-analysis.md` §8.6.10 |
| P3.3 | 对象字段 `HashMap<u16, Value>` → `Vec<(u16, Value)>` | `photon-mem-analysis.md` §8.6.6，空间降 60–70% |

---

## 八、诊断手段（代码内建，不需要改代码即可定位）

### 8.1 阶段二分（最该先跑的一步）

`PhotonPipeline.aura:173, 283`：

```powershell
$env:AURA_PHOTON_STOP="A"   # 只看 Phase A（HIR → SSA）耗时
$env:AURA_PHOTON_STOP="B"   # + Phase B（SSA → LIR）
$env:AURA_PHOTON_STOP="C"   # + Phase C（LIR → DAG）
$env:AURA_PHOTON_STOP="D"   # + Phase D（RegAlloc + Peephole）
```

每次单独计时，就能把总时间切成 5 段。

### 8.2 单阶段定位探针（有界截断，落盘不刷 stdout）

| 环境变量 | 输出 | 覆盖 |
|---|---|---|
| `AURA_SEL_PROBE=1` | `<out>/isel_probe.log` | Phase C 指令选择 |
| `AURA_RA_PROBE=1` | `<out>/ra_probe.log` | Phase D 寄存器分配 |
| `AURA_PE_PROBE=1` | `<out>/pe_probe.log` | Phase E 编码 → COFF → 链接 |
| `AURA_XE_PROBE=1` | `<out>/xe_probe.log` | X86Emitter 细节 |
| `AURA_MEM_STATS=1` | 内存归因统计 | 分配热点 + `free_calls` 计数 |
| `AURA_MEM_LIMIT_MB=20480` | 内存闸门 | 触顶退出码 70 |

> ⚠️ 这组探针**只能定位"卡在哪一步"**，不能直接给出"这一步慢在哪"。
> 对 Phase A 需要额外的手段（见下）。

### 8.3 Phase A 专用（当前无内建探针）

Phase A 的 `parsePhirText` 已经埋了计数器（`dbgEnvGet` / `dbgEnvSet` / `dbgSigCalls` /
`dbgSigScanned`，`PhotonPipeline.aura:183-189`），并在每 25 个函数打点一次
（`:931-939`）。但这些点走的是 `traceMark`（需要 `AURA_PHOTON_TRACE=1`，
而那个开关一旦开启就同时打开 O(行数²) 的整文件重写——见 §5.1）。

**建议**：把这几行 `traceMark` 改成 `peProbe` 式的有界截断，
让 Phase A 也能用上探针而不误伤计时。

### 8.4 VM 侧确认（验证头号嫌疑）

```powershell
# 看 stderr 写入量——几十 MB 即为命中 interp.rs:1575
$env:AURA_VM_TRACE_CALL="split"   # 只用已有开关看单类调用，不要全开
```

---

## 九、验收口径

每项改动前后各跑一次，记录三个数：

```powershell
# 基线命令（与 HANDOFF.md §0 一致）
$env:AURA_HAT_AURA  = "D:\Code\AuraLang\aura\compiler\aura\lang\compiler\Main.aura"
$env:AURA_HAT_OUT   = "build\bmain"
$env:AURA_HAT_MODULE= "Main"

# 计时 + 内存峰值
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p  = Start-Process .\build\hat-native\PhotonHatCompile.exe -PassThru -NoNewWindow `
        -RedirectStandardOutput build\_out.txt -RedirectStandardError build\_err.txt
$p.WaitForExit(1800000) | Out-Null; $sw.Stop()

# 记录：
#   1) $sw.Elapsed.TotalSeconds        ← 总耗时
#   2) (Get-Item build\_err.txt).Length ← stderr 字节数（P0.1 的直接指标）
#   3) 峰值内存                        ← 用 measure.exe 或 Get-Process 采样

# 回归：产物必须仍能链接且可运行
& <LLVM>\bin\lld-link.exe build\bmain\Main.obj build\bmain\aura_runtime.obj `
  /OUT:build\bmain\Main.exe /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB `
  "C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64\kernel32.lib"

powershell -File scripts\photon\photon-hat-native-suite.ps1 -Phase P1,P2,P3
# 期望：PASS=15 FAIL=0（HANDOFF.md 第十轮基线）
```

> ⚠️ 运行自举产物**必须加超时**（`WaitForExit` + `Kill`）——历史上出现过单核占满的死循环
> （`HANDOFF.md` §0「操作禁忌」）。

---

## 十、附录：关键位置速查

| 项 | 文件 : 行 |
|---|---|
| VM 主循环无指令上限 | `seed/compiler/src/vm/mod.rs:1494-1496` |
| 每条指令 clone Instr | `seed/compiler/src/vm/interp.rs:147` |
| 无条件 `eprintln!`（头号嫌疑） | `seed/compiler/src/vm/interp.rs:1575, 1771` |
| `natives[idx].clone()` | `seed/compiler/src/vm/interp.rs:1455, 1665` |
| `IncRef/DecRef` 的 `.cloned()` | `seed/compiler/src/vm/interp.rs:442-458` |
| `pop()` clone func_name | `seed/compiler/src/vm/interp.rs:1957-1966` |
| `trace_call_enabled`（已有开关，未套用到 1575/1771） | `seed/compiler/src/vm/interp.rs:2249` |
| `forceReachable` 清单 | `aura/photon/.../InstructionSelection.aura:101` |
| `computeReachable` | `aura/photon/.../InstructionSelection.aura:369` |
| `traceMark`（O(行数²)） | `aura/photon/.../PhotonPipeline.aura:196-202` |
| `peProbe`（有界截断的模板） | `aura/photon/.../PhotonPipeline.aura:220-234` |
| `phirEnvSet`（重建整串） | `aura/photon/.../PhotonPipeline.aura:1038` |
| `phirSigLookup`（已修，可参考） | `aura/photon/.../PhotonPipeline.aura:1074` |
| `MachineDag.roots` / `chains` | `aura/photon/.../MachineDag.aura:445-460` |
| `MachineDag.patternCache`（刻意 no-op） | `aura/photon/.../MachineDag.aura:410-440` |
| `RegisterAllocator.frameLayout` / `callStack` | `aura/photon/.../RegisterAllocator.aura:72, 81` |
| `RegisterAllocator.colors`（已修，可参考） | `aura/photon/.../RegisterAllocator.aura:53-59` |
| `InstructionSelection.phiMovByBlock` | `aura/photon/.../InstructionSelection.aura:929` |
| `InstructionSelection.stringConstChunks`（已修，可参考） | `aura/photon/.../InstructionSelection.aura:114` |
| `PhotonObjectWriter.chEq` / `sliceOf`（已修，可参考） | `aura/photon/.../PhotonObjectWriter.aura:1093, 1128` |
| `HirUtils.hirKidsAdd` | `aura/compiler/.../hir/Hir.aura:111` |
| 阶段短路开关 `AURA_PHOTON_STOP` | `aura/photon/.../PhotonPipeline.aura:282-347` |
| `buildRuntimeObject` 每次重算 | `aura/photon/.../PhotonPipeline.aura:440, 787` |

---

## 十一、与既有文档的关系

| 文档 | 聚焦 | 关系 |
|---|---|---|
| `docs/photon/photon-mem-analysis.md` | **内存**（1 GB 基线 + 20 GB 极端） | 姊妹篇。本文 §一 直接引用它的 §八 P0 清单；本文 §六 第 1 项引用它的 §8.6.10 / 6.13 |
| `docs/photon/photon-compiler-bootstrap-analysis.md` | 自举链路设计与差距 | 本文不覆盖功能正确性，只覆盖性能 |
| `docs/photon/HANDOFF.md` | 自举逐轮交接（含构建命令、崩溃定位） | 本文 §九 的验收命令与其 §0 一致 |
| `docs/photon/Photon后端问题修复方案.md` | 功能 bug 修复 | 正交 |

**关键交叉引用**：`photon-mem-analysis.md` 的 P0 五项（§8.6.1–6.5）在本文 §1.1 已逐项核对，
**五项全部未落地**。这是当前性能与内存两大问题的共同起点。
