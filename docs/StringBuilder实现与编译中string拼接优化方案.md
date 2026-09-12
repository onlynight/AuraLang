# StringBuilder 实现与编译期 String 拼接优化方案

> 状态：**方案设计（未实施）**
> 目标读者：Aura 编译器（自举）/ VM / AOT 运行库维护者
> 关联文档：`docs/编译器LLVM交互分析与纯Aura化迁移计划.md`、`aura/compiler/README.md`（Phase 6.5 性能优化节）

---

## 1. 背景与目标

### 1.1 背景

Aura 编译器（`aura/compiler/aura/lang/compiler/`，纯 Aura 实现）已可自举：`aura-compiler-native.exe` 能编译自身并产出可执行文件。但**编译期内存占用偏高**（编译自身时峰值 RSS 曾达 5.2 GB）。

经分阶段插桩（`--mem-trace`，见 §2）定位：

- **发射阶段（HIR → LLVM IR 文本）** 是异常增长的主因，已通过「函数体/组装缓冲改为 `List<String>` + 树形归并」修掉超线性部分；
- 进一步细分后发现，剩余内存几乎全部来自 **「用不可变 String 反复拼接」的中间串开销** —— AOT 运行时**没有 GC**，每个中间串都会被永久保留。

### 1.2 目标

| 目标 | 说明 |
|---|---|
| G1 | 在标准库中提供 **StringBuilder**：可增长缓冲区 + 摊还 O(1) 追加，Aura 侧为薄封装 |
| G2 | 用它替换编译器发射/链接/组装路径上的字符串累积，**显著降低编译期内存** |
| G3 | 同一套 Aura 源码在 **VM** 与 **AOT（两条后端路径）** 下行为一致 |
| G4 | 不改变编译器可观测行为（IR 输出、退出码、诊断），不破坏自举链 |

### 1.3 非目标

- 不引入 GC / 引用计数到 AOT 运行时（超出本次范围）。
- 不重写词法/语法层（其开销已在上一轮消除，见 §2.3）。
- 不追求「零分配」；只消除**中间串**与**归并拷贝**。

---

## 2. 现状实测数据

### 2.1 测量方法

编译器入口新增 `--mem-trace`（`Main.aura` → `aotBuildExeFileTraced`），运行库新增 `aura_mem_used_mb()`（C：存活分配 MiB；AOT 侧由 `compiler/src/codegen/aot/runtime.rs` 声明，Aura 侧由 `aot/AotUtil.aura::aotMemMB/aotMemMark` 调用）。

```powershell
$a = ".\build\bin\aura-compiler-native.exe"
& $a .\aura\compiler\aura\lang\compiler\Main.aura -o .\build\self.exe --mem-trace
```

### 2.2 分阶段结果（编译自身：49 模块 / 52454 HIR 节点）

| 阶段 | 存活分配 | 备注 |
|---|---|---|
| start | 0 MB | |
| 链接（Lexer→Parser→AST→HIR 合并） | **57 MB** | 已优化（见 §2.3） |
| 发射 + 工具链 结束 | **303 MB** | 增量 +246 MB |
| 峰值 RSS | **410 MB** | 含 malloc 开销/碎片 |

发射阶段内部细分（临时插桩，`Emit.emitProgram` 打点）：

| 子阶段 | 增量 | 性质 |
|---|---|---|
| 结构体登记 / 函数签名表 | ~0 MB | 纯扫描 |
| **函数体发射**（`emitFunction` × ~140） | **+141 MB** | 每行 IR 中转串 + `fBody` 列表 + `joinPieces` 归并 |
| **最终 IR 组装**（`joinPieces(funcParts)` + `out` 链 + `runtimeDeclarations`） | **+65 MB** | 累积/归并为主 |
| 写 `.ll` + `llc` + `clang` | **+40 MB** | 落盘 + 子进程输出捕获 |

参考量级：自举编译产出的 LLVM IR 文本约 **4.0 MB**（`build/bin/*.ll`）。即发射阶段内存 ≈ IR 体积的 **60×**。

### 2.3 已完成的相邻优化（本方案的前置）

| 轮次 | 改动 | 效果 |
|---|---|---|
| 1 | `Emit.aura` 的 `fBody/fAlloca` 由 `String +=` 改为 `List<String>` + `joinPieces`（136 处） | 发射阶段 +3085 MB → +248 MB |
| 1 | C 运行库：`charAt` 单字符缓存、`toStr` 小整数缓存 | 链接 530 → 427 MB |
| 2 | `Ast`/`Hir` 的 span 由拼接串改为 6 条并列 `List<Int>` | 链接 427 → 402 MB |
| 2 | `Lexer/Token`：关键字表查询由「每次重建 + `split`」改为「实例内缓存查询表」 | 链接 402 → **57 MB** |
| 2 | `Runtime.callSignature` 增加前缀守卫 | 防止普通调用点重建 3 张符号表 |

---

## 3. 问题根因

### 3.1 语言/运行时事实

| 事实 | 位置 | 影响 |
|---|---|---|
| `String` 是**不可变值** | VM：`Value::Str(Rc<str>)`（`compiler/src/vm/value.rs`）；AOT：`char*`（`aura_std_cffi.c`） | 无「就地追加」能力，`a = a + b` 必然新分配 |
| AOT 运行时**只分配不释放**（无 GC/ARC 生效路径） | `aura_std_cffi.c` 的 `aura_mem_alloc/realloc` + 内存闸门注释 | 每个中间串永久占用内存 |
| 字符串分配带 16 字节头 + malloc 开销 | `AuraMemHdr` | 短串的实际成本 ≈ 56~80 字节（内容仅数十字节） |
| 发射器是「文本生成」型：每条 IR 指令由多个片段拼成 | `Emit.aura` | 分配次数 ≈ IR 行数 × 每行片段数 |

### 3.2 三类开销

1. **累积开销**：`acc = acc + x`（现已改列表）与 `joinPieces` 的**树形归并**中间串。
   树形归并总拷贝量 ≈ `0.5 · N · log₂(P)`（N=总长度，P=片段数）。对 4 MB / 十万级片段，量级为数十 MB。
2. **每行中转串**：`"  " + t + " = call i8* @" + f + "(" + args + ")\n"` —— 每个 `+` 都是一次分配，且**StringBuilder 不会自动消除**，必须改写成逐段 append 才能节省。
3. **工具链**：把 4 MB IR 写盘、捕获 `llc`/`clang` 输出（属固定成本，本方案不重点优化）。

### 3.3 为什么「纯 Aura 的 StringBuilder」无效

| 纯 Aura 做法 | 问题 |
|---|---|
| `class StringBuilder { var parts: List<String>; fun build() { return joinPieces(parts, "") } }` | **等价于现状**（`fBody` 已是 list + `joinPieces`），收益为 0，只是 API 糖 |
| `List<Int>` 存码点（8 B/字符），末尾需要 `charsToString` | 缓冲 4 MB IR → 32 MB（含翻倍浪费更大）；末尾仍需**原生**批量转换；CPU 也比 `memcpy` 差 |
| 「本地小块累积 + 定期 flush」 | 块内仍是 `+=`（O(k²)），无 GC 会把中间串全部留下，实测可放大到 ~50× 数据量，**比现状更差** |

**结论**：要获得实质收益，必须提供**原生 StringBuilder 原语**（AOT：C 运行库；VM：Rust native），Aura 侧仅做薄封装。

---

## 4. 设计约束

| 编号 | 约束 | 影响 |
|---|---|---|
| C1 | `Emit.aura` 必须**同时**在 VM（`tests/phase6_5_aot_tests.aura`、`aura run`）和 AOT（自举）下运行 | VM native 与 AOT 实现缺一不可 |
| C2 | Aura 编译器有**两条 AOT 路径**：① Rust AOT 后端（`aura build --aot`，用于产出 `aura-compiler-native.exe`）；② Aura 侧自研发射器（`aot/Emit.aura`+`StdSigs.aura`，用于自举） | 两侧符号表都要登记 |
| C3 | Aura 是**扁平函数命名空间**，类方法名须全局唯一（见 `Parser.aura` 顶部说明） | 新方法名需加前缀（`sb*` 或 `StringBuilder.*`） |
| C4 | AOT 下 `i8*` 低位被 Plan A 用作「装箱整数」判定 | 句柄类型选择需谨慎 |
| C5 | Rust 编译器（`compiler/`）保持「尽量少改、可回退」 | 改动限制在纯新增条目 |

---

## 5. 方案对比与推荐

| 方案 | 做法 | 收益 | 成本/风险 | 结论 |
|---|---|---|---|---|
| **A** | 原生 StringBuilder（句柄 + `append/appendInt/finish`） | 上限最高：累积开销 → 0；配合阶段 3 可消除大部分每行中转串 | 跨 4 层（C / Rust-VM / Rust-AOT 符号表 / Aura 发射器符号表） | **推荐的终态** |
| **B** | 只加原生「批量 join(list, sep)」 | 消除 `joinPieces` 树形归并的全部中间串 | 1 个原生 × 2 后端 + 2 处符号表 | **推荐先做（低风险）** |
| **C** | 纯 Aura StringBuilder（list + joinPieces） | 0（API 糖） | 无 | 不采用 |
| **D** | 纯 Aura 分块缓冲 | 负收益 | 无 | 不采用 |

**推荐路线**：`阶段 0（插桩确认）` → `阶段 1（方案 B）` → `阶段 2（方案 A）` → `阶段 3（可选：Emit 表达式改 append）`。

---

## 6. 详细设计

### 6.1 命名与契约

采用与现有 std 一致的「**调用点符号**」命名（`sanitize(aura.lang.std.X.y)` → `aura_lang_std_X_y`），这样能复用 `stdSignature` / `stdSymbolFor` / `isStdClassName` 三处既有机制。

| 层 | 名称 | 备注 |
|---|---|---|
| Aura 声明 | `object StringBuilder { ... }` | 见 §6.5，bodyless（native） |
| VM 注册名 | `aura.lang.std.StringBuilder.new/append/appendChar/appendInt/length/finish/reset` | `compiler/src/std/std_sb.rs` |
| AOT 调用点符号 | `aura_lang_std_StringBuilder_new/append/...` | C 运行库直接按此名导出（与 `String`/`Collections` 同风格） |
| Aura 侧发射器符号表 | 同上，登记在 `StdSigs.aura::stdSignatureTable()` + `AotUtil.isStdClassName` 增加 `"StringBuilder"` | |
| Rust AOT 符号表 | `compiler/src/codegen/aot/runtime.rs::cffi_signature` 增加同名条目 | 注意 `translate_to_legacy_c` 对未登记类名原样返回，故直接按全名匹配即可 |

**API 契约（LLVM 类型）**：

| 函数 | 返回 | 参数 | 语义 |
|---|---|---|---|
| `aura_lang_std_StringBuilder_new` | `i64` | — | 新建 builder，返回句柄 |
| `aura_lang_std_StringBuilder_append` | `i64` | `i64, i8*` | 追加字符串，返回句柄（便于链式） |
| `aura_lang_std_StringBuilder_appendChar` | `i64` | `i64, i64` | 追加单字符（码点/字节） |
| `aura_lang_std_StringBuilder_appendInt` | `i64` | `i64, i64` | 追加十进制整数（等价 `toStr` 但**不产生中间串**） |
| `aura_lang_std_StringBuilder_length` | `i64` | `i64` | 当前长度 |
| `aura_lang_std_StringBuilder_finish` | `i8*` | `i64` | **转移缓冲区所有权**并返回字符串；原句柄失效 |
| `aura_lang_std_StringBuilder_reset` | `i64` | `i64` | 清空（保留容量），返回句柄 |
| `aura_lang_std_StringBuilder_free` | `void` | `i64` | 可选；AOT 下为空实现 |

> **句柄用 `i64` 而非 `i8*`**：AOT 侧 `int64_t` 承载指针，避免与 Plan A「低位标记整数」判定、以及 Rust AOT 后端 `i8*`/结构体表示混用的历史坑（见 README 中 `identity.aura` 复现）。VM 侧自然对应 `Value::Int` 或 `Value::Ptr`。

### 6.2 AOT 运行库实现（`compiler/src/std/cffi/aura_std_cffi.c`）

```c
typedef struct {
    char  *buf;   /* 数据区（16 字节对齐，见 aura_mem_alloc） */
    int64_t len;  /* 已用字节（不含结尾 NUL） */
    int64_t cap;  /* 容量（字节，含结尾可用位） */
} AuraSb;
```

要点：

1. **分配全部走 `aura_mem_alloc/aura_mem_realloc`**，保持内存闸门（`AURA_MEM_LIMIT_MB`）记账一致；重启不放回 OS（与现状一致）。
2. **增长策略**：初始 256 B，`cap < need` 时 `max(cap*2, need)`；几何增长保证 append 摊还 O(1)、总拷贝 O(N)。
3. **`finish` 零拷贝**：直接把 `buf` 交给调用方（返回指针），句柄置为失效（`buf=NULL`）。这样「一个函数体/一个模块」只产生 **1 次最终分配**，之前的缓冲区增长由 `realloc` 完成。若调用方需要继续使用，用 `reset` 重新分配。
4. **Plan A 兼容**：`aura_mem_alloc` 返回的负载天然 16 对齐；`finish` 返回的字符串指针低位恒为 0，不会被误判为装箱整数（与 `aura_dup_n`/argv 注释同源）。
5. **`appendInt`**：`snprintf` 到栈缓冲再 memcpy，避免 `aura_to_str` 的中间分配。
6. 若需要 `join(list, sep)`（方案 B），在 `AuraDynList` 上实现：
   - 第一遍求总长（`strlen` 累加 + sep 长度），一次 `aura_mem_alloc`；
   - 第二遍 memcpy；
   - **前提**：元素必须是字符串指针（见 §8 决策）。

### 6.3 VM 实现（`compiler/src/std/std_sb.rs`，新增）

VM 的 native 形状为 `fn(&[Value]) -> Value`（`compiler/src/vm/native.rs::NativeFn`）。

```rust
struct SbRegistry { items: HashMap<i64, String>, next_id: i64 }
pub fn get_sb_registry() -> &'static Mutex<SbRegistry> { /* OnceLock + Mutex */ }
```

要点：

1. 沿用 `std_net.rs::SocketRegistry` 的**全局句柄注册表**模式（`OnceLock<Mutex<...>>`）。
2. `sb_new` → 分配 id，返回 `Value::Int(id)`（或 `Value::Ptr(id)`，二选一，需与 Aura 侧声明的 `Long` 对齐）。
3. `sb_append` → `registry.items.get_mut(&id).push_str(s0(args))`（**摊还 O(1)**，Rust `String` 自带几何增长）。
4. `sb_finish` → `Value::Str(Rc::from(std::mem::take(&mut items[&id])))`（**移动，无拷贝**）。
5. 注册：
   - `compiler/src/std/mod.rs`：新增 `pub mod std_sb;`、`register_all`/`register_with_modules` 增加 `"sb"` 分支、`module_name_from_path` 增加 `"StringBuilder" => Some("sb")`。
   - `compiler/src/std/decl.rs`：在 `build_all_names()` 增加 `aura.lang.std.StringBuilder.*` 条目（**必须**，否则 sema 报未解析、HIR 形态也会变）；其单元测试有「按模块前缀断言」的用例，需同步。

### 6.4 符号层打通（共 3 条路径）

| 路径 | 文件 | 具体动作 |
|---|---|---|
| AOT-Rust 后端声明 | `compiler/src/codegen/aot/runtime.rs` | `RUNTIME_FUNCTIONS` 增加 `aura_lang_std_StringBuilder_*`（若走 runtime 表）**或** `cffi_signature` 增加同名条目（推荐后者，与 `Collections` 一致） |
| AOT-Aura 发射器：签名 | `aura/compiler/aura/lang/compiler/aot/StdSigs.aura::stdSignatureTable()` | 追加同上条目（格式 `符号\|ret\|params`） |
| AOT-Aura 发射器：类识别 | `aura/compiler/aura/lang/compiler/aot/AotUtil.aura::isStdClassName` | 增加 `"StringBuilder"` |
| AOT-Aura 发射器：方法名映射 | `aura/compiler/aura/lang/compiler/aot/Runtime.aura::methodCallSymbol` | 若以「裸方法名」形式调用（`sbAppend(h,s)`），需把 `sbAppend`/`sbFinish`/… 映射到调用点符号；若以 `StringBuilder.append(...)` 形式调用，由 `stdSymbolOfQualified` 自动处理，无需改此表 |
| AOT 声明去重 | `aot/Runtime.aura::runtimeDeclarations` | 新符号会被 `stdSignatureTable()` 自动加入 `declare` 列表 |

> ⚠️ 注意上一轮在 `Runtime.aura::callSignature` 中加入的前缀守卫：非 `aura_` / `toString*` / `toStr` 开头的名字会**提前返回**。若走 `aura_lang_std_StringBuilder_*` 调用点符号，会在 `stdSignature()` 阶段命中（守卫之前），无需改动；若改用别的名字前缀，**必须同步扩展该守卫白名单**。

> ⚠️ Rust AOT 后端要正确发射调用点，函数必须被**前端识别为内置 native**（落入 `program.natives`，见 `emit.rs` 的 `func_ret_types` 预热），因此 §6.3 的 `decl.rs` 登记与 §6.5 的 Aura 声明是**必要条件**，不能只加符号表。

### 6.5 Aura 侧 API

新建 `aura/core/aura/lang/std/StringBuilder.aura`，模板参照 `std/FileSystem.aura`（`internal object` + bodyless native）：

```aura
package aura.lang.std

/// 可变字符串缓冲区（原生实现：AOT 见 aura_std_cffi.c，VM 见 std_sb.rs）。
internal object StringBuilder {
    /// 新建缓冲区，返回句柄。
    fun create(): Long
    /// 追加字符串（返回自身句柄，便于链式）。
    fun append(handle: Long, text: String): Long
    /// 追加单字符。
    fun appendChar(handle: Long, ch: Char): Long
    /// 追加十进制整数（不产生中间字符串）。
    fun appendInt(handle: Long, value: Int): Long
    /// 当前长度（字节）。
    fun length(handle: Long): Int
    /// 结束并交出内容；原句柄失效。
    fun finish(handle: Long): String
    /// 清空（保留容量）。
    fun reset(handle: Long): Long
}
```

同时保留一个**纯 Aura 回退实现**（供 `joinPieces` 在 VM 未注册/未来裁剪时使用）：

```aura
/// 纯 Aura 树形归并（现状实现，保留为回退）。
fun joinPiecesPure(parts: List<String>, sep: String): String { /* 现有实现 */ }
```

**编译器内部使用方式**（示意，`Emit.aura`）：

```aura
// 原：this.fBody.add("  " + t + " = call ...")
// 方案 B（先做）：保持不变，仅把最终 joinPieces 换成原生 join
// 方案 A（终态）：
this.fSb = StringBuilder.create()
...
StringBuilder.append(this.fSb, "  ")
StringBuilder.append(this.fSb, t)
StringBuilder.append(this.fSb, " = call i8* @")
...
// emitFunction 末尾：
return joinPieces(this.fHead, "") + StringBuilder.finish(this.fSb)
```

### 6.6 编译器落地改造点

| 位置 | 现状 | 改法 |
|---|---|---|
| `aot/AotUtil.aura::joinPieces`（:205） | 纯 Aura 树形归并 | 改为委托原生 `join`（方案 B），保留 `joinPiecesPure` 回退 |
| `aot/Emit.aura`：`stringParts/structParts/funcParts`（:61~67） | `List<String>` + `joinPieces` | 方案 B 即可（原生 join）；方案 A 换成 sb |
| `aot/Emit.aura`：`fBody`/`fAlloca`（:78~81） | `List<String>` + `joinPieces`（:753~754） | 同上 |
| `aot/Emit.aura::emitProgram` 的 `out` 链（:254~273） | `out = out + …` × 7 | 改为 sb append，最后 `finish`（省掉若干次整串拷贝） |
| `aot/ModuleLink.aura::loadPath`（:118） | `joinPieces(parts, ",")` | 复用原生 join |
| `aot/CBackend.aura::?`（:51） | `joinPieces(parts, "")` | 同上 |
| `aot/Emit.aura` 的逐行拼接（emitStmt/emitExpr/emitCall/coerceValue，~130 处） | `"a" + b + c` | **阶段 3**：改为 `sb` 逐段 append（优先改最热的 3~5 个函数） |

---

## 7. 分阶段实施计划

### 阶段 0：插桩确认（约 0.5 h，先做）

**目的**：确认 +141 MB 中「累积/归并」与「每行中转串」的占比，决定后续投入。

- 在 `Emit.emitFunction` 内按「每 N 个函数」打点（`aotMemMark`），或在 `emitBody/emitStmt` 入口按深度打点（临时）。
- 交付：一份比例数据（累积 : 每行中转）。
- 回退：删除临时打点。

### 阶段 1：原生批量 join（方案 B，低风险）

| 项 | 内容 |
|---|---|
| 改动 | C：`aura_lang_std_Collections_join(list, sep) -> i8*`（一次分配 + 两次遍历）<br>VM：`std_string.rs` 或新 `std_sb.rs` 注册 `aura.lang.std.Collections.join`（当前 `nat_join` 语义错误，需修正）<br>Rust AOT：`cffi_signature` 增加条目<br>Aura 发射器：`StdSigs.stdSignatureTable()` 增加条目<br>Aura：`AotUtil.joinPieces` 改为调用原生 + 保留 `joinPiecesPure` |
| 验收 | phase1–9 全绿；`--mem-trace` 复测；自举链成功；`tests/aot/*` 输出一致 |
| 回退 | 把 `joinPieces` 改回纯 Aura |
| 预估 | 发射阶段 −30~60 MB，峰值 RSS 410 → ~360 MB |

### 阶段 2：StringBuilder（方案 A）

| 项 | 内容 |
|---|---|
| 改动 | §6.1~6.5 全部；`Emit.aura` 的 `fBody/fAlloca/funcParts/stringParts/structParts/out` 改为 sb |
| 验收 | 同上 + `fBody` 相关 IR 断言（phase6.5）逐条通过 |
| 回退 | 保留 list 实现分支（编译期开关或直接回滚改动） |
| 预估 | 发射阶段再 −80~150 MB，峰值 RSS → ~250 MB |

### 阶段 3（可选，收益最大、工作量最大）：消除每行中转串

| 项 | 内容 |
|---|---|
| 改动 | `emitStmt/emitExpr/emitCall/coerceValue/inferType` 等热点：`"a" + b + c` → `sb.add("a").add(b).add(c)` |
| 范围控制 | 先做调用频次最高的 3~5 个函数（可用临时计数器确定），不必一次改完 130 处 |
| 验收 | 同上；IR 输出**逐字节一致**（迁移期间可用 `--emit-ir` 对比） |
| 预估 | 发射阶段 → ~40~60 MB，峰值 RSS → ~200 MB |

---

## 8. 关键设计决策与取舍

| 决策 | 选择 | 理由 / 备选 |
|---|---|---|
| 句柄类型 | `i64`（`Long`） | 避免 Plan A 低位判定与 `i8*`/结构体表示混用；VM 侧天然对应 `Value::Int`/`Value::Ptr`。备选 `i8*`（更贴近 C，但 Aura 侧类型推断需 `Any`，风险高） |
| 是否提供 `free` | 提供但 AOT 为空实现 | AOT 本来就只分配不释放；VM 可真正移除，避免注册表常驻 |
| `finish` 语义 | **转移所有权**（零拷贝） | 编译器用法为「最后一次使用后丢弃」，零拷贝最优；如需续用则 `reset` |
| 分隔符处理 | `joinPieces` 的 `sep` 在原生 join 中支持 | `ModuleLink` 用 `","`，`Emit` 用 `""` |
| `join` 元素类型 | 仅限 `List<String>`（文档 + sema 校验） | AOT 下 `List` 元素是不透明 `i8*`，整数为 Plan A 装箱值，原生无法自证类型 |
| 命名风格 | `aura_lang_std_StringBuilder_*` 调用点符号 | 复用 `stdSignature`/`stdSymbolFor`/`isStdClassName` 既有机制；绕开 `callSignature` 新前缀守卫 |
| 是否做用户可见类 | 先只做 `object`（静态方法） | 编译器是唯一使用方；后续再包一层 `class StringBuilder` 供用户使用（方法名需 `sb*` 前缀） |
| VM 句柄泄漏 | 接受常驻（编译器用量极小） | 与 AOT 行为对齐；如需可加 `free` |

---

## 9. 风险与缓解

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| R1 | 三路（VM / AOT-Rust / AOT-Aura 发射器）签名或语义不一致 | 链接期 undefined symbol、IR 非法、或 VM 下不可用 | 阶段 1 先在最小用例（`tests/aot/string_runtime.aura`）验证；建一张「符号 × 路径」核对表 |
| R2 | `Emit.aura` 只在 VM 或只在 AOT 通过 | 自举链断裂 | 每阶段同时跑 `aura run tests/phase6_5_aot_tests.aura` 与自举链 |
| R3 | 前端未把新函数识别为 native（未改 `decl.rs`） | HIR 形态变化 → 未解析调用退化 | 把 `decl.rs` 登记列为阶段 1/2 的**必做项**，并补 `decl.rs` 单元测试断言 |
| R4 | `callSignature` 前缀守卫拦截新符号 | 调用点退化为 i32/未声明 | 使用 `aura_lang_std_` 前缀（守卫之前命中）；若换前缀则同步扩展白名单 |
| R5 | `join` 误用于非 `List<String>` | 把装箱整数当字符串 → 崩溃/乱码 | 仅限编译器内部调用点；用户 API 加 sema 校验 |
| R6 | `finish` 后误用同一句柄 | 读到空/野指针 | 返回句柄失效约定 + 注释；`reset` 重新分配 |
| R7 | 改动 `compiler/` 触发 Rust 侧回归 | 现有测试失败 | 仅新增条目；跑 `cargo test -p compiler --lib` 相关子集 |
| R8 | 内存记账不一致（绕过 `aura_mem_*`） | `AURA_MEM_LIMIT_MB` 闸门失效 | code review 检查：所有分配必须走 `aura_mem_*` |

---

## 10. 测试与验证方案

### 10.1 功能回归矩阵

| 层次 | 命令 | 期望 |
|---|---|---|
| Aura 侧测试 | `aura run tests/phase1_lexer_tests.aura` … `phase9_compiler_tests.aura` | 全部 `RESULT: PASS`（含 phase6.5 的 IR 结构断言） |
| AOT 运行样例 | `build/bin/aura-compiler-native.exe tests/aot/string_runtime.aura -o x.exe && x.exe` | `aot.string_runtime ok` |
| Rust 单测 | `cargo test -p compiler --lib` | 相关子集通过（`aot::runtime`、`std::decl`） |
| VM 一致性 | 同一 Aura 程序分别用 `aura run`（VM）与原生 exe 运行 | 输出与退出码一致 |

### 10.2 自举链

```powershell
# 1) 用 Rust 后端产出原生编译器
.\target\release\aura.exe build .\aura\compiler\aura\lang\compiler\Main.aura --aot --output .\build\bin\aura-compiler-native.exe
# 2) 原生编译器自编译
.\build\bin\aura-compiler-native.exe .\aura\compiler\aura\lang\compiler\Main.aura -o .\build\g1.exe
# 3) 二代编译器编译样例并运行
.\build\g1.exe .\examples\basics\hello-world.aura -o .\build\g1h.exe
.\build\g1h.exe    # 期望输出 Hello, Aura!
```

### 10.3 IR 等价性

迁移期间，对固定输入比较「改动前 / 改动后」产出的 `.ll` **逐字节一致**（上一轮已用此法验证 `fBody` 重构无副作用）：

```powershell
& $before <input.aura> -o build\_v\a.exe ; & $after <input.aura> -o build\_v\b.exe
(Get-FileHash build\_v\a.ll).Hash -eq (Get-FileHash build\_v\b.ll).Hash
```

### 10.4 内存指标

每阶段记录并对比：

| 指标 | 采集方式 |
|---|---|
| link 存活 | `--mem-trace` 的 `link done` 行 |
| emit 存活 | `--mem-trace` 的 `emit + toolchain done` 行 |
| 峰值 RSS | 外部探针（`Start-Process` + 轮询 `WorkingSet64`） |
| 耗时 | 同上 |

---

## 11. 预期收益（基于 §2 实测外推，标注为预估）

| 阶段 | link | emit 增量 | 峰值 RSS | 备注 |
|---|---|---|---|---|
| 现状 | 57 MB | +246 MB | 410 MB | |
| 阶段 1（原生 join） | 57 MB | ~+190 MB | ~360 MB | 消除树形归并中间串 |
| 阶段 2（StringBuilder） | 57 MB | ~+110 MB | ~250 MB | 累积开销归零 |
| 阶段 3（表达式改 append） | 57 MB | ~+45 MB | ~200 MB | 消除每行中转串 |

> 相对最初的 5232 MB 峰值，完整落地后预期降至约 200 MB（≈26×）。

---

## 12. 附录

### 附录 A：改动文件清单（预估）

| 层 | 文件 | 变更 |
|---|---|---|
| C 运行库 | `compiler/src/std/cffi/aura_std_cffi.c` | 新增 `AuraSb` + `aura_lang_std_StringBuilder_*` + `aura_lang_std_Collections_join` |
| Rust VM | `compiler/src/std/std_sb.rs`（新）、`std/mod.rs`、`std/decl.rs`（+ 可选 `std_string.rs` 修正 `nat_join`） | 注册 + 句柄注册表 + 命名登记 |
| Rust AOT | `compiler/src/codegen/aot/runtime.rs` | `cffi_signature` 条目 |
| Aura 标准库 | `aura/core/aura/lang/std/StringBuilder.aura`（新） | native 声明 |
| Aura 发射器 | `aot/StdSigs.aura`、`aot/AotUtil.aura`（`isStdClassName`、`joinPieces`）、`aot/Runtime.aura`（可选 `methodCallSymbol`） | 符号表 + 类识别 + join 委托 |
| Aura 编译器热路径 | `aot/Emit.aura`、`aot/ModuleLink.aura`、`aot/CBackend.aura` | sb/join 替换 |

### 附录 B：符号 × 路径核对表（实施时逐格打勾）

| 函数 | C 实现 | Rust-AOT `cffi_signature` | Aura `stdSignatureTable` | `isStdClassName` | VM `register` | VM `decl.rs` | Aura 声明 |
|---|---|---|---|---|---|---|---|
| `StringBuilder.create` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.append` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.appendChar` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.appendInt` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.length` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.finish` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `StringBuilder.reset` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |
| `Collections.join` | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ | ☐ |

### 附录 C：常用验收命令

```powershell
# 测试套件
foreach ($t in @('phase1_lexer_tests','phase2_sema_hir_tests','phase3_mir_tests',
                 'phase5_vm_tests','phase6_5_aot_tests','phase7_jit_tests',
                 'phase8_stdlib_tests','phase9_compiler_tests')) {
    & .\target\release\aura.exe run "tests\$t.aura" 2>$null | Select-String "RESULT:"
}

# 内存分阶段
$env:AURA_MEM_LIMIT_MB="8192"
& .\build\bin\aura-compiler-native.exe .\aura\compiler\aura\lang\compiler\Main.aura -o .\build\self.exe --mem-trace

# Rust 侧相关单测
cargo test -p compiler --lib aot::runtime
cargo test -p compiler --lib std::decl
```

---

## 13. 结论

1. **必须**提供原生 StringBuilder（纯 Aura 实现无法降低累积开销，分块缓冲反而更差）。
2. 建议**先做方案 B（原生批量 join）**：改动面最小、风险最低，可立即回收树形归并的中间串。
3. 再做**方案 A（StringBuilder）**：把 `Emit.aura` 的缓冲从「列表 + 归并」改为「单缓冲区 append + finish 零拷贝」，累积开销归零。
4. **阶段 3** 才是消除「每行中转串」的关键，属 `Emit.aura` 重构，建议在 1/2 生效并回归通过后按热点分步推进。
5. 全程保持「IR 逐字节一致 + VM/AOT 双跑 + 自举链可用」三条红线，任何阶段失败均可独立回退。
