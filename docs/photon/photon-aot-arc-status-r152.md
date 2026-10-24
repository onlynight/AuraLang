# Photon AOT ARC —— 方案完成度审计（2026-10-21）

> 审计对象：`arc_implementation_plan.md`（P0/P1/P2/P3/P4 + §六 双发射器 + §八 度量门）
> 审计口径：**以代码与可复现验收为准**，不看报告里的自述。

## 一、总表

| 编号 | 计划项 | 状态 | 证据 / 缺口 |
|---|---|---|---|
| P0 | 运行时地基（`AuraMemHdr.rc+magic`、`arc_retain/release`、`.rdata` 区间、泄漏报告） | ✅ R148 | `aura_hdr_init` / `aura_arc_retain` / `aura_arc_release` / `aura_lit_register` / `aura_leak_report_atexit` |
| P1 | dynlist / 字符串原语 ARC 化 | ✅ R148 | `Collections.push` 扩容即 `free(oldItems)`；`listRemoveAt`/`clear` 对被移除元素 release；装箱整数按低位跳过 |
| P2 | 发射注入（seed `emit.rs`） | ⚠️ **部分** | retain 全量注入；release 只有「`x = <expr>` + `String` 静态类型 + 逃逸放行」一条通道。`SCOPE`/`RETURN` 经二分证伪，已改 opt-in（默认关） |
| **P2-B** | **双发射器同步（`aot/Emit.aura`）** | ❌ **未开始** | 10061 行的 `Emit.aura` 里只有 `arcIncrement/arcDecrement` **原语映射**（第 2772–2803 行），**没有任何 ARC 注入**：无 `scan_escapes`、无 `ArcLocal.releasable`、无 `AURA_ARC*` 开关。即方案 §三 G5「双发射器语义一致」与 §六「每条规则双侧同轮落地」**未兑现** |
| P3 | 配对消除 + 受限逃逸分析 + 全链验收 | ✅ R150 | 785s（+7.9%）、存活块 −16.8%、`.hat`/`.obj` 逐字节一致、BOOT-4 通过 |
| P4-度量 | 后端堆载荷归因 | ✅ R151 | `big allocs: 14650 calls / 5433 MB; live 4216 blk / 2867 MB`，见 `photon_aot_arc_p4_2026-10-21.md` |
| P4-① | 选择器状态复用（原「阶段 region 回收」的具体化） | ❌ 未开始（**阻塞点已定位**，见 §三） | 靶点明确：`InstructionSelector` 记账列表 ≈ 2.6GB |
| P4-② | 大临时物分块化 | ❌ **主动降级** | R151 实测：`sbReserve` + `string_concat` 全部站点合计 < 60MB |
| P4-③ | weak / 环 / 多线程原子档 | ❌ 顺延 | |
| §四 D5 | native 返回缓冲审计（「返回静态缓冲」历史教训点） | ✅ **R152（本轮补齐）** | 见 §二 |
| §八 | 峰值 RSS 双口径 | ✅ R151 | `Refresh()` + 删掉误加的清零；`arc-peakprobe.ps1` 验证 20s → 69.8MB |
| — | 代码提交 | ❌ 待办 | 工作区仍有一批未提交改动 |

**结论：5 个阶段里 P0/P1/P3 完成，P2 完成一半（单侧），P4 完成「度量」这一步、优化未开始。真正未兑现的是两条：P2-B（双发射器）与 P4-①（选择器状态复用）。**

## 二、本轮补齐：D5 静态缓冲审计（R152）

方案 §四 D5 留了一句「需专项排查『返回静态缓冲』的历史教训点（`fromCharCode` 注释记载过 static 缓冲互相覆盖）」。全量扫过 76 个返回 `char*` 的 C 导出后，真正的静态缓冲只剩 **2 处**，且都是**有意为之**的高频缓存：

| 函数 | 缓冲 | 为什么是安全的 |
|---|---|---|
| `aura_to_str`（0..1023） | `static char cache[1024][16]` | 槽位写一次、此后只读；按 16B 对齐维持 Plan A「真实指针低位为 0」 |
| `aura_string_charAt` → `aura_char_cache` | `static char slots[256][16]` | 同上；把词法器「每字符一次分配」降为 0（实测省掉 480MB 级分配） |

其余已确认返回**新分配**或 **.rdata 字面量**：`concat`/`substr`/`dup_n`/`strdup`/`fromCharCode`/`env_get`/`base64*`/`path_*` 全部走 `aura_scr_alloc`/`aura_mem_alloc`；`to_str_bool`/`env_platform`/`env_os`/`env_arch` 返回字面量；`aura_time_format` 早已按 P0/D5 改成带头堆分配（源码里有对应注释）。

**问题**：这两段静态缓存落在 **.bss/.data 而非 .rdata**，所以 D2b 的字面量闸门接不住 —— retain/release 会一路掉到 magic 校验失败、被计入 `bad_magic`。功能上安全（no-op），但会**污染 P3 验收赖以判定的 `bad_magic==0` 信号**。

**修法（本轮落地）**：`aura_lit_register()` 增加一张「显式长生区间」表（`g_lit_x_lo/hi`，上限 4 段），把两段缓存与 `.rdata` 同等对待；`aura_ptr_is_lit()` 先查 `.rdata` 再查这张表。两处缓存数组从函数内 `static` 提到文件作用域（`g_int_cache` / `g_char_cache_slots`）以便取地址。

## 三、P2-B 与 P4-① 为什么是同一件事

P4-① 的最小实现是「不要每次 `select()` 都 `this.nodeMapIds = arrayListOf<Int>()`，改成**复用同一数组、把长度置 0**」。落地时撞到一个硬约束：

> `aura/core/aura/lang/collection/ArrayList.aura` 的 `add()` 注释写着：
> 「旧调用 `Collections.listAppend(data, item)` 走 Aura 函数（Long/i64 参数），**与 AOT 不兼容**」
> 必须写成 `data.add(item)`，由**发射器的方法拦截**路由到 i8* 签名的 C 函数。

也就是说，任何新的列表原语（`listTruncate` / `listResizeFill`）都必须：

1. C 侧新增 `aura_lang_std_Collections_listTruncate(i8*, i64)`；
2. Aura 侧 `Collections` 加声明；
3. **seed `emit.rs` 与自举 `aot/Emit.aura` 各自新增方法拦截**（`Emit.aura` 现有 `add` 拦截在 5987 行附近）。

⇒ **P4-① 天然依赖 §六「双发射器同步」**。这也解释了为什么它不能像 P3 那样只动 `emit.rs` 就完成：动单侧会让「驱动编出来的东西」和「驱动自己」行为分叉。

### 建议的落地顺序（下一轮）

1. **先做 §六 的机械对齐**：把 P3 的「`x = <expr>` String 槽覆盖释放 + 逃逸扫描 + 开关诊断」按 D4 表移植到 `Emit.aura`，以「自举第二代的 `.hat`/`.obj` 与第一代一致」验收（ARC 关 → 应零差异；ARC 开 → 两代一致）。
2. **再加 `listTruncate` 双侧拦截**（薄、独立可验）。
3. **最后改 `InstructionSelector`**：5 处重置改复用 + `registerNode` 批量填充 → 预期回收 ~1.4GB 且省掉 1.6 亿次 append。

## 四、本轮验收记录（R152，整链自举）

| 指标 | R151（度量版） | **R152（D5 加固版）** |
|---|---|---|
| 墙钟 | 787.1 s | 801.3 s（+1.8%，在单次 ±2% 噪声内） |
| 退出码 / RESULT | 0 / success | 0 / success |
| `.hat` / `.obj` md5 | 基线一致 | **逐字节一致（PASS）** |
| 产物自检 | BOOT-1…4 PASS | BOOT-1…4 PASS |
| 存活堆块 / 载荷 | 41,275,574 / 4334 MB | **41,275,574 / 4334 MB（完全一致）** |
| `bad_magic` | 0 | 0 |
| **峰值工作集** | 26.8 MB（采样失效） | **4810.2 MB ✅（首次取到真值）** |

**关于 D5 加固的效果**：`lit` 计数两轮完全相同（34,706,697），说明这两段静态缓存
在当前工作负载下**从未被 retain/release 触达** —— 也就是说「注册长生区间」是
**防御性加固**（计数器与产物零变化），不是一次性能/内存优化。它的价值在于：
一旦将来某条释放通道真的落到 `aura_to_str`/`charAt` 的返回值上，会被 O(1) 闸门
静默放行，而不是把 `bad_magic` 打成正数、污染 P3 验收赖以判定的信号。

## 五、R153 更正：自举链的「第二个发射器」不是 `Emit.aura`

方案 §六 假设自举链由 `seed/emit.rs` 与 `aot/Emit.aura` 两个 LLVM IR 发射器构成。**实测不成立**：

- 驱动的编译链（`PhotonPipeline.aura:9` 注释原文）：
  `SsaMir → Lowering → Lir → InstructionSelection → X86Emitter → PhotonObjectWriter(COFF) → PhotonSystemLinker`，
  输出**机器码**，全程不产 LLVM IR；驱动 stdout 里的 `===COFF-MAIN=== / ===LINK===` 也是这条链。
- `PhotonHatCompile.aura` 确实 `import aura.lang.compiler.aot.Emit`，但全文**没有一处 `Emit.` 调用**。
- `Emit.aura`（`LlvmEmitter`）只被 `Aot.aura` 的 `compile()` 使用（Aura 代码自己想走 LLVM 后端时）。
- 决定性证据：在 `Emit.aura` 里加的三档符号探针（`arcpb_a/b/c`，外加一条**无条件**的
  `define void @arcpb_here()`）在驱动编出的样例 `.obj` 里**一个都没出现**；而这些字符串
  确实进了驱动自己的 `drv-p2b.ll`（`grep -c arcpb_a` = 2）⇒ 代码在产物里、就是没执行。

**推论**

1. P2-B 的落地对象要从 `Emit.aura` 改成**机器码后端**（在 ISel / Lowering 层对 String 槽发
   `call aura_arc_retain/release`，操作数是寄存器/栈槽而不是 SSA 值），工作量与语义都要重估。
2. P4-① 的新原语拦截要落在 **(a) `seed/emit.rs`** 与 **(b) 机器码后端**，不是 `Emit.aura`。
3. 本轮已把 `Emit.aura` 侧的 ARC 骨架保留（**惰性、默认关**）：它对 `Aot.aura` 那条 LLVM 链
   仍然有效，且已验证默认路径产物逐字节不变（`14-string-interp.obj` md5 `d0e2670b…`）。

**另一条坑（省下下一轮的时间）**：本类方法里的 `println` 会被**静默丢弃**——字符串进产物、
运行时一行不打。诊断必须用「符号引用」（`llvm-nm` 可见）或往生成的 IR 里写注释。

## 六、P2-B 实施手册（落点已探明；代码仅 `Emit.aura` 骨架）

> 注意：下表的「自举侧」列指 `Emit.aura`（LLVM 链）。若目标是自举链，需按 §五 改到机器码后端。

下一轮做 `Emit.aura` 的 ARC 对齐时，**不用再重新探路**，以下是已经核实过的锚点：

| 需要的件 | seed 侧 | 自举侧（`Emit.aura` / `Runtime.aura`） |
|---|---|---|
| 赋值覆盖点 | `emit.rs:3441-3468`（`emit_assign` 的 `HirExpr::Var` 分支：retain 复制右值 → load 旧值 → release → store） | `Emit.aura:4510 emitAssign`，末尾 `ty`/`coerced`/`addr = declareVar(name, ty)` 之后、`store` 之前 |
| 声明登记点 | `emit.rs:361 declare_var`（登记 `ArcLocal{releasable}`） | `Emit.aura:4345 emitDecl`（`hir.textOf(id)` 就是变量名） |
| 函数入口（做逃逸扫描） | `emit.rs:2675 ctx.arc_escaping = scan.escaping` | `Emit.aura:2199 this.emitBody(hir, bodyId)` 之前（`bodyId` 已在 `emitFunction` 内解析） |
| 逃逸扫描 | `emit.rs:1209-1460 scan_escapes/scan_stmt/scan_expr` | **需按 HIR 扁平表重写**：`hir.kindOf(id)` / `hir.textOf(id)` / `hir.kidsOfList(id)` + `hirKidsCount/hirKidsAt`，可做**通用递归**（不必逐 kind 分支） |
| 运行时符号声明 | `runtime.rs` 的 decl 表 | `Runtime.aura:27 runtimeTable()`（**注意**：该表会注入 `declare` 行，**加任何条目都会改变产物字节**，必须与 `.hat/.obj` 基线同步重定） |
| 开关 | `arc_knob` / `arc_knob_opt_in`（未设=开；opt-in 未设=关） | 同语义，`Env.get("AURA_ARC", "") == "1"` 起步 |

**语句/表达式的 kind 词表（已确认）**：`HirValDecl` / `HirVarDecl` / `HirAssign` / `HirReturn` /
`HirIf` / `HirIfExpr` / `HirWhile` / `HirFor` / `HirBlock` / `HirTry` / `HirBreak` / `HirContinue`；
表达式侧 `HirVar` / `HirLit` / `HirCall` / `HirMember` / `HirIndex` / `HirLambda` / `HirNamedArg`。

**安全方向**：`Emit.aura` 侧的逃逸集可以做得比 seed 更保守（例如「出现在任何 `HirCall` /
`HirMember` / `HirIndex` / `HirLambda` 子节点下的变量一律判逃逸」），得到的是 seed 已验证
集合的**子集** ⇒ 只会少释放、不会多释放。不要反过来放宽。

**快速验证回路（关键，省掉 13 分钟一顿）**：

1. `build-driver.ps1 -Tag X`（约 30s，改的是 `Emit.aura` 也只需重编驱动）；
2. 跑一个样例（`hat-one.ps1 -NoRebuild`，约 0.3s）—— **驱动编译样例时走的就是它自己的
   `Emit.aura`**，所以这一步就能验证自举侧的发射逻辑；
3. 比对样例的 `.obj` 与基线（ARC 关时必须逐字节一致）；
4. 只有到「两侧都自洽」才值得跑一次 13 分钟的全链自举 + 第二代产物比对。

## 六、下一步的度量口径（不变）

- 正确性：`.hat=263a3bbd…` / `.obj=54b33c02…` 逐字节一致 + `BOOT-4`；
- 内存：`AURA_BIG_CENSUS=1` 的 `live` 行 + `AURA_LEAK_REPORT=1` 的 `live heap blocks`；
- 时间：同二进制复跑，单次可信差分下限 ±5%；
- 峰值：`arc-peakprobe.ps1` 或验收脚本的 `peakWS drv`（已修好，需长跑进程）。
