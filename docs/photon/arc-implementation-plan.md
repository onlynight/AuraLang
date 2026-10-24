# Photon AOT 运行时 ARC 实现方案（长期）

> 基于 R90–R147 十轮自举编译剖析的现状（131s / 5.05GB 峰值），2026-10-20。  
> 状态：**P0/P1 已实施并验收（R148）**；P2 机制已实施但**释放默认关闭**（R149，见 P2 报告 §三）；
> **P3 已实施并验收**：逃逸分析 + 配对消除完成，**「String 槽覆盖释放」通过全链自举逐字节验收**（存活块 −16.8%），
> 作用域退出 / return 出口释放经二分证伪并默认关闭。见 `photon_aot_arc_p3_2026-10-20.md`。
> **P5（新增）是自举链真正需要的 ARC**：驱动走 Photon 机器码后端，C 侧（P0–P3）对它不可见。
> 设计见本文 §五 P5。
> 实施报告：`photon_aot_arc_p0_p1_2026-10-20.md`、`photon_aot_arc_p2_2026-10-20.md`（工作区；仓库副本在 `docs/photon/`）。目标读者：编译器/运行时维护者。

---

## 一、问题定义

自举编译峰值 5.05GB 的构成：活列表 ~1.5GB + **字符串/列表临时物 ~1.2GB（永不回收）**

- 活结构 ~1GB + 其他。前端靠 region + `RegionReset` 整块回收（已工作良好）；  
  后端走堆分配但**从不 free**——所有 `+` 拼接、substring、CSV `+=` 的旧串全部滞留。  
  过去十轮的修复（tail-first、两两归并、前缀快速路径）都是在「没有回收机制」的前提下  
  做分配瘦身——治标。ARC 是治本：**旧值被覆盖时自动释放，`+=` 类模式的滞留垃圾从  
  机制上消失**，不再依赖逐点手工优化。

## 二、现状盘点（ARC 为什么现在是死的）

| 层          | 现状                                                                               | 证据                                |
| ---------- | -------------------------------------------------------------------------------- | --------------------------------- |
| Rust VM 基线 | **真 ARC 已实现**：堆槽带 `rc`、`IncRef/DecRef` 字节码、归零回收进空闲链表                             | `vm/heap.rs`（冻结只读，自举不走它）          |
| AOT 原语     | `Memory.arcIncrement/arcDecrement`（`lock inc/dec`）存在，emit.rs 能发 `atomicrmw`      | `Memory.aura:84`、`emit.rs:923`    |
| AOT 机制     | **不存在**：编译器从不自动插 retain/release；分配路径不给对象加引用计数头                                   | 全仓 `arcIncrement` 零调用者            |
| GC.aura    | `ObjectHeader{refCount,typeId,size}` + `ARC.retain/release` + `GC.freeObject` 草图 | 无调用者；`freeObject` 调 `Memory.free` |
| 堆头         | `AuraMemHdr{size, pad}` 16B 已存在，负载 16B 对齐                                        | `aura_std_cffi.c:114`             |

三个结构性障碍（方案必须逐一解决）：

1. **指针无类型信息**：`char*` 可能指向 .rdata 字面量 / 区域块 / 堆块——release 需要分辨；
2. **借用与持有混用**：同一缓冲区可能被别名共享（`strdupRetain` 的存在即为此）；
3. **环引用**：编译器 IR 父子互指 + owner 反指——纯 ARC 对环必泄漏。

## 三、目标与非目标

**目标**

- G1 后端（堆时代）堆对象确定性回收：字符串、列表、类实例覆盖；
- G2 前端区域时代**行为与性能完全不变**（bump + RegionReset 保留）;
- G3 自举编译为验收负载：峰值内存显著下降（目标 ≤3.5GB）、无正确性回归；
- G4 单线程零原子开销（编译器场景）；多线程可选原子模式；
- G5 双发射器（seed `emit.rs` 与自举 `aot/Emit.aura`）语义一致。

**非目标**

- 不做多线程安全 ARC（第一阶段）；
- 不做移动/借用检查的 Rust 级所有权系统（发射规则保守：复制即 retain）；
- 不处理环引用的自动回收（P4 提供 `weak`，此前环 = 文档化泄漏）；
- 不改动 Rust VM 冻结基线。

## 四、关键设计决策

### D1：ARC 管辖范围 = 后端堆时代对象；前端区域原样保留

**时代判据已存在**：`g_rgn_active`。前端（region 激活）所有分配来自 bump 区，  
**逐对象释放既不可能也无必要**（RegionReset 整块回收）→ ARC release 在区域时代  
**一律 no-op**（O(1) 标志检查）。`RegionReset`（`PhotonHatCompile.aura:320`）即  
前端→后端的时代边界，且**前后端对象不共享**（经 .hat 序列化重建，`hatReadParse`）  
→ 后端 ARC 永远不接触前端区域对象。这是整个方案能保持简单的根基。

### D2：头设计——复用 `AuraMemHdr`，`pad` 改 `rc`

```c
typedef union AuraMemHdr {
    struct {
        int64_t size;          /* 负载字节数（现状保留） */
        int64_t rc_and_flags;  /* [0..31]=rc, [32..47]=magic, [48..63]=flags */
    } h;
    long double align_ld;
} AuraMemHdr;   /* 16B 不变 → 负载对齐 / Plan A 不受影响，零额外空间 */
```

- `aura_mem_alloc` 初始化 `rc = 1`（分配即持有）；`magic` 参与 `hdr_sane` 强校验  
  （现在只查 size 范围——加 magic 后 double-free/use-after-free 可检测）；
- `arcRelease(p)` 流程：
  1. `g_rgn_active == 1` → **no-op**（区域时代，D1）；
  2. `p ∈ [.rdata_start, .rdata_end)` → **no-op**（字面量永生，见 D2b）；
  3. 头 magic 校验失败 → 计数 `g_aura_mem_bad_hdr` 并 no-op（防御）；
  4. `rc--`；为 0 → `aura_mem_free`。
- `arcRetain(p)`：步骤 1–3 同上（3 失败则告警），`rc++`。

**D2b：.rdata 字面量**。HIR 字符串常量发射在 `.rdata`（只读段，无法写 rc）。  
注册区间的两个候选（P1 择一）：

- emit.rs 在 `.rdata` 前后各发一个导出符号 `__aura_lit_begin/end`，运行时  
  `aura_mem_init` 用 `GetModuleHandleA(NULL)` + 符号地址注册区间（首选，零解析）；
- 或运行时解析 PE 头取 `.rdata` 区段（不依赖符号，但依赖 PE 布局假设）。

**D2c：区域块指针的防线**。理论上区域时代 release 已被 D1 拦截；但为防  
「后端时代拿到残留区域指针」（悬空，RegionReset 后页已 unmap），沿用  
`aura_mem_realloc` 已有的区块扫描守卫思想——后端时代 `g_rgn_ret.n == 0`，  
扫描为 O(1)。

### D3：所有权不变量（编译器与运行时的契约）

> **任何在 SSA/槽位中存续的 String/List/Object 值，持有者对其计数 +1；  
> 覆盖、离开作用域、从容器移除时持有者负责 release。**  
> 整数/浮点/布尔走 Plan A 低位装箱（bit0=1），天然不参与 ARC。

该不变量使 release 决策局部化：覆盖槽位时 release 旧值**不需要知道旧值来源**  
（无论它是字面量、拼接结果还是字段加载——字面量/区域指针被 D2 防线过滤）。

### D4：发射规则（emit 层，双发射器一致）

按 HIR 语句/表达式类型逐一列出（emit.rs 与 aot/Emit.aura 各实现一份）：

| 场景                  | 动作                                                              |
| ------------------- | --------------------------------------------------------------- |
| `val x = expr`（新建槽） | 无额外动作（expr 结果的所有权转移给槽）                                          |
| `x = expr`（覆盖）      | `release(old_x)` → 存入                                           |
| `y = x`（读取复制到新槽）    | `retain(x_val)` → 存入                                            |
| 函数实参传递              | 调用者 `retain`（被调者返回前 release；见 D5 简化）                            |
| 函数返回值               | 所有权随返回值转移给调用者（无额外动作）                                            |
| 字段存 `obj.f = v`     | `retain(v)` → `release(old_f)` → 存                              |
| 字段读 `t = obj.f`     | `retain`（读出即持有）                                                 |
| 列表 `push/set`       | `retain`；`remove/pop`：所有权转出（调用者负责）                              |
| 作用域退出               | 按声明逆序 `release` 所有 String/List/Obj 局部量                          |
| **循环体**             | 每次迭代末 release 本迭代新建的临时槽（**这条对后端编译循环最关键**——发射循环变量槽的迭代边界 release） |
| `if/else` 汇合        | 两分支各自持有的临时在分支尾 release；merge 槽按覆盖规则                             |

\** native（C 侧）边界（D5）\*\*：

- 约定：**C 侧对实参只借用**（调用期间有效，不 retain 不 free）；返回的  
  String/List 一律是**新持有**（堆分配）或**永生**（.rdata）；
- P0 审计清单：全部返回字符串的 C 导出（`aura_string_concat/dup_n/substr_dup/
  strdup/strdupRetain/env_get/fromCharCode/…`）——已确认 concat/dup/substr/  
  strdup/env_get/fromCharCode 均返回新分配 ✓；需专项排查「返回静态缓冲」的  
  历史教训点（`fromCharCode` 注释记载过 static 缓冲互相覆盖的教训）；
- `strdupRetain` 在 ARC 世界里退化为 `retain`（或保留为「复制进堆」的兼容入口）。

### D6：单线程非原子 rc

编译器单线程 → `rc++/--` 用普通加解（`lock inc` 原子版保留给多线程场景，  
经 `AURA_ARC_ATOMIC` 编译开关切换）。这是相对 Swift 的主要性能优势来源。

### D7：环引用

- 编译器自身 IR **不受影响**：前端 IR 在区域里（D1），后端 LIR/DAG 在 ARC 化后  
  若出现互指环——后端结构（Dag parent/owner 反指）**明确为非拥有指针**  
  （发射规则：反指字段不做 retain/release，由正向边拥有），在 P2 的发射规则  
  中按字段标注 owner/borrow；
- 用户程序环：P4 提供 `weak` 字段限定符（发射为「存裸指针不 retain」）；  
  在此之前文档声明「环 = 泄漏」。

## 五、分阶段实施路线

### P0（1–2 天）：运行时地基，零行为变化 —— ✅ 已实施（R148）

- `AuraMemHdr.pad → rc_and_flags`（含 magic 强校验进 `hdr_sane`）；
- C 侧 `arc_retain/arc_release`（D2 流程）+ 退出时泄漏报告  
  （live blocks 按 size 直方图 + 总数，`AURA_LEAK_REPORT=1`）；
- .rdata 区间注册（D2b）；
- **验收**：全部现有测试/冒烟不变；泄漏报告在自举后端跑出 ~1.2GB 临时物数字  
  （与归因对账）。

### P1（3–5 天）：dynlist 与字符串原语 ARC 化（C 侧先走通）—— ✅ 已实施（R148）

- `dynlist_new/push/realloc` 经 ARC 头分配；`listRemoveAt/clear` 对被移除元素  
  release（元素是 `Value`——只有低位 0 的指针才 release，装箱整数跳过）；
- 字符串四入口（concat/dup_n/substr_dup/strdup）已天然产堆块 → 无需改；
- `listAppend` push 时对元素 retain（若为指针）；
- **验收**：不开编译器发射（无 release 发生）→ 行为与现在逐字节一致；  
  泄漏报告开始区分「无 release 发射」与「漏 release」。

### P2（1–2 周）：编译器发射注入（核心工程）—— ⚠️ **仅单侧完成**（R149）：retain 计数可用，**release 默认关闭**（3 类逃逸别名未闭合，见 P2 报告 §三）；`aot/Emit.aura` 侧**尚未开始**（见 §六）

- emit.rs 与 aot/Emit.aura 各实现 D4 规则表；先挂 `AURA_ARC=1` 开关；
- 实现顺序：①局部槽覆盖/作用域退出（收益最大——varMap/CSV += 类自动自愈）  
  → ②字段/列表 → ③实参传递；
- 槽类型判定用静态类型（HIR 类型已知）；`List<Any>` 元素用低位位测；
- **验收**：自举 success + BOOT-4；`.hat/.obj` 逐字节一致（ARC 不改输出）；  
  峰值内存按阶段对账；泄漏报告应显示后端 live ≈ 活结构真实大小。

### P3（性能与收尾）——**已实施**

- ✅ retain/release 配对消除（发射器窥孔 `AURA_ARC_PEEP`）+ 自赋值省略；
- ✅ **受限逃逸分析**：`scan_escapes` 一趟扫描标出逃逸写点（字段/下标赋值、存储型
  调用实参、`New` 实参、`Box`/`WeakRef`/`Await`、`defer`/`try` 块），未命中才放行 release；
- ✅ **全链自举逐字节验收**：只有「`x = <expr>` 槽覆盖释放 + String 静态类型 + 逃逸放行」
  这一条通道通过（存活块 41.28M → 34.35M，**−16.8%**；`.hat`/`.obj` md5 不变、BOOT-4 全过）；
- ❌ **作用域退出释放 / return 出口释放**：二分证明单独或叠加都会破坏 `.obj`
  （释放仍被引用的活对象，且**无崩溃信号**——`bad_magic` 可为 0），故默认关闭；
- ❌ 「定值自有性闸门」（更严的第 4 层）也没能救回这两条路径，仅作为探索旋钮保留。
- ⚠️ 代价：同规模样例 3 次均值 +3%~8%（自举级因大堆 churn/冷启动因素单次观测偏高，待复测）。
- ⇒ **不做**原计划的「`AURA_ARC` 默认开」：默认路径保持零 ARC 指令不变（已验证），
  回收档继续以显式开关提供。

### P4（后续）：**先度量，再决定回收面** —— 度量已落地（R151）

**R151 实测改写了优先级**（见 `photon_aot_arc_p4_2026-10-21.md`）：4.3GB 后端载荷里
**2.87GB 落在 4216 个 ≥64KB 大块上，其中约 2.6GB 是 `InstructionSelector` 每函数重建的
记账列表**（`nodeMapIds` 等，1125 个平均 ~1.3MB 的旧数组并存）；而 `sbReserve` /
`string_concat` 这些「大临时物」加起来不到 60MB。

- ❌ **大临时物分块化降级**：收益上限 ≈ 60MB，不值得先做；
- ❌ **局部逃逸分析**：P3 已证伪（要求零 false-negative，代码库存在编译器不可见的别名）；
- ✅ **①「选择器记账表」**（原「阶段 region 回收」的具体化）——**根因已在源码层确认，见
  `photon_aot_mem_bulk_analysis_2026-10-21.md`**：
  `nodeMapIds` / `labelNames` 每函数都按 `while (size <= id) add(...)` 线性补齐，而
  **`valueCount` / `blockCount` 是 `LirProgram` 上的程序级计数器**（`Lir.aura:284/287`，
  `LirFunction` 没有任何计数器）⇒ 第 k 个函数建的表长度 ≈ 此刻的全局 id 数，**总量 O(n²)**
  （1125 函数 × 平均 16 万项 ≈ 1.8 亿项 ≈ 1.4GB，与实测吻合）。
  ⇒ 修法是**把索引改成函数内偏移**（`id - vidBase` / `id - blockBase`），
  **不需要 ARC、不需要 free、不需要新原语**；预期 ≥64KB 大块 2867MB → ~450MB，
  同时把 1.8 亿次 append 降到 ~420 万次（时间内存同降）。
- weak 引用 + 环文档化 + 多线程原子开关继续顺延。

**度量设施（R151 交付，默认关闭）**：`AURA_BIG_CENSUS=1`（≥64KB 分配按栈帧建账、
退出打存活 top-20）+ `build/arc-bigsite.py`（RVA → Aura 函数名，需 `AURA_LINK_MAP=1` 建驱动）
+ `build/arc-peakprobe.ps1`（峰值 WS 采样定点验证）。

### P5（新）：**Photon 机器码后端的 ARC** —— 这才是自举链的回收面

> 背景（R154 实测）：驱动 `PhotonHatCompile.exe` 走的是
> `SsaMir → Lowering → Lir → InstructionSelection → X86Emitter → PhotonObjectWriter(COFF)`，
> 输出机器码，**不经 LLVM IR、不链 `aura_std_cffi.c`**（`aura_runtime.obj` 里查不到
> `aura_malloc`/`aura_arc_*` 符号）。P0–P3 做的 C 侧 ARC 对它**完全不可见**。
> ⇒ 想让自举产物自己回收内存，必须在 **PhotonRuntime（机器码）** 里实现一套 ARC。

#### 已核实的事实（设计的地基，勿凭记忆改）

| 事实 | 出处 |
|---|---|
| 变量赋值在 SSA 层就是一个 `Store`，`aux` 里带变量名 | `SsaBuilder.aura:2265` `addValue("Store", varTypeId, args, varName, …)` |
| 对象头布局 `[0x00]=vtable`、`[0x08]=size`、`[0x10]` 起是字段 | `PhotonRuntime.aura:3315-3325`（`emitClassConstructorWithVtable`） |
| **对象头里没有 rc 槽位，字符串连头都没有** | 同上 + 字符串走各自的分配路径 |
| 已有裸 rc 原语 `Memory_arcIncrement/arcDecrement`（`rcx=addr`，返回 `rax`） | `PhotonRuntime.aura:4315/4332` |
| 类型信息可用：`TypeRegistry.isString(vid)/isPtr(vid)`，`InstructionSelection` 已持有 `src.types` | `TypeRegistry.aura:329/334`、`InstructionSelection.aura` |
| 逃逸/存储写点信息在 SSA 层可得（`Store` 到非本帧变量、`Call` 实参） | `SsaBuilder` 的 `Store`/`Call` 生成处 |

#### 设计决策

- **D-P1　rc 放侧表，不改对象头。**
  在对象头加 rc 需要把 `[0x10]` 之后所有字段整体后移，等于改 vtable 布局、字段偏移、
  结构体 `size` 计算 —— 牵动面太大且难验。改为运行时维护
  **`ptr → rc` 开放寻址侧表**（初始 64K 槽，超过 70% 装载则 ×2 重散列）：
  零布局变更，字符串/类实例统一处理。代价是每次 retain/release 一次哈希（可接受，
  先按「默认关、opt-in 对比」验证收益后再谈优化）。
- **D-P2　只管辖 String，与 seed 已验证档同构。**
  闸门 = `TypeRegistry.isString(vid)`。容器/类实例先不做（P3 已证别名面太宽）。
- **D-P3　注入点选在 SSA→LIR 之间的一趟 `ArcLower` pass**（新增，不改 `SsaBuilder`）。
  规则（对应 D4）：
  | 场景 | 动作 |
  |---|---|
  | `Store(var, v)` 且 `v` 是复制（同一 vid 直接来源） | 先 `retain(v)`，再取旧值 `release`，再 store |
  | `Store(var, v)` 且 `v` 是新值 | 只取旧值 `release`，再 store |
  | 逃逸闸门命中（别名存进容器 / 字段 / 下标 / 闭包） | 该变量整条通道关闭 |
  `ArcLower` 之后 `InstructionSelection`/`X86Emitter` 无需知道 ARC 存在 ——
  它只是多出几条 `Call` 到运行时符号的 LIR 指令。
- **D-P4　四道防线（与 C 侧同源，防止踩外来指针）**：
  `NULL` → 装箱整数（低位 = 1）→ `.rdata` 字面量区间（运行时解析主模块 PE 头）→
  侧表未登记（= 不是本运行时分配的）。任一不过一律 no-op，并计诊断计数。
- **D-P5　开关是「编译期」的，不是运行期 env。**
  产物是静态机器码，所以 `PhotonPipeline` 读 `AURA_ARC`（编译它的那个进程的环境）
  决定要不要跑 `ArcLower`。默认不跑 ⇒ 产物逐字节不变（这是回归护栏）。
- **D-P6　前置条件：字符串分配必须集中，且堆必须支持逐对象释放。**
  初查：Photon 侧只暴露出 `Heap_init` / `Heap_mmap` 两个堆符号（`X86Emitter:451` 调 `Heap_init`），
  没有任何 `Heap_free`/`Heap_alloc` —— **很可能是「mmap 上的一次性 bump 堆」**。
  如果是这样，P5 的第 0 步不是写 ARC，而是**先在 PhotonRuntime 里补一个可回收的分配器**
  （size-class free list，或至少 `Heap_free` + 惰性合并），否则 `arcRelease` 归零后无处归还，
  「回收」只是把计数减到 0 而内存照样占着。
  ⇒ **P5-A 的第一件事是审计**：确认字符串/对象是否全部经由同一个分配入口、该入口是否可 free。
  审计不通过就停在 P5-A，不要进 P5-C。
  另一条约束：rc 侧表要求「所有 String 值都来自本运行时」，若存在内联 `malloc`/`Heap_mmap`
  直分配，必须先收敛到 `runtime_allocString(n)`，否则会给 libc 指针登记 rc，释放时双管。

#### P5 的收益预期（先说实话）

- 若 Photon 堆是 bump-only：**ARC 单独做不出「内存下降」**，必须先有 free list；两步合计是
  「新增分配器 + ArcLower」的大工程，且要在自举产物上验（13 分钟一轮）。
- 若已存在可回收分配：按 seed P3 的实测比例外推，String 槽覆盖释放能回收的量级在
  **数十 MB~百 MB**（P3 在驱动上回收 6.9M 块只值 36MB，payload 主体是容器容量而非字符串）——
  ⇒ 对「峰值 4.8GB」这个目标贡献有限。**真正的大头仍是 P4-①（`nodeMapIds` 1.4GB）。**

#### 分步与验收

| 步 | 内容 | 验收 |
|---|---|---|
| **P5-A** | 字符串分配入口审计与收敛（`runtime_allocString`）+ `.rdata` 区间注册 | 样例产物 `.obj` 逐字节不变（只加不被调用的函数） |
| **P5-B** | `runtime_arcRetain/arcRelease`（侧表 + 四道防线）+ 诊断（retain/release/freed/bad 计数） | 只挂不调用 ⇒ 产物不变；手工注入一对 retain/release 的样例行跑通 |
| **P5-C** | `ArcLower` pass（Store 覆盖释放 + 复制 retain + 逃逸闸门），`AURA_ARC` 编译期开关 | ① 默认关：`.obj` 不变；② 开：样例矩阵 exit 0 + stdout 不变；③ 全链自举 `.hat`/`.obj` 逐字节一致 + `BOOT-4`；④ `freed` 计数 > 0 且 `bad=0` |

#### 风险与缓解

| 风险 | 缓解 |
|---|---|
| 字符串分配路径不收敛（有内联 malloc） | P5-A 先审计，不通过就不进 P5-C |
| 侧表与 libc 堆双管（释放后再被 CRT free） | 侧表只对「登记过」的指针动手；未登记一律 no-op |
| 逃逸闸门漏判 → 提前释放 | 只放行 String；`AURA_ARC_ESCAPE` 可退化为「全部不释放」对照 |
| 性能：每次赋值一次哈希 | 先测收益；必要时 rc 直接内联到字符串 pre-header（需字符串入口已收敛，P5-A 之后可行） |
| 与 seed 侧规则漂移 | D4 规则表是唯一真源，两侧按表实现；验收都用「产物逐字节一致」 |

## 六、双发射器同步

> **先读 P5。** 本节原本假定自举链由两个 *LLVM IR* 发射器构成，R154 实测已推翻该前提：
> 自举链实际是「seed 的 LLVM 后端 + Photon 的机器码后端」。所以本节的「双侧对齐」
> 对自举链而言 = **`seed/emit.rs` ↔ Photon 机器码后端**，而 `aot/Emit.aura` 只服务
> `Aot.aura` 那条可选 LLVM 路径（已补齐惰性 ARC 骨架，默认关、零产物变化）。

本仓库存在**两个发射器**：seed 的 `codegen/aot/emit.rs`（编译驱动自身）与  
自举的 `aura/compiler/aura/lang/compiler/aot/Emit.aura`（自编译产物用的）。  
历史教训（ARC intrinsic 两处注释）表明二者必须逐语义对齐。P2 的每条规则  
必须双侧同轮落地 + 同一验收（.obj 逐字节一致 + 自举全链）。建议：  
规则表写成一份共享文档（本文 D4），两侧按表实现并交叉审。

**当前状态（2026-10-21 审计，见 `photon_aot_arc_status_2026-10-21.md`）：**

- `seed/codegen/aot/emit.rs`：D4 表已落地（retain 全量 + `x = <expr>` String 槽覆盖释放 + 逃逸扫描 + 配对消除 + 开关诊断）；
- `aura/compiler/aura/lang/compiler/aot/Emit.aura`：已补齐 ARC 骨架（开关 / 逃逸扫描 / 覆盖释放注入 /
  惰性 `declare`），**但默认关、且不在这条自举链上**（见下条）。

> ⚠️ **R153 更正：本节「两个发射器」的前提对自举链不成立。**
> 实测驱动的编译链是
> `SsaMir → Lowering → Lir → InstructionSelection → X86Emitter → PhotonObjectWriter(COFF) → PhotonSystemLinker`
> （`PhotonPipeline.aura:9` 的注释即此链路），**输出的是机器码，完全不经过 LLVM IR**。
> `PhotonHatCompile.aura` 虽然 `import aura.lang.compiler.aot.Emit`，但全文**没有任何 `Emit.` 调用**
> （只 import 不用）；`Emit.aura`（`LlvmEmitter`）只被 `Aot.aura` 的 `compile()` 使用，
> 即「Aura 代码自己想走 LLVM 后端」时才用。
> ⇒ **自举链上真正的「第二个发射器」是 `X86Emitter`/`InstructionSelection`（机器码），不是 `Emit.aura`。**
> 证据：`Emit.aura` 里加的三档符号探针（含一条无条件 `define`）在驱动编出的样例 `.obj` 里
> **一个都没出现**，而同样的字符串确实进了驱动自己的 `.ll`。
> ⇒ P2-B 的落地对象要改成机器码后端（在 ISel/Lowering 层发 `call aura_arc_retain/release`），
> 工作量和语义（寄存器/栈槽值）都与 LLVM IR 侧不同，需重新估。

- 这条缺口也决定 P4-① 的形态：新列表原语（如 `listTruncate`）的拦截必须同时落在
  **(a) `seed/emit.rs`**（让 `aura.exe` 能编译含新调用的 `InstructionSelection.aura`）
  与 **(b) 机器码后端**（让驱动自己能编译），而不是 `Emit.aura`。

## 七、风险与缓解

| 风险                        | 影响       | 缓解                                      |
| ------------------------- | -------- | --------------------------------------- |
| 漏 retain → use-after-free | 崩溃/数据损坏  | magic 校验 + 泄漏报告 + 自举全链验收；`AURA_ARC` 逃生门 |
| 双发射器不一致                   | 两路产物行为分叉 | 规则表共享 + 逐条双侧验收                          |
| native 返回借用指针             | 释放后使用    | P0 审计清单逐个确认（有历史教训记录可循）                  |
| rc 更新性能                   | 时间倒退     | 非原子 + 字面量省略 + 配对消除；实测门禁（自举 ≤ +5%）       |
| 区域指针误 release             | 计数错乱     | D1 时代闸门 + D2c 防线 + magic                |
| 环泄漏                       | 用户程序内存增长 | 文档化 + P4 weak                           |

## 八、验证与度量

- **正确性**：photon 全测试集 + 自举全链 success + `.hat/.obj` 逐字节一致 + BOOT-4；
- **泄漏对账**：退出报告的 live bytes 应与「活结构估算」收敛（~1.5–2GB）；
- **性能门**：自举墙钟增幅 ≤5%（rc 非原子 + 字面量省略 + 配对消除后预期 ≤2%）；
- **内存门**：后端平台期从 5.3GB → 预期 ~2.5GB（活列表 1.5GB + 活结构 1GB）；  
  全局峰值预期 ≤3.5GB。

## 九、为什么 ARC 是这个代码库的正确长期解

过去十轮修的每一类内存 bug（changedSuffixCsv 2.6GB、phiPredMap 1.3GB、  
varMapAdd 761MB、tail-first 10 处……）本质都是同一件事：**没有回收机制时，  
每个「旧值被覆盖」都需要人肉发现并优化**。ARC 把这一整类问题变成机制保证：  
覆盖即释放。前端的区域策略保留（批量回收对「编译完就扔」最优），后端堆时代  
用 ARC（细粒度确定性回收对「长 lived 混合负载」最优）——两种策略按时代分工，  
正好匹配两个时代各自的负载特征，且不需要暂停世界的 GC。
