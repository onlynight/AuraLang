# Photon 编译器多线程化设计方案

> 状态：**设计评审稿，未实施**。本文档基于对代码库的结构性分析，不改任何代码。
>
> 核心结论：**多线程不是这个编译器的瓶颈解药**。它应该作为第三优先级的选项，而不是主抓手。

---

## 一、范围界定与时间分布

### 1.1 本次讨论的范围

**Photon 编译后端的 LLVM IR 路径保持现状，不在本方案的改造范围内**。

- Photon 前端（源码 → HIR → SSA → `.hat`）：本方案讨论的改造对象。
- Photon 后端（`.hat` → COFF → exe，即 Phase B–E）：**保持现状**，不引入多线程改造。
- 遗留的 LLVM-IR 路径（`aura/compiler/aura/lang/compiler/aot/Aot.aura`）：**保持现状**，其内部 `llc`/`clang` 的进程级并行也不纳入。
- 因此本文档中所有耗时数据均指**前端部分**，不含 `compileHat` 与最终链接。

这样界定之后，**前端的 131.57 s（SSA 阶段 95.3%）就是唯一值得追的目标**，而且它恰好全部落在 `SsaBuilder` 里，不涉及任何后端改动。

### 1.2 时间花在哪

自举编译 `aura/compiler/aura/lang/compiler/Main.aura`（111 模块 / ~15 万 HIR 节点 / 3000+ 函数）的实测分段，仅计前端：

| 阶段 | 耗时 | 占比 | 可并行性 |
|---|---|---|---|
| `link`（ModuleLink，111 模块合并） | 3.56 s | 2.3% | ❌ 已被实验证伪 |
| `ssa.buildClassIndex` | 19.88 s | 12.6% | ⚠️ 部分 |
| `ssa.scanInitArities` | 1.76 s | 1.1% | — |
| **`ssa.synthesizeCctors`** | **71.67 s** | **45.5%** | ⚠️ 需先重构 |
| **`ssa.synthesizeSingletons`** | **59.90 s** | **38.0%** | ⚠️ 需先重构 |
| `ssa.buildAllFunctions`（~3000 函数） | 4.30 s | 2.7% | ✅ 但收益微小 |
| `hatSerialize`（SSA → 7.8 MB 文本） | 1.12 s | 0.8% | ✅ |
| **前端合计** | **157.35 s** | 100% | |

**131.57 s（95.3%）集中在三个函数**：`synthesizeCctors` + `synthesizeSingletons` + `buildClassIndex`。这三个不是并行难做，而是**它们的串行性不来自数据依赖，而来自共享的输出 value arena**。换句话说：它们是串行的，不是因为函数 A 的输出要喂给函数 B，而是因为它们都往同一个 `program.values` 列表尾部追加。

这个区分决定了整份设计的走向：**先做的应该不是加锁、不是开线程，而是把这三个 pass 拆成 per-class chunk**。线程是 chunk 化之后的可选加速器，不是替代品。

---

## 二、并行化的三条路，逐条评估

### 2.1 模块级并行 —— ❌ 不可行

**结构障碍**：`AotModuleLinker.link()`（`aura/compiler/aura/lang/compiler/aot/ModuleLink.aura:65`）在**进入 SSA 之前**就把所有 111 个模块合并成一个扁平的 `Hir` arena。没有 per-module HIR，没有模块边界记录，没有独立的符号表。

**这个尝试已经被做过并且被放弃了**。`ModuleLink.aura:93-113` 的 `preloadCache` 里有原始记录：

> 为什么这里是顺序读取（而不是 Thread.spawn 并行）… **AOT 后端目前不支持 `object` 单例字段访问**：(1) `ParallelState.paths` 会被发射成**每次调用都新建一个 `ArrayList` 对象**；(2) `ArrayList` 实例又会被当作**裸集合句柄**（`AuraDynList*`）传给 `Collections.listAppend`，而 `%struct.ArrayList`（16 字节）与 `AuraDynList`（24 字节）布局不同 —— 读 `items` 会读到 `0xABAB…` 填充，随即在 `realloc` 中段错误（0xC0000005）。
>
> **实测并行版与顺序版自举耗时相同**（均约 13.2 s：40+ 模块的读取并非瓶颈）。

结论：即使解决了单例 bug，这个环节的收益上限是 2.3%。投入产出比不成立。

### 2.2 函数级并行 —— ⚠️ 可行但收益 2.7%

这是唯一"技术上干净"的并行点，所以值得完整分析，哪怕收益不大。

**独立性论证**：`buildFunction`（`SsaBuilder.aura:339-506`）之间没有数据依赖。具体地：

- **符号命名是前置且确定的**。`buildFunction` 第 361-380 行在 lowering 开始前就按 `src.textOf(id) + ownerClassNameOf(id)` 算出 `Class_method`，重载 `init` 的 `_<N>` 后缀来自 `initArityCsv`（`scanInitArities` 预扫描）。
- **没有第二遍符号修复**。`buildCall`（1649-1804）在调用点就地用 `methodOwners`/`methodRetTypes`/`freeFuncs` 解析被调符号 —— 这些都是 `buildClassIndex` 建好的**只读表**，lowering 期间不写入。
- **没有跨函数前向引用需要回填**。

**耦合点只有三个**，全部可解：

| 耦合点 | 位置 | 解法 |
|---|---|---|
| 共享 value arena | `MirSsaProgram.addValue`（`SsaMir.aura:227-239`）追加全局 `values` | 每 worker 独立 arena，事后按 `valueStart` 范围合并 |
| `addBlock` 读 `this.currentFunc` | `SsaMir.aura:264-277` | 把 `currentFunc` 从实例字段改为显式参数 |
| `TypeRegistry` 读写 | `SsaBuilder.aura:385, 401, 453` | 预填充为只读，或加互斥 |

顺带一个好消息：`MirFunction.valueStart`（`SsaMir.aura:168`）的存在本身就是为"连续追加后按范围切片"设计的（注释明确说这是为了避免 O(functions × values) 扫描）。**所以 arena 合并这件事，数据模型已经支持了**。

**收益上限**：4.3 s ÷ 8 核 ≈ 最好 3.3 s，省下 1.0 s。**不值得先做**。

### 2.3 进程级并行 —— ❌ 本次不改（后端与 LLVM IR 路径保持现状）

按本次的范围界定，**Photon 后端与遗留 LLVM IR 路径都保持现状**，因此进程级并行不作为本方案的候选项。这里只记录现状，供将来若范围变化时参考。

**当前状态**：活跃的 Photon 路径（`PhotonObjectWriter.aura` 2261 行 + `X86Encoder.aura` 1418 行）**手工拼装 COFF，不走 LLVM**。唯一的外部进程调用是 `PhotonSystemLinker.aura:156-162` 的最终 `lld-link`（只链接 2 个 `.obj`，1 条命令）。所以这条路径几乎没有可外发的活儿。

**遗留路径**（`aura/compiler/aura/lang/compiler/aot/Aot.aura`）才是真进程级：`HIR → LLVM IR 文本 → .ll 落盘 → llc → .obj → clang → .exe`，三个阻塞的 `Process.run`（`Aot.aura:145, 180, 191`）。`llc` 与 `clang` 天然可并行，但按本次范围界定**不改动**。

**未做的一个备选**是"**一模块一进程**"：`PhotonHatCompile.aura` 本身就是一个读环境变量（`AURA_HAT_AURA`/`AURA_HAT_OUT`/`AURA_HAT_MODULE`）、往 stdout 打 `===COFF-MAIN===`/`===RESULT===` 标记的独立 worker，spawn N 个零改造。**但前提仍是 §2.1 的合并-HIR 模型要拆开**，否则每个 worker 都会重复编译全部 111 个模块——所以即便开放范围，这条路也只是 §2.1 的延伸，不是独立选项。

**记录一个潜在的阻塞项**：`Process.run`（`aura/core/aura/lang/std/Process.aura:37`）**只有同步阻塞版**，全仓库搜不到 `Process.start`/`Process.wait`。本次不处理，仅备案。

---

## 三、如果一定要上多线程：现状盘点

### 3.1 已有的原语（都在，但质量参差）

`aura/core/aura/lang/concurrent/` 下 12 个文件齐全：`Thread`（80 行）、`Mutex`（89）、`Atomic`（107）、`RwLock`（112）、`Condvar`（105）、`Barrier`（91）、`Semaphore`（116）、`Channel`（48）、`Promise`（113）、`Future`（198）、`Actor`（110）、`Coroutine`（338）。

底层 native 桥**完整且跨平台**：

- `aura/core/aura/lang/native/ThreadOps.aura:37-49` 声明 5 个 extern 函数（`create`/`join`/`sleepMs`/`currentId`/`cores`）。
- `aura/runtime/cffi/aura_syscalls.c:1286-1357` 全实现：Windows `CreateThread`（1293）、POSIX `pthread_create`（1298）、`WaitForSingleObject`/`pthread_join`、`GetSystemInfo.dwNumberOfProcessors`/`sysconf`。
- `aura/core/aura/lang/native/arch/x86_64_windows/Syscalls.aura:163` 有 `@native(0x23) ntCreateThread`、`:236` 有 `ntCreateThreadEx` 全签名。
- AOT 发射器已能降级这些调用：`Emit.aura:2213-2251`、`:2833-2845`。

### 3.2 必须先知道的坑

这些是实测过的，不是理论风险。按严重度排序：

1. **`object` 单例字段在 AOT 下语义错误**。`ModuleLink.aura:93-113` 有完整复现：单例字段被编译成"每次调用新建一个 ArrayList"，且 `%struct.ArrayList`（16 B）与 `AuraDynList`（24 B）布局不符 → 读到 `0xABAB…` 填充 → `realloc` 段错误 0xC0000005。
   **规则**：并行共享状态必须挂在**显式传入的 `class` 实例**字段上，绝不挂 `object` 单例。

2. **`HashMap` 构造即崩溃**。`ModuleLink.aura:49-52`：`HashMap()` 在自举下 EXIT 0xC0000005。
   后果：整个代码库的所有表都是 `|` 分隔 CSV 字符串或 `List<String>`，配 `indexOf` 扫描。**这意味着 work-stealing 与共享计数器的惯用法都不可用**，只能用 `Cpu.atomicAdd` 支撑的原子（`Atomic.add`）或 `Mutex`。

3. **`List[Int]` + 索引 + add 在 AOT 下崩溃**。`SsaBuilder.aura:119-123`、`:220-222`、`Emit.aura:9502` 都有记录。这正是我上一轮优化里 `clsLayoutTotal` 必须用定宽字符串而不是 `mutableListOf<Int>` 的原因。
   **后果**：并行写入方不能写类型化整型数组，只能写 CSV 字符串槽。

4. **`AURA_THREAD_FN_MAX = 64`**（`aura_syscalls.c:1250`）。`aura_thread_register_fn` 用静态表 `aura_thread_fns[64]` 按 `fn_id` 索引。`ThreadOps.create(fn_id, arg)` **只接受一个 Int 参数**，所以 work item 不能是胖指针/闭包，必须传一个索引到全局任务数组。
   64 够一个线程池用，但这是个硬编码上限。

5. **`Thread` 只能 spawn 函数，不能 spawn lambda**。`spawn(fn_id, arg)` 里的 `fn_id` 是注册表索引，没有闭包捕获。worker 必须是**自由函数 + 单个 Int 参数索引共享状态**。

6. **`Mutex.lock` 是无退避自旋锁**（`Mutex.aura:53-62`）。纯 Aura 实现的 `while (true)` 循环，没有 `sleep`、没有 backoff、unlock 没有内存屏障。
   对"临界区内不分配内存"的极短段没问题；对"持锁时可能分配"的段是活锁风险 —— 而本编译器在 AOT 无 GC 运行时下分配极其频繁（`PhotonPipeline.aura:296-298` 明确依赖"无 GC、malloc 即泄漏"）。

7. **`Atomic.load`/`store` 不是原子的**（`Atomic.aura:33-46`）。`load = Memory.read64`、`store = Memory.write64` 都是普通读写，**只有 `add` 走了 `Cpu.atomicAdd`**。所以 `Atomic` 类只能用于 CAS 式增量，不能当普通变量用。

8. **没有 GC**（`PhotonPipeline.aura:296-298`）。线程退出后 `malloc` 的内存不释放。8 个 worker 各建一套 arena，峰值内存会近似线性膨胀到 8 倍。自举编译当前峰值 ~5.2 GB，8 核并行理论上可能 30+ GB。

### 3.3 已经存在的可复用模式

- **配对合并 join**：`joinChunks`（`HatSerializer.aura:76`）、`coffJoinList`（`PhotonObjectWriter.aura:881`）、`joinBytes`（`X86Encoder.aura:968`）、`joinPairwise`（`PhotonPipeline.aura:1023`）。注释明确记录这是为了解决 `out = out + x` 的 O(n²)：`PhotonPipeline.aura:1007-1010` 记载"自举 Main.aura 有 2,647 个函数，总长约 106 KB，逐条拼接的理论拷贝量约 140 MB"。
- **已知可用的共享状态范式**：状态挂 `class` 实例字段（`ModuleLink.aura` 的 `AotModuleLinker` 自己就是范本）。

---

## 四、真正的时间杀手：三个串行 pass 的结构问题

这是本文档的核心论点，单独展开。

### 4.1 `synthesizeCctors`（71.67 s）与 `synthesizeSingletons`（59.90 s）

两者都在**共享 value arena 上追加**。具体地，它们调用 `this.program.addFunction` 和 `this.program.addValue`，而 `addValue`（`SsaMir.aura:227-239`）是往全局 `values` 列表尾部追加并返回全局索引。

**它们串行不是因为数据依赖**，而是因为共享追加 arena。没有任何"类 A 的构造器需要类 B 的构造器输出"的依赖 —— 每个构造器的输入只有该类自己的字段布局表（`clsLayoutTys`/`clsLayoutDefaults`/`clsLayoutTotal`，全只读）和 `initArityCsv`（预扫描，只读）。

**推论**：给 `addValue` 加锁来并行化，是在用锁竞争去买一个 chunk 化就能免费拿到的东西。先 chunk 化，之后如果要再加线程，那时每个 worker 有自己的 arena，根本不需要锁。

### 4.2 `buildClassIndex`（19.88 s）

这个更麻烦。它是 4 个顺序 pass 扫描 15 万 HIR 节点（`SsaBuilder.aura:2712-2775`）：

1. 预填 `ownerByNode`（2715-2719）
2. 登记类（2720-2735）
3. `scanDeclMethodReturnTypes` 扫全部声明（2749-2757）
4. 收集 `freeFuncs`（2758-2770）
5. `buildFieldLayoutCache`（2774）

**真正的 O(n²) 病灶**在 2765 行：

```aura
this.freeFuncs = this.freeFuncs + "\n" + fn + "\n"
```

在 15 万节点的 `while` 循环里做字符串拼接 —— 每次追加都拷贝整串。注释（136-138）记载这正是当初把 `funcOwners` 改成 `ownerByNode: List<String>` 的原因（那个改对了，`freeFuncs` 被漏下了）。这是 19.88 s 里一个可单独修掉的部分。

顺带，`localTypeOf`（2583-2590）在**每个局部变量查找**时都 `splitStr(this.localTypes, "\n")` 把整个 map 重新切一遍，是 per-function 的 O(decls²) 内循环。

### 4.3 为什么这决定了优先级

95.3% 的时间在一个**不需要线程就能拆**的结构问题里。多线程在这里是拿锁竞争去模拟 chunk 化。这不是不能做，是做了会**更慢**（AOT 无 GC，临界区分配 = 内存泄漏 + 自旋锁空转）。

---

## 五、建议方案

按本次范围界定（前端改造，后端与 LLVM IR 路径不动），共三个阶段。

### 阶段 1（主抓手，估计 3-5 天，目标 -60% 到 -80% 总时长）

**不做线程**。改三个串行 pass 的结构。

1. **修 `freeFuncs` 的 O(n²) 拼接**（`SsaBuilder.aura:2765`）—— 改成 `List<String>` 收完后一次配对合并（沿用 `joinChunks`/`coffJoinList` 这套既有范式）。这一处单独就能砍掉 `buildClassIndex` 的相当比例。
2. **修 `localTypeOf` 的每次查找全量 split**（2583-2590）—— 要么预建反向索引，要么改成 `indexOf` 边界扫描（本文件惯例）。
3. **把 `synthesizeCctors` / `synthesizeSingletons` 拆成 per-class chunk**：
   - 第一遍：为每个类**预分配** value 区间 `[base, base + estimate)`，`estimate` 由 `clsLayoutTotal[ci] * K + C` 估出（构造器体形状固定，可精确估算）。
   - 第二遍：每个类的构造器写进自己的区间，无竞争、无锁、无共享可变状态。
   - 第三遍：按区间顺序回填全局 `values` 的 ID 重映射表。
   - 这一项的目标是把 131.57 s 的 95% 部分从"串行追加"变成"批量写入"，即使纯串行执行也应该显著下降，因为消除了 `values` 列表反复增长的内存搬移。

**为什么这一步优先于线程**：chunk 化后每个 worker 天然拥有独立区间，**线程是白送的**（无锁、无共享、无 GC 压力）。反过来先上线程，就得在 `addValue` 上加自旋锁，每次临界区里还在 `malloc`（无 GC），会锁竞争 + 内存膨胀双输。

### 阶段 2（可选，1 天，收益 ~1-3%）

**如果阶段 1 做完仍不达标**，再加函数级并行：

- 把 `addBlock` 的 `this.currentFunc` 改成显式参数（`SsaMir.aura:264-277`）。
- 每 worker 一个 `MirSsaProgram`，用 `valueStart` 范围事后合并。
- `TypeRegistry` 在 `buildAllFunctions` 前一次性建满，之后只读。
- 任务分发：`ThreadOps.create(fn_id, arg)` 里 `arg` 是全局任务数组的类序/函数序索引；worker 是自由函数。
- worker 数 = `ThreadOps.cores()`，上限 `AURA_THREAD_FN_MAX - 1`（留 1 给主线程调度）。
- **绝对不要用 `Mutex` 包 `program.addValue`**：改用每 worker 独立 arena + 合并。

收益上限 4.3 s → ~3.3 s。作为锦上添花，不作为目标。

### 阶段 3（大改，估计 1-2 周，收益不可预估计，本次不做）

**模块级并行**。前提是拆掉合并-HIR 模型：

- `link()` 返回 per-module `Hir` + 模块边界记录，而不是合并后的单一 arena。
- `SsaBuilder` 改为 per-module 实例。
- 跨模块调用在 lowering 后统一解析（当前 `resolveMethodSymbol` 用只读表，天然支持，但需要一张跨模块符号表）。
- 对应 `docs/系统-多文件编译打包方案设计.md:881` 的 R4 项："百模块以上增量编译慢 → 并行编译（模块间无共享状态）+ fingerprint 预过滤"。

这是设计文档里早就写下的路线，但**它不是多线程问题，是架构问题**。线程只是它的实现手段之一。**本次不列入实施计划**，仅备案：`link` 只占 2.3%，且该方向已被实验证伪无收益（见 §2.1），性价比极低。

---

## 六、不推荐做的事

1. **不要给 `program.addValue` 加 `Mutex` 然后并行 `buildAllFunctions`**。收益 2.7%，而 AOT 无 GC + 自旋锁 + 临界区分配 = 活锁风险 + 内存泄漏。
2. **不要为并行化引入 `HashMap`**。它在自举下构造即崩。所有表保持 CSV 字符串 / `List<String>` 惯例。
3. **不要把共享状态挂 `object` 单例**。`ModuleLink.aura:93-113` 有完整崩溃记录。挂 `class` 实例字段。
4. **不要动 `localTypeOf` 的反向查找语义**。`SsaBuilder.aura:2550-2567` 注释记录它为了修 `#public|String` 前缀污染的 bug 才引入，性能代价是已知接受的权衡。改它要么预建索引，别直接删。
5. **不要指望进程级并行救 Photon 路径**。Photon 手工拼 COFF，唯一外部调用是最后一条 `lld-link`，没有可外发的阶段。
6. **不要在本轮改动 Photon 后端或遗留 LLVM IR 路径**。本次范围明确只动前端 SSA，后端保持现状；`compileHat`（Phase B–E）本身只占 2%，改它没有意义。

---

## 七、预估与验收

| 方案 | 预估收益 | 复杂度 | 风险 |
|---|---|---|---|
| 阶段 1（chunk 化 + 修 O(n²)） | **-60% 到 -80%** | 3-5 天 | 中：value ID 重映射需仔细 |
| 阶段 2（函数级并行，可选） | ~1-3% | 1 天 | 低 |
| 阶段 3（模块级并行，本次不做） | 不可预估计 | 1-2 周 | 高：架构改动 |
| 直接上多线程（不做阶段 1） | **可能负收益** | — | 高：锁竞争 + 内存膨胀 |

**验收口径**：以 `Main.aura` 自举**前端总时长**为准（不含 `compileHat` 与链接），用 `AURA_PHOTON_TIME=1` 分段输出核对。阶段 1 的目标是让 `synthesizeCctors` + `synthesizeSingletons` 合计从 131.57 s 降到 30 s 以下。

---

## 八、一句话总结

**这个编译器的慢，不是"没有线程"，而是"有一个该拆成 chunk 的串行追加结构，被当成了并行问题"。** 先做 chunk 化，线程是白送的；反过来先上锁，你会同时得到锁竞争和内存泄漏。
