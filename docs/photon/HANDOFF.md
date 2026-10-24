# AuraLang Photon 自举（Step 4c）交接文档

> 用途：跨对话交接。读完本文可独立继续推进 Photon 后端自举。
> 状态时间点：自举链路「零未定义符号 + 可运行 + CLI 参数通道打通 + 无死循环」，
> 剩余唯一拦路石是 **字符串判等 / 分支控制流（P0）**。
>
> ⚠️ **构建环境注意**（2026-09-28 更新）：
> 当前 `aura.exe`（`seed/target/release/aura.exe`，7.5 MB，含 `llvm` feature）
> 支持 `run`/`compile`/`check`/`build`/`version` 五个命令。
> `build` 命令已实现（`main.rs::cmd_build`），支持 `aura build <entry> --aot --output <path>`。
> AOT 后端两个崩溃点已修复（`not an assignable lvalue`、`getelementptr i8* 0`）。
> **Import 解析已修复**（`photon_pkg_root` 路径、`canonicalize("")` Windows 兼容、`@native` 无括号解析）。
> **驱动已重建成功**（`build/hat-native/PhotonHatCompile.exe`，1.38 MB）。

## 本轮修复（2026-09-28 第五轮）

### Console_println 符号名转换修复

**问题**：代码生成的是 `@aura_lang_std_Console_println`，但 IR 中是 `@Console_println`。
前端（`Hir.aura::lowerCall` 的 P1 fix）把大写接收者名的成员调用写为 `ClassName_method`，
因此 `Console.println(msg)` 在 HAT IR 里是 `@Console_println(...)`，而 runtime 实现的名字
是 `println`（`PhotonRuntime.aura::RUNTIME_SYMBOLS`）。

**修复**（`InstructionSelection.aura::mapStdlibFuncName`）：

新增映射：
- `Console_println` / `Console.println` / `aura.lang.std.Console.println` / `aura_lang_std_Console_println` → `println`
- `Console_print` / `Console.print` / `aura.lang.std.Console.print` / `aura_lang_std_Console_print` → `print`
- `IO_println` / `IO.println` / `aura_lang_std_IO_println` → `println`
- `IO_print` / `IO.print` / `aura_lang_std_IO_print` → `print`
- `Stdio_println` / `Stdio.println` → `println`
- `Stdio_print` / `Stdio.print` → `print`

**验证**：重建驱动后，HAT 输出中 `@println` 出现 181 次，`@Console_println` 出现 0 次 ✓

### Ast 工具方法兜底映射修复（P1 残留）

**问题**：`kindOf` / `textOf` / `childOf` / `tyOf` 同时被 `Ast`、`Hir`、`Mir` 三个类声明，
`receiverClassOf` 返回 "" 导致 `resolveMethodSymbol` 退化成裸名 → `undefined symbol`。

**修复**（`InstructionSelection.aura::mapStdlibFuncName`）：

新增类型消歧 + 兜底映射（与 `leaf`/`kidsOf`/`spanOf` 同源）：
- `kindOf`：按 `recvTy` 分流到 `Ast_kindOf` / `Hir_kindOf` / `Mir_kindOf`；兜底 `Ast_kindOf`
- `textOf`：按 `recvTy` 分流到 `Ast_textOf` / `Hir_textOf` / `Mir_textOf`；兜底 `Ast_textOf`
- `childOf`：按 `recvTy` 分流到 `Ast_childOf` / `Hir_childOf` / `Mir_childOf`；兜底 `Ast_childOf`
- `tyOf`：按 `recvTy` 分流到 `Ast_tyOf` / `Hir_tyOf` / `Mir_tyOf`；兜底 `Ast_tyOf`

### 编译崩溃调查（0xC0000005）

**现象**：重建驱动后，`PhotonHatCompile.exe` 在编译任何测试文件时均以
`exit code 0xC0000005`（访问违规）崩溃，stdout/stderr 均为空。

**定位**：Windows Event Log 显示崩溃偏移固定为 `0x2a99e`，反汇编显示：
```asm
14002a99e: 48 8b 09                     movq  (%rcx), %rcx   ← 解引用 rcx，rcx 为 null
```
该指令在保存 `rcx`/`rdx` 到栈后、调用另一函数后执行，模式为「读取对象字段」。
推测崩溃在 `AotModuleLinker` 构造（`Hir()` / `HirLowerer()` / `ArrayList()`）
或 `link()` 早期的对象初始化阶段。

**验证**：
- 无参数运行 → 正常输出 `===RESULT===fail`（`Env.get` 正常）
- 不存在的文件路径 → 正常输出 `===ERR===source not found`（`FileSystem.exists` 正常）
- 存在的文件路径 → 崩溃（前端 `AotModuleLinker.link` 或 `SsaBuilder` 阶段）

**根因推测**：`ModuleLink.aura` 注释已记录 AOT 后端的两个已知崩溃点：
1. `HashMap()` 构造即崩溃（EXIT 0xC0000005）
2. `ArrayList` 实例与 `AuraDynList` 布局不匹配（16 vs 24 字节）→ `realloc` 段错误

当前崩溃很可能仍由这两个问题之一触发。代码已刻意规避（用扁平字符串替代 HashMap、
顺序读取替代并行），但 `Hir()` / `Ast()` 构造函数内部的 `arrayListOf<String>()`
仍可能命中该缺陷。

**待办**：需要在 AOT 后端修复 `ArrayList` / `HashMap` 的对象布局与集合句柄传递。
这是自举链路的根本性阻塞，需优先解决。

### 本轮修复（2026-09-28 第六轮）— 对象布局缺陷修复

**根因定位**：`movq (%rcx), %rcx`（rcx=0）崩溃的根因是**字段默认值初始化**与
**列表 `add` 映射**的双重缺陷：

#### 缺陷 1：`fieldDefaultNode` 只接受字面量，非字面量默认值被零初始化

`SsaBuilder.aura::fieldDefaultNode` 此前只接受 `HirLit`（字面量），导致
`= arrayListOf<String>()` 这类**非字面量默认值**被忽略，字段零初始化为 null。

`Hir` / `Ast` / `Mir` 等核心类的所有列表字段（`kinds` / `texts` / `tys` 等）
全部初始化为 null（0）。构造后首次 `add` 操作读取 `[null]` 即触发
`movq (%rcx), %rcx`（rcx=0）崩溃。

**修复**：`fieldDefaultNode` 新增 `HirCall` 支持（`arrayListOf()` 是纯函数调用，
不引用 `this`/局部变量，在构造器合成阶段求值安全）。

#### 缺陷 2：`synthesizeCctorBody` 只在 init 重载时调用 init

`SsaBuilder.aura::synthesizeCctorBody` 此前只在 `initOverloaded(cname)` 为 true
时调用 `<Class>_init<ar>`。单 init 类（如 `Hir` / `Exception`）的 init 体被完全跳过，
构造参数无处落地。

**修复**：改为 `classDeclaresMethod(cname, "init")` 判定；重载时加 `<ar>` 后缀，
非重载时用裸名，与 `buildFunction` / `buildCall` 一致。

#### 缺陷 3：`add` 映射到 `ArrayList_add`（类方法），与裸句柄布局不匹配

`arrayListOf()` 创建的**裸列表句柄**（`[count:8][e0:8][e1:8]...`）与
`ArrayList` 类对象（`[vtable:8][size:8][data:8][size:8]`）布局不同。
`add` 被映射到 `ArrayList_add`（类方法），而类方法期望 `this` 是 `ArrayList` 对象，
实际传入的是裸句柄 → 布局不匹配 → 读取野值 → 段错误。

**修复**：
- 新增 `Collections.listAppend` runtime 函数（直接操作裸句柄布局：读 count → 写入 → count+1）
- `InstructionSelection.aura::mapStdlibFuncName` 中 `add` 映射改为 `Collections.listAppend`
- `SsaBuilder.aura::collectionRuntimeSymbol` 新增 `add → Collections.listAppend`

#### 构建环境阻塞

重建驱动时 `aura.exe build --aot` 报 `invalid redefinition of function 'String_charAt'`
（LLVM IR 重复定义，与本轮修复无关，属预存问题）。
`aura.exe build` 命令的 `--aot` 标志必须放在**文件路径之后**（否则参数解析错误）。

**待办**：修复 `String_charAt` 重复定义问题后重建驱动，验证本轮三项修复。

## 0. 环境与验证流程（新对话直接照做）

工作目录 `d:/Code/AuraLang`。

**每次改动 `aura/compiler/aura/lang/compiler/**` 后必须重建驱动**，否则跑的是旧驱动（已多次踩坑）。

```powershell
# ① 重建驱动（用全新缓存目录避免陈旧对象；约 2–4 分钟）
$env:AURA_CACHE_DIR="build\cache-cleanNN"          # NN 每轮递增
.\rust\target\release\aura.exe build --aot aura\compiler\aura\lang\compiler\backend\photon\PhotonHatCompile.aura --output build\hat-native\PhotonHatCompile.exe

# ② 编译自举模块（约 2 分钟）
$env:AURA_HAT_AURA="d:\Code\AuraLang\aura\compiler\aura\lang\compiler\Main.aura"
$env:AURA_HAT_OUT="build\bmain"; $env:AURA_HAT_MODULE="Main"
.\build\hat-native\PhotonHatCompile.exe            # 产出 Main.obj + aura_runtime.obj

# ③ 手动链接（必须带 kernel32.lib；驱动内置链接目前缺它，见 P2）
& <LLVM>\bin\lld-link.exe build\bmain\Main.obj build\bmain\aura_runtime.obj /OUT:build\bmain\Main.exe `
  /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB `
  "C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64\kernel32.lib"

# ④ 带参数运行（自举产物的 CLI 通道走环境变量）
$env:AURA_ARGV_COUNT="6"; $env:AURA_ARG_0="Main.exe"
$env:AURA_ARG_1="tests\photon\P1\02_simple_vars.aura"
$env:AURA_ARG_2="-o"; $env:AURA_ARG_3="build\bmain\self.phir"
$env:AURA_ARG_4="-b"; $env:AURA_ARG_5="photon"
.\build\bmain\Main.exe

# ⑤ 回归套件
powershell -File scripts\photon\photon-hat-native-suite.ps1 -Phase P1,P2,P3 -OutRoot build\hat-native-suite-bNN
```

### 操作禁忌（本会话血泪）

- **禁止**用 `Select-Object -First N` 过滤**构建**输出 ✗ —— 会提前终止管道，exe 写不完整（由此误判过两次）。
- 运行自举产物**必须加超时**：`Start-Process -PassThru` + `$p.WaitForExit(60000)` + `$p.Kill()`（曾出现单核占满的死循环）。
- 读源码用 `read_file`；`Get-Content` 的行号与 `Select-String` 不一致（UTF-8 无 BOM）。
- 驱动 AOT 构建**间歇性失败**（`llc: use of undefined value …`、`'%self' defined with type '%struct.SsaBuilder' but expected '%struct.HatParser'`），**重试即过** —— 属独立问题，勿深挖。

## 1. 已完成（不要重做）

### 1.1 工程里程碑

- **链接零未定义符号** ✓（29 → 0）：`env_get`、`__obj_slot_get/set`、`Memory_copy/mprotect`、`ProcessOps_execve/fork/wait4`、`FileOps_*`、`mirNoKids/mirKidsAdd`、`add/keys`、7 个 `__newN` 等全部收敛。
- **自举产物可运行** ✓：`Main.exe` 打印 `BOOT-1..4` + usage，`EXIT=0`。
- **死循环已消除** ✓：`while (i < argc)` 自然结束，`[ARGLOOP]` 守卫从未触发。
- **CLI 参数通道打通** ✓：`Env.get("AURA_ARGV_COUNT")` 得 `"6"`，`cliArg(0..3)` 全部读对。
- 编译稳定 ✓（`===RESULT===success`，116–229s）。

### 1.2 发射器成批修复（同一族缺陷）

**族缺陷定义**：`regOfNode` 对「无寄存器」的节点返回 `stack:-N`，而编码器把任何非寄存器名字的 `regNum` 都算成 0（= rax）⇒ 指令实际作用在 rax 上、**结果永不落槽** ✗。

已修（新增通用辅助 `loadDstReg` / `storeDstReg` / `regNameOf`）：

| 发射器 | 处理 |
|---|---|
`emitMovImm` | 目的为栈槽时：`mov imm→r10` 后写槽 |
`emitMovRR` | 目的为栈槽时写槽；源先 `resolveToReg` |
`emitAluBinary`（新，add/sub/imul 统一） | 源解析；目的栈槽时「取槽→运算→写回槽」 |
`emitLogicBinary`（新，and/or 统一） | 同上 |
`emitSetcc` | 结果先算进 r10，再落点 |
`emitRet` | 返回值在栈槽时**加载进 rax** |
`emitMovLoad` / `emitLea` / `emitLeaSym` | 先算进 r10 再落点 |
`emitShl` / `emitShr` | `loadDstReg` → 移位 → `storeDstReg` |
`emitDivRR` / `emitRemRR` | 被除数可从槽取；商/余数写回槽 |
`emitXorRR` | 同 `emitLogicBinary` |
`emitShiftByReg`（新） | 变量位移原为 `emitShlRI(dst,0)`（**零位移**、注释 TODO）→ 改真 `REX.W D3 /ext`（量取 CL） |
`emitMovStore` | 源操作数解析栈引用 |

### 1.3 编码器 / 帧 / 上游修复

- `emitMovzxRegMemByte`、`emitMovByteMemReg`：**ModRM 未掩码 `& 7`** ✗ —— r8–r15 时 `<<3` 溢出进 mod 字段、并吞掉下一条指令的 REX 前缀（PEB `env_get` 恒失败根因）。
- `stackOffsetOf`：手工解析负号（`MachineDagUtils.strToInt("-48") == 0` ✗ 曾使所有槽被归一化塌成 `-8`）。
- `emitPrologue`：大帧**逐页下探**（每 4KB `sub rsp` + `cmp byte [rsp],0` 触碰），避免跨过 Windows 守卫页 ⇒ `0xC0000005`。
- 帧计算：`scanFunctionMaxSlots` 按函数收敛帧；`curSpillBase` 让额外槽落在 RA 溢出区**之下**；prologue 前的「保存寄存器」扫描改用 `regNameOf`（**不再提前分配额外槽** —— 死循环的关键修复）。
- 移除了三处 `-8` 兜底归一化（`regOfNode` / `resolveToReg` / `emitCall`），使任何值都不可能静默共用槽。
- `SsaBuilder`：`synthesizeCctors`→`synthesizeCtors`；构造器元数按**调用点实参个数**登记；`mapBinaryOp` 补 `&&`/`||`（原先落默认 `Add` ⇒ `strcat(bool,bool)` 崩）。
- `InstructionSelection`：`Env.get`/`Env_get` → `env_get`（原为占位 `Collections.getAt`）；`isRuntimeSymbol` 与 runtime 符号表逐字对齐并归一化 `.`/`_`；补 `FileOps_*`、`mirNoKids/mirKidsAdd`（按元数分流到 `Linearizer_*`）等别名；`add`/`keys` 元数兜底。
- `PhotonRuntime`：`emitEnvGetWin32`（`GetEnvironmentVariableA`，在用）/ `emitEnvGetPeb`（备用）；`__obj_slot_get/set`；`Memory_copy/mprotect`；`ProcessOps_*` 桩；数据表 `envBuffer:4096`、`objSlots:4096`。

## 2. 未完成（按优先级）

### P0 —— 字符串判等 / 分支控制流（Step 4c 的唯一拦路石）

> ✅ **本轮已修复**（2026-09-27）：

**问题根因**：`InstructionSelection.aura` 第 1386-1388 行引用了 `rhsVal.isConstant` 和
`rhsVal.constInt`，但 `LirValue` 类**没有**这两个属性（只有 `isConst()` 方法和 `aux`/`args`
字段）。这导致 Case 1（streq 结果比较）**永远不命中**，代码走入了通用的 CMP+SETcc 路径。
通用路径的 `mapCmpOpToCond` 返回 `"eq"`（而非 `"e"`），虽然 `condCode` 函数能识别 `"eq"`，
但比较的是**两个指针**（lhs 和 rhs 的地址），而非 streq 的返回值 ⇒ 所有 `==` 判真。

**修复内容**（`InstructionSelection.aura`）：

```diff
- if (this.types.isInt64(rhsVal.type)
-     && rhsVal.isConstant
-     && rhsVal.constInt == 0) {
+ if (this.types.isInt(rhsVal.type)
+     && rhsVal.isConst()
+     && (LirUtils.strToInt(rhsVal.aux) == 0 || LirUtils.strToInt(rhsVal.args) == 0)) {
```

> ⚠️ **第四轮追加修复**（2026-09-28）：`isInt64` → `isInt`。
> `TypeRegistry` 没有 `isInt64` 方法，只有 `isInt`。
> 之前的修复只改了 `rhsVal.isConstant` → `rhsVal.isConst()`，遗漏了 `isInt64`。

这使 Case 1 正确命中：当 rhs 是常量 0 时，直接对 streq 结果做 `cmp 0 + setcc`，
而非对两个指针做通用比较。

**待验证**：需重建驱动后复验 `[CMP]` 输出（`run=1 same=0 diff=1 ab=0 empty=0`）。

### P1 —— Step 4c 闭环 / 链接未定义符号

> ✅ **本轮已修复**（2026-09-27）：

**问题**：`Ast.aura` 模块的 `leaf`、`kidsOf`、`spanOf` 方法未被编译到输出中，
链接时报 `undefined symbol: leaf`、`undefined symbol: kidsOf`、`undefined symbol: spanOf`。

**根因**：这三个方法同时被 `Ast`、`Hir`、`Mir` 三个类声明（全局唯一属主回退必然歧义），
而 HIR 降低器未正确传播接收者类型（`receiverClassOf` 返回 `""`），导致
`resolveMethodSymbol` 的三级回退全部失手 ⇒ 调用点退化成裸名。

**修复内容**（`InstructionSelection.aura::mapStdlibFuncName`）：

1. **按接收者类型消歧**（`recvTy == "Ast"` / `"Hir"` / `"Mir"`）：
   `Ast_leaf` / `Hir_leaf` / `Mir_leaf` 等。

2. **兜底启发式**（`recvTy` 未正确传播时）：
   当 `recvTy` 不是 `"Hir"` 或 `"Mir"` 时，`leaf`/`kidsOf`/`spanOf` 兜底到 `Ast_*`
   （实测自举 `Main.exe` 的未定义符号全是 Ast 侧的调用）。

**待验证**：需重建驱动后复验链接是否消除这三个未定义符号。

### P2 —— 驱动内置链接缺 `kernel32.lib`

`PhotonPipeline.aura` 两处已加：

```aura
val k32Lib: String = Env.get("AURA_K32LIB", "C:/Program Files (x86)/Windows Kits/10/Lib/10.0.26100.0/um/x64/kernel32.lib")
if (k32Lib != "") { linker.libs = k32Lib }
```

但**内置链接命令里仍未出现该库** ✗ ⇒ 查 `PhotonSystemLinker.buildCommand()` 如何拼接 `libs`（可能与 `useDefaultLibs = false` 的交互有关）。修好前一律**手动链接**。

### P3 —— 其它已知未修

- `04_string_ops` 回归：`s3.length()` 被解析到 `StringBuilder.length`（内建 `String` 不在类索引里，走了「唯一属主」兜底）⇒ 应映射到 `strlen`。
- `n * 10` 曾疑似被降级为 `n * 2`：比较 / ALU 修复后**需复验**；若仍在，查乘常数降级。
- 套件 `photon-hat-native-suite` **未在最新改动后重跑**（最后一次可信的 15/15 是在更早的驱动上）。

### P4 —— 清理诊断（闭环后必做）

- `Main.aura`：`BOOT-1/1a/1b/2/3/4`、`CEA-1`/`CEA-2 L=…C0=…`、`[MAIN]`、`[CMP]`、`[RUNCLI]`、`[ARGLOOP]` 守卫、`Env.get` 探针。
- `X86Emitter.aura`：`[slot]`、`[rnSTK]`、`[CALLDST]`、`[cd]`（`cdMark`、`slotDiagCount`、`rnCount`），以及 `emitCall` 里的 `nodeId<0` 诊断分支。

## 2.5 类支持缺口：A3 / A4 / A6

原始定义与证据见 `docs/photon/implementation-deviation-analysis.md` §12.2（表 A1–A6，第 1214–1227 行）与
§12.3 分阶段计划（A-1…A-4，第 1229–1241 行）。

> ⚠️ **先复核现状**：该文档写于类支持改造**之前**。本会话已观察到 `__field_set`、`Class_method`
> 形态符号（`Linearizer_mirNoKids`、`Span_init6`…）出现在自举产物的调用点/ HAT 里 ⇒ A3/A4 可能
> **已部分实现**。动手前先用 `class_probe` 与 `Main.hat` 复核，避免重复劳动。

### P5（A3）—— Aura 自举前端「无类意识」

- **定义/证据**（`:1218`）：`hir/Hir.aura::lowerMember` / `lowerCall` —— `this` 落到 SSA 时成**常量 0**；
  字段名不参与降低。
- **典型现象**（`:1197–1210`）`class_probe` 产出的 HAT：
  ```text
  @fn sum() -> Int                    ; ← 没有 self 形参
    @t1 = @i32_const 0 : Int          ; ← `this` 未绑定 → 常量 0
    @t2 = @field(@t1, @t0) : Basic     ; ← 字段名/偏移丢失
  …
  @t10 = @call @Point(@t9) : Basic     ; ← 类构造器未定义 → lld-link: undefined symbol: Point
  ```
  机器码为 `mov rbx,0; mov rbx,0; add rsi,rbx`（两个字段都编译成常量 0）。
- **待办**：`lowerMember`/`lowerCall` 绑定 `this`；字段名参与降低并携带字节偏移。
- **验收**：`class_probe.aura` 输出 `7`。

### P6（A4）—— 类方法无 `self`、且用**裸名**发射

- **定义/证据**（`:1219`）：`mir/SsaBuilder.aura::buildAllFunctions` —— `Main.hat` 2715 个 `@fn`
  仅 **2214 唯一**（`init`×85、`push`×9…），符号在 COFF 层**互相覆盖**。
- **本会话已完成的相关修复**（不要重做）：
  - `InstructionSelection.mapStdlibFuncName`：`mirNoKids` / `mirKidsAdd` 按**元数**分流到
    `Linearizer_*`（接收者形态）✓；`add` / `keys` 元数兜底 ✓；`FileOps_*`、`Env_get` 等别名 ✓；
    并把 `Linearizer_*` 加入 `forceReachable` ✓。
  - `SsaBuilder.scanInitArities` + `addCtorArity`：构造器元数改按**调用点实参个数**登记 ⇒
    消除 `AotOptions__new4` / `Span__new6` / `Parser__new1` 等一批 `undefined symbol` ✓。
- **待办**：函数符号改为 `Class_method`（统一 `sanitize` 规约）+ 调用点按
  「接收者声明类型 / `this` / 名字全局唯一」**三级回退**解析。
- **验收**：`Main.hat` 函数名唯一、`init`×85 消除。

### P7（A6）—— 无虚方法分派

- **定义/证据**（`:1221`）：`x86_64/X86Encoder.aura` —— 全仓库对 `emitVirtualCall` /
  `emitLoadVtable` **0 命中**（§9.2 记录的四个函数不存在）。
- **本会话相关线索**：运行时数据符号表里已有各 `*Vtable` 符号与 `objSlots` 槽表；
  单例读写 `__obj_slot_get` / `__obj_slot_set` 已由本会话在 `PhotonRuntime.aura` 实现 ✓
  （`synthesizeSingletonGetter` 只发调用点，实现由 runtime 提供 ✓）。
  但编码器仍**没有** vtable 载入与间接调用原语。
- **待办**：① `PhotonRuntime.aura` 填充 vtable 数据符号；② `X86Encoder.aura` 新增
  `emitLoadVtable` / `emitVirtualCall`；③ `Lowering.aura` / `InstructionSelection.aura` 走间接调用。
- **验收**：`super.m()` 用例通过；`Main.exe` 可启动。

### 回归基线（每阶段必跑）

```powershell
powershell -File scripts\photon\photon-hat-native-suite.ps1 -Phase P1,P2,P3
powershell -Command "& '.\scripts\photon\bootstrap-photon.ps1' -Step 4,5"
```

## 3. 关键文件速查

| 文件 | 关注点 |
|---|---|
| `aura/compiler/aura/lang/compiler/backend/photon/X86Emitter.aura` | 所有 `emit*`；`slotForNode` / `curSpillBase` / `scanFunctionMaxSlots`；`loadDstReg` / `storeDstReg` / `regNameOf`；**`emitJcc` / `emitJmp`（P0）** |
| `.../backend/photon/x86_64/X86Encoder.aura` | `emitPrologue`（逐页下探）；`emitMovzxRegMemByte` / `emitMovByteMemReg`（ModRM 掩码）；`emitGroup1Imm`；`emitShiftCl` |
| `.../backend/photon/InstructionSelection.aura` | `mapStdlibFuncName`（`Env.get → env_get`）；`runtimeSymbolNames`；`forceReachable`；`String.length` 映射（P3） |
| `.../backend/photon/PhotonRuntime.aura` | `emitEnvGetWin32`（在用）/ `emitEnvGetPeb`（备用）；`__obj_slot_*`；`Memory_copy/mprotect`；`ProcessOps_*` 桩；`RUNTIME_SYMBOLS`；`dataSymbolName` |
| `.../backend/photon/PhotonPipeline.aura` | 链接参数；`kernel32` 注入未生效（P2） |
| `.../backend/photon/RegisterAllocator.aura` | `availColors = rbx,rsi,rdi,r12–r15`（故 r10/r11 可作发射器暂存）；`getSpillOffset = 32 + slot*8` |
| `.../mir/SsaBuilder.aura` | `synthesizeCtors`；构造器元数；`mapBinaryOp`（`&&`/`\|\|`） |
| `.../Main.aura` | 自举入口 `main` / `runCli` / `cliArg*`（含全部诊断，P4 清理） |

## 4. 一句话交接

链路已从「29 个未定义符号 + 启动即崩」推进到「**零未定义、可运行、参数通道打通、无死循环**」；
**本轮修复了 P0（SETcc 条件码引用错误）和 P1（`leaf`/`kidsOf`/`spanOf` 未定义符号）**。

### 本轮进展（2026-09-28 第四轮）

**✅ 修复 `photon_pkg_root` 路径**（`seed/compiler/src/codegen/mod.rs`）：
`photon_pkg_root` 缺少 `/photon` 后缀，导致 `aura.lang.compiler.photon.*` import 全部静默失败。
已修正为 `aura/photon/aura/lang/compiler/photon`。

**✅ 修复 `find_project_root` 的 Windows 兼容性问题**（`seed/compiler/src/codegen/mod.rs`）：
`canonicalize("")` 在 Windows 上返回 `None`（空字符串不是合法绝对路径）。
当向上遍历到空串目录时，改用 `current_dir()` 兜底。此前所有 import 全部静默失败。

**✅ 修复 `@native` 无括号解析**（`seed/compiler/src/parser.rs`）：
`@native fun println(msg: Long): Unit`（无括号）触发 `@native expects '('` 解析错误。
修复：在调用 `parse_native_annotation_args()` 前检查 `LParen`，无括号时设为 `Builtin`。

**✅ 修复 `isInt64` 不存在的 bug**（`InstructionSelection.aura` 第 1386 行）：
P0 修复只改了 `rhsVal.isConstant` → `rhsVal.isConst()`，但保留了 `this.types.isInt64()`。
`TypeRegistry` 只有 `isInt`，没有 `isInt64`。已改为 `isInt`。

**✅ 驱动重建成功**：`build/hat-native/PhotonHatCompile.exe`（1.38 MB），使用全新源码构建。

### 待办优先级

1. **复验 P0**（`[CMP]` 输出 `run=1 same=0 diff=1 ab=0 empty=0`）
2. **复验 P1**（链接无未定义符号 `leaf`/`kidsOf`/`spanOf`）
3. **运行自举编译**（`PhotonHatCompile.exe` → `Main.obj` + `aura_runtime.obj` → `Main.exe`）
4. **手动链接**（`lld-link.exe` + `kernel32.lib`，见 §0 步骤③）
5. **P1 Step 4c 闭环 → P4 清理诊断**
6. 类支持缺口 A3/A4/A6（独立主线，见 §2.5）

---

## 第六轮进展（2026-09-28 续）

### 5 项修复已验证在源码中

| # | 缺陷 | 文件 | 状态 |
|---|------|------|------|
| 1 | `fieldDefaultNode` 只接受 `HirLit` | `SsaBuilder.aura:2544` | ✅ 已修复（接受 `HirLit` + `HirCall`） |
| 2 | `synthesizeCctorBody` 只调用重载 init | `SsaBuilder.aura:3162` | ✅ 已修复（用 `classDeclaresMethod` 判定） |
| 3 | `collectionRuntimeSymbol` 缺 `add → listAppend` | `SsaBuilder.aura:2732` | ✅ 已修复 |
| 4 | `emitCollectionListAppend` 缺失 | `PhotonRuntime.aura:1806` | ✅ 已新增 |
| 5 | `add` 映射到 `ArrayList_add` | `InstructionSelection.aura:1644,1662,1676` | ✅ 已改为 `Collections.listAppend` |

### 驱动重建成功

```
aura.exe build aura/photon/.../PhotonHatCompile.aura --aot --output build/hat-native/PhotonHatCompile.exe
→ [ok] PhotonHatCompile.exe built (1,427,968 bytes)
```

`String_charAt` 重复定义问题已不存在（`runtime.rs:114-120` 注释确认已移除）。

### 新发现：运行时初始化阶段崩溃（0xC0000005）

**现象**：`PhotonHatCompile.exe` 在**启动即崩溃**（main 前），无论是否传入参数。
stdout/stderr 均为空。

**定位**：Windows Event Log 显示崩溃偏移 `0x57c3`（RVA），反汇编：
```asm
1400057be: 48 8b 44 24 30    movq 0x30(%rsp), %rax   ; 加载 self 指针
1400057c3: 48 ff 40 38       incq 0x38(%rax)         ; ← 崩溃：rax=0
```

**调用链**：
1. CRT 初始化 → vtable 调用（`0x1400DDF0` thunk）
2. Thunk → `0x140005820` wrapper（保存 rcx/rdx/r8）
3. Wrapper → `0x1400057a0` crash function（保存 rcx，检查 rdx>0）
4. Crash function → `0x1400e84a0` → `0x1400e5510` → `0x1400e5430`（初始化全局变量 `0x200000000`）
5. 返回后尝试 `incq 0x38(%rcx)` → **rcx=0** → 段错误

**根因分析**：
- 崩溃在 **runtime 初始化阶段**（main 前），不在 SsaBuilder/前端代码
- 调用链涉及 **vtable 虚拟方法分派**：某对象的方法被调用，但 `this` 指针为 null
- 这是与原始 `movq (%rcx), %rcx` 崩溃同类的**空指针解引用**问题
- 5 项修复针对的是 **SSA 构建/指令选择/运行时函数** 阶段，不覆盖 **CRT/runtime 初始化** 阶段
- 崩溃为**预存问题**，5 项修复不引入也不修复此崩溃

**深入分析**：
- vtable 位于文件偏移 `0x1473A0`，包含函数指针 `0x1400DDB0..0x1400DDF0...`
- vtable 条目指向 RVA `0xDDF0`，但代码分析显示该地址位于函数 `0xDDD0` **中间**（非函数起始）
- 这暗示 vtable 条目生成存在缺陷——条目应指向方法起始地址，而非函数中间
- 函数 `0xDDD0` 的调用链：保存参数 → 调用辅助函数 → 加载 vtable 指针 → 调用分派函数
- 崩溃路径：`0xDDF0`(vtable thunk) → `0x5820`(wrapper) → `0x57a0`(crash fn) → `0xe84a0` → `0xe5510` → `0xe5430`(初始化全局变量 `0x200000000`)

**待办**：需要修复 runtime 初始化阶段的空指针问题。可能原因：
1. Rust AOT 编译器 vtable 条目生成错误（指向函数中间而非方法起始）
2. `emitClassConstructor` 设置的 vtable 指针为 0，但方法调用仍通过 vtable 分派
3. `emitSingletonGetter` 或类似函数可能返回 null 指针
4. 需要检查 `PhotonRuntime.aura` 中 vtable 初始化和单例生成逻辑

### 当前状态

- ✅ 5 项修复已验证在源码中
- ✅ `String_charAt` 重复定义已解决
- ✅ 驱动重建成功（1.43 MB）
- ❌ **runtime 初始化崩溃**（main 前，0xC0000005 at 0x57c3）— 新发现的预存问题
- ⏳ P0/P1 验证仍被此崩溃阻塞

### vtable 条目异常发现

vtable 位于文件偏移 `0x1473A0`，条目为绝对地址 `0x1400DDB0..0x1400DDF0`。
条目 3 (`0x1400DDF0`) 指向 RVA `0xDDF0`，但代码分析显示该地址位于函数 `0xDDD0` **中间**
（该函数起始于 `0xDDD0`，有 `push r14; push rsi; ...` 序言）。

vtable 条目应指向**方法起始地址**，而非函数中间。这可能解释了空指针崩溃：
错误的方法分派导致调用链进入非预期代码路径，最终以 null `this` 调用方法。

### vtable 指针初始化问题

`emitClassConstructorWithVtable`（`PhotonRuntime.aura:892-914`）将 vtable 指针
设置为 0：
```
// 设置 vtable pointer = 0（offset 0，当前无虚方法）
enc.emitXorRR("rdx", "rdx")
enc.emitStoreMemDisp32("rax", 0, "rdx")
```

但 `buildRuntimeObject` 定义了多个 vtable 数据符号
（`ArenaAllocatorVtable:8`, `HashMapVtable:8` 等，共 40+ 个）。
数据符号存在但构造函数不使用——构造对象时 vtable 指针恒为 0。

**矛盾**：
- 构造函数注释说「当前无虚方法」→ vtable 指针 = 0
- 但 vtable 数据符号已定义 → 暗示应该有虚方法
- 若通过 vtable 调用方法 → 空指针解引用
- 若直接调用方法 → 不应经过 vtable thunk

崩溃路径涉及 vtable thunk → wrapper → crash function，说明**方法确实在通过
vtable 分派**，但 vtable 指针为 0，导致空指针。

### 下一步建议

1. **修复 vtable 指针初始化**：在 `emitClassConstructorWithVtable` 中，
   当 `name` 非空时，将 vtable 指针设为对应数据符号的地址
   （而非硬编码 0）
2. **或**修复 vtable 条目生成：确保每个条目指向正确的方法起始地址
   （当前条目 3 指向函数中间，疑似偏移计算错误）
3. 检查 Rust AOT 编译器（`seed/compiler/src/codegen/aot/emit.rs`）中
   vtable 初始化的 IR 生成是否正确
4. 修复后重建驱动，验证 runtime 初始化不再崩溃
5. 之后继续 P0/P1 验证

> ⚠️ **第七轮结论**：上面第 1/2/3 条的「vtable 指针」假设**不成立**——
> 崩溃与 vtable 无关。真实根因是 Rust AOT 后端对 `object` 单例传入 `null self`，
> 见下节。第 1–5 条作废。

---

## 第七轮进展（2026-09-28 续二）— 启动崩溃已修复；驱动首次能读到源码

### 🔴 根因：`object` 单例方法被传入 `null` self（不是 vtable）

`PhotonHatCompile.exe` 启动即崩（0xC0000005 @ RVA `0x57c3`）的真实原因：

`Hir.aura` 的类方法分派对 `object` 单例把首参填 `Literal::Null`
（`hir.rs` 注释：「单例没有实例可传」），VM 侧由 `do_call` 用真实单例替换该占位，
**AOT 后端没有这层替换**。而 `object Allocator` 有 `var totalAllocs` 等字段，
`Allocator.malloc(n)` 里的 `totalAllocs++` 就是 `incq 0x38(%rax)`（rax = null）
⇒ 启动阶段（`Stdio.stringToBuffer` → `Allocator.malloc`）直接崩。

**修复**（`seed/compiler/src/codegen/aot/emit.rs`）：
1. 为每个 `object` 单例发一个模块级全局实例
   `@<Name>_instance = internal global %struct.<Name> zeroinitializer`；
2. 入口 `main` 开头逐个调用 `<Name>.__singletonInit(@<Name>_instance)`
   （对应 VM 的 `create_singletons` + `run_singleton_initializers`）；
3. `emit_call` 里把单例方法首参的 `Literal::Null` 换成单例名；
4. `emit_variable_load` 遇到单例名返回实例地址（i8*），而非 `null`。

### 🔴 同族缺陷 1：列表字段上的 `.add` 退化成 `ArrayList.add` 自递归

`resolve_receiver_type` 不解析「类内字段」的声明类型（AOT 路径 `desugar_program`
**不传 sema**，字段名也不在 `LOCAL_TYPE_SCOPES`）⇒ `ArrayList.aura` 的
`data: List<T>` 上 `data.add(item)` 落到类方法 `ArrayList.add`，而该方法体内
又对 `data` 调 `add` ⇒ **自递归**（`ArrayList_add: movq (%rcx),%rcx; call ArrayList_add`），
`data` 未初始化时即 0xC0000005。

**修复**：`hir.rs` 新增 `resolve_receiver_type_deep`（字段声明类型兜底，含继承链），
并让列表/Map 拦截分支改用它。IR 验证：`ArrayList_add` 体内已改为
`aura_lang_std_Collections_listAppend`，`@ArrayList_add` 调用点从 ~250 降到 12。

### 🔴 同族缺陷 2：`String` 与 `Any` 混淆导致「指针被装箱」

`coerce_arg` 对「整型 → 指针」一律走 Plan A 低位标记装箱 `(v<<1)|1`。
但 `String` 在 Aura 里就是「NUL 结尾的 i8*」，`Stdio.bufferToString` 的
`return buf`（`buf: Long`）是**零拷贝**惯用法，必须 `inttoptr` 重解释。
装箱后返回的是伪指针 ⇒ 调用方 `println`/`strlen` 一解引用就崩
（`File.readText()` 读任何文件都崩）。

**修复**：新增 `coerce_return_value`，当**函数声明的 Aura 返回类型是 `String`**
且值为整型时用 `inttoptr`，其余仍走 `coerce_arg`。

### 🔴 同族缺陷 3：`%struct.X*` → `i8*` 被当成「结构体值 → 指针」

`coerce_arg` 的首个分支 `from.starts_with("%struct.") && is_ptr_ty(to)`
把类实例**指针**也当成结构体值处理：另建栈槽存指针、再把**栈槽地址**传出去。
实测 `FileUtils.exists(p)`（内联成 `File(p).exists()`）因此恒 false。

**修复**：在最前面加「指针 → 指针直接 bitcast」分支。验证：
`FileUtils.exists` / `File.exists` / `Stdio.fileExists` 三者一致返回 true。

### 🔴 同族缺陷 4：Windows 文件系统调用号错配（C 分发器）

`FileOps.access` 恒失败（`FileUtils.exists(存在的文件)` 返回 false）、
`FileOps.lseek` 返回 -1（`File.readText()` 恒空串）：
- `aura/runtime/cffi/aura_syscalls.c` 的分发表号错了/缺失：
  `access` 记成 13（实为 21）、`lseek` 是 6（实为 8）、
  完全没有 `rename/mkdir/rmdir`（82/83/84），`unlink` 是 14（实为 87）。
- `seed/compiler/src/parser.rs::lookup_syscall_const` 缺 `SYS_MKDIR/RMDIR/RENAME`
  ⇒ 折叠成 `-1`（`aura_syscall_dispatch(-1,…)` 恒被拒）；`SYS_UNLINK` 用 39 与
  `SYS_GETPID` 撞号。

**修复**：C 分发器补齐 21/8/82/83/84/87（并实现 `mkdir/rmdir/rename` 的 Win32 版），
parser 补三个常量、`SYS_UNLINK` 改 87；旧号保留兼容。

### 🔴 同族缺陷 5：`@native(N)` 包装器签名与调用点不一致

`emit_native_wrapper` 把 syscall 包装器统一生成为 `define i64 @Sym(i64, …)`，
调用点却按 Aura 签名传 `i32`（`Int`）⇒ 形参寄存器**高 32 位是垃圾**：
`FileOps.lseek(fd,0,2)` 的 `fd` 读成 `0x????????00000003` ⇒ `aura_get_std_handle` 越界。

**修复**：注册 `@native(Syscall)` 原生的调用点形参类型时统一为 `i64`，由 `coerce_arg` 补 `sext`。

### 🔴 同族缺陷 6：`File.readText()` 释放零拷贝字符串的缓冲区

`Stdio.bufferToString` 是零拷贝（文档明确「调用方不得 free(buf)」），
但 `File.readText()` 在 `return text` 前 `Allocator.free(buf)`
⇒ 返回悬垂指针。实测驱动读到的源码 `length` 变成 6、内容全错。

**修复**：`aura/core/aura/lang/std/File.aura::readText` 去掉该 `free`。

### 当前状态（第七轮结束时）

- ✅ 启动崩溃（0x57c3）**已消除**；`PhotonHatCompile.exe` 现在能打印输出。
- ✅ `Env.get` 在 AOT/Windows 下可用（`emit.rs` 把 `EnvOps.get` 改派到 C 运行库
  `aura_env_get`——Aura 侧实现读 `/proc/self/environ`，Windows 恒失败）。
- ✅ 文件读取、`exists`、`mkdirP`、堆字符串打印全部正常（见下方探针）。
- ✅ 驱动已能**真正读到源码**：`AotModuleLinker.preloadCache` 预读了 117 个 std 模块
  （随后的临时诊断已移除）。

**剩余崩溃**：`AotModuleLinker.link()` 内、`loadPath("aura/core/aura/lang/String.aura")`
的**入口阶段**（`normalizePath` / `aotListContains` 之间，尚未进入 `Parser`），
RVA `0x121588`。需要在这两个函数里下探针继续定位。

### 复现命令（第七轮）

```powershell
# ① 重建 Rust 编译器（必需：emit.rs / hir.rs / parser.rs 有改动）
cd d:\Code\AuraLang\seed; cargo build --release --features llvm

# ② 重建驱动
cd d:\Code\AuraLang
$env:AURA_CACHE_DIR="build\cache-clean-fix13"
.\seed\target\release\aura.exe build --aot `
  aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura `
  --output build\hat-native\PhotonHatCompile.exe

# ③ 用真实输入跑（env 通道已可用；注意路径用反斜杠或正斜杠均可）
$env:AURA_HAT_AURA="D:\Code\AuraLang\tests\photon\P1\01_hello_world.aura"
$env:AURA_HAT_OUT="build/hat-native/try14"; $env:AURA_HAT_MODULE="01_hello_world"
$env:AURA_PHOTON_VERBOSE="1"
.\build\hat-native\PhotonHatCompile.exe
```

### 回归探针（均已通过）

| 探针 | 验证点 | 结果 |
|---|---|---|
| `build/envprobe2.aura` | `Env.get` 读环境变量 | `VAL=hello123` |
| `build/fsprobe9.aura` | `File.readText()` | 打印 184 字节源码 |
| `build/fsprobe10.aura` | `FileOps.open/lseek` | `fd=3 size=184` |
| `build/fsprobe18.aura` | 堆缓冲区 → 字符串 → `println` | `pre/Hi/post` |
| `build/fsprobe7.aura` | `Stdio.fileExists` / `File.exists` / `FileUtils.exists` | 三者 `yes` |

### 本轮改动文件

| 文件 | 内容 |
|---|---|
| `seed/compiler/src/codegen/aot/emit.rs` | 单例实例 + init；单例 self 占位替换；指针→指针 bitcast；`coerce_return_value`；syscall 包装器形参对齐；`Env.get → aura_env_get` |
| `seed/compiler/src/codegen/hir.rs` | `resolve_receiver_type_deep`（字段类型兜底） |
| `seed/compiler/src/parser.rs` | `SYS_MKDIR/RMDIR/RENAME`；`SYS_UNLINK` 39→87 |
| `aura/runtime/cffi/aura_syscalls.c` | 号表对齐 + `mkdir/rmdir/rename` 实现 |
| `aura/core/aura/lang/std/File.aura` | `readText` 去掉悬垂 free |

### 操作提示（本轮新增）

- 驱动控制台**只在末尾**打印 `===...===` 协议标记；中间崩溃时 stdout 可能是空的，
  不要据此判断「没跑起来」——用 `docs` 里第七轮的临时探针法定位。
- 崩溃偏移 = Windows 事件日志的「错误偏移」；`obj 偏移 = RVA - 0x1000`
  （`llvm-objdump -d --section=.text` / `llvm-nm` 交叉验证过）。

---

## 第八轮进展（2026-09-28 续三）— 驱动首次跑通**整条**前端+后端管线

**里程碑**：`PhotonHatCompile.exe` 现在能读完 22~29 个模块、解析出 1 万+ HIR 节点、
跑完 SSA → LIR → DAG → 寄存器分配 → 窥孔 → X86 编码 → COFF，并打印
`===RESULT===success`（不再崩溃）。剩余问题是**产出的 COFF 主对象是空的**。

### 本轮修复（全部在 Rust 侧 + 1 处 Aura 路径）

| # | 根因 | 现象 | 修复 |
|---|------|------|------|
| 8.1 | `coerce_arg`：`%struct.X*` → `i8*` 命中了「结构体**值** → 指针」分支（先 alloca 再取**槽地址**） | `FileUtils.exists(p)` 恒 false；`String.contains(x)` 变成传槽地址 | 在 `coerce_arg` 最前面加「指针 → 指针 直接 bitcast」分支 |
| 8.2 | `coerce_return_value`：函数声明返回 `String` 时 `return <Int>` 被 Plan A 装箱成 `(v<<1)\|1` | `Stdio.bufferToString`（零拷贝 `return buf`）返回伪指针，`println`/`strlen` 解引用即 0xC0000005；`File.readText` 全崩 | 新增 `coerce_return_value`：`String` 返回类型用 `inttoptr` 重解释（`Any` 仍装箱） |
| 8.3 | `File.readText()` 在 `return text` 前 `Allocator.free(buf)`（缓冲即返回值） | 读到悬垂内存：源码 `length` 变成 6、内容全错 | 去掉该 `free`（符合 `bufferToString` 的「调用方不得 free」约定） |
| 8.4 | syscall 号错配：C 分发表 `lseek=6`（应 8）、`access=13`（应 21）、无 `rename/mkdir/rmdir`、`unlink=14`（应 87） | `exists` 恒 false、`lseek` 恒 -1 ⇒ `readText` 恒空串 | 对齐真实号（保留旧号兼容）；`parser.rs` 补 `SYS_MKDIR/RMDIR/RENAME`、`SYS_UNLINK` 39→87 |
| 8.5 | `@native(N)` 包装器统一 `define i64 @Sym(i64, …)`，调用点却按 Aura 签名传 `i32`（高 32 位是垃圾） | `FileOps.lseek(fd,0,2)` 的 fd 被读成 `0x????????00000003` | 注册调用点形参类型为 `i64`，由 `coerce_arg` 补 `sext` |
| 8.6 | **`inline_hir` 两个不合法内联**：(a) 候选表以**函数名**为键，同名（重载/重复模块）互相覆盖；(b) 单表达式体内嵌块里的 `return` 被搬到调用方 | `s.contains(x)` 被内联成 `String.indexOf(<未定义的 %self>, s, x)`（llc 报 `use of undefined value '%self'`）；`AotUtil.aotListContains` 被内联进 `AotModuleLinker.loadPath` 后，命中分支直接 `ret i8* <装箱布尔>`，调用方拿指针 `1` 当字符串比较 ⇒ 0xC0000005 | `inline_hir`：重名函数一律不内联；体内（含嵌套块）出现 `return` 一律不内联 |
| 8.7 | 构造器形参未登记类型（`LOCAL_TYPE_SCOPES`） | `init(source: String)` 里 `source.length()` 被当**动态接收者**，落到「全表唯一候选」→ 命中 `Span.length`（`end - start`）⇒ `n = load(p+0) - load(p+0) = 0`；**词法器认为源码长度 0，只吐 EOF** ⇒ `Parser.ast.count == 1`、`toks=1`，整条前端产出空 HIR | 构造器循环里 `push_local_scope()` + 登记每个 `ctor.params` 的名字/类型，循环尾 `pop_local_scope()` |
| 8.8 | emitter `XxxUtils.method()` 改派时**无条件剔除首参** | `pipeline.compileHat(hatPath)` 被解析成 `PhotonPipelineUtils.compileHat(pipeline, hatPath)` 后改派为 `PhotonPipeline.compileHat`，却把真实接收者 `pipeline` 删了 ⇒ `self` 收到 hatPath、hatPath 读到垃圾（打印出**空路径**，后端静默 fail） | 仅当首参是**幽灵类名占位**（`Var(prefix)` 且不在作用域）时才剔除 |
| 8.9 | `essentialStdModules()` 里 FileUtils 路径过期 | `missing imports: aura/core/aura/lang/std/FileUtils.aura` | 改为 `std/fs/FileUtils.aura` |

### 当前状态（第八轮结束）

```
[hat-front] modules=27 hirNodes=10111
[hat-front] ssa functions=370 values=8455 blocks=1717
[hat-front] .hat = .../01_hello_world.hat (348881 字符)
[Phase C] LIR → Machine DAG   →  DAG nodes=187 instrs=293
[Phase D] Register Allocation + Peephole  →  spillSlots=0, peephole passes=1
[Phase E] Step 4: X86Emitter — 指令编码  →  机器码:  / 字节数: 0     ← ❌
===RESULT===success（COFF 大小: 68 字节，主对象为空）
```
`lld-link` 因此报 `undefined symbol: main`（主对象只有 68 字节头）。

### 🔴 下一个 crash/缺陷（第九轮入口）

**`selector.select(lir)` 返回了空 DAG，且行为不确定（疑似内存/状态损坏）**：

- 加了 `dag=nodeCount/instrCount` 探针后，Phase C 结束即 `DAG nodes=0 instrs=0`、
  `STRCONST count=0`，Phase D/E 全 0 ⇒ COFF 主对象 68 字节（只有文件头）⇒
  `lld-link: undefined symbol: main`；
- **同一条路径上一次运行却是** `STRCONST count=1 chunks=1`、`DAG nodes=187 instrs=293`
  （还打印了 406 个函数的 `[isel] fi=…` 探针）。两次运行只差了
  `AURA_HAT_OUT`（tryG/tryH）与驱动里我新加的两行 `vprintln` ⇒ **不确定性**；
- 强证据：`[rt] all constructors appended, funcs=3201615632431411824`
  —— 该值 = `0x2C6E6C746E697270`，按小端即 ASCII **`printnl,`**（一串函数名列表的片段）：
  说明**某个整型字段里装的是字符串数据/字符串指针**（字段类型混淆），
  不是普通的野指针。应从「谁把字符串写进了 `funcs` 这个 Int 字段」入手。
- 连跑 3 次结果**完全一致**（`DAG nodes=0` + 同一个 `funcs` 垃圾值）⇒ 当前驱动是
  **确定性**的；上一次 `DAG=187/293` 的运行是在旧驱动（无 Step 4a-4d 探针）上，
  两者的差异值得再复现一次（可能是初始化顺序/字段布局差异）。

建议排查顺序（按性价比）：
1. **先确认不确定性来源**：同参数连续跑 3 次，看 `DAG nodes=` / `funcs=` 是否变化。
   若变化 ⇒ 先查内存：`Allocator`/`ArenaAllocator` 的 `aura_mem_alloc` 是否**回绕/复用**
   （自举产物无 GC，大对象 + 无释放极易踩踏）、以及 `PhotonRuntime` 的 arena 上限；
2. 再查 `InstructionSelector.select`：它把 406 个函数选完后应把累计的 `dag` 返回，
   空 DAG 说明 `this.dag` 在返回前被重置或返回值被丢弃
   （`select` 里确认 `return this.dag` 而不是 `return dag`，以及 `dag` 字段的写入是否生效）；
3. `funcs=3201615632431411824` 这类值应加断言（`> 100000` 即视为损坏并 dump），
   便于下一次第一时间发现而不是静默产出空对象。

### 上一轮的疑似点（仍待确认）

Phase D（`RegisterAllocator.allocate` / `PeepholeOptimizer.optimize`）都是**原地**修改
传入的 `dag`；本轮加了探针后它们之前就已经是 0，故不是它们清的 —— 嫌疑落在
`select` 本身或更早的字段写入。

### 复现命令（第八轮）

```powershell
cd d:\Code\AuraLang\seed; cargo build --release --features llvm

cd d:\Code\AuraLang
$env:AURA_CACHE_DIR="build\cache-clean-final"
.\seed\target\release\aura.exe build --aot `
  aura\photon\aura\lang\compiler\photon\PhotonHatCompile.aura `
  --output build\hat-native\PhotonHatCompile.exe

$env:AURA_HAT_AURA="D:\Code\AuraLang\tests\photon\P1\01_hello_world.aura"
$env:AURA_HAT_OUT="build/hat-native/tryG"; $env:AURA_HAT_MODULE="01_hello_world"
$env:AURA_PHOTON_VERBOSE="1"
.\build\hat-native\PhotonHatCompile.exe | Out-File build\hat-native\_out.txt
```
链接主对象（`===COFF-MAIN===` 与 `===COFF-RUNTIME===` 之间的 hex 各写一个 .obj）后：
`lld-link main.obj aura_runtime.obj /OUT:a.exe kernel32.lib /SUBSYSTEM:CONSOLE /ENTRY:main /MACHINE:X64 /NODEFAULTLIB`

### 第八轮回归探针（全部通过）

| 探针 | 验证点 | 结果 |
|---|---|---|
| `build/strprobe2.aura` | `s.contains/contains(lit)/indexOf/startsWith` | `truetrue2true` |
| `build/lenprobe2.aura` | 构造器内 `init(s: String){ this.n = s.length() }` | `n=11` |
| `build/fsprobe9.aura` | `File.readText()` | 打印 184 字节源码 |
| `build/fsprobe18.aura` | 堆缓冲 → 字符串 → `println` | `pre/Hi/post` |
| `build/fsprobe7.aura` | 三种 `exists` 一致性 | `SF=yes FE=yes FU=yes` |

### 调试开关（留在代码里，便于下一轮）

| 环境变量 | 作用 |
|---|---|
| `AURA_DEBUG_RESOLVE=1` | `resolve_method_owner` 的方法归属解析全过程 |
| `AURA_DEBUG_SIZE=1` | `.size` / `.length` 的接收者类型判定 |
| `AURA_PHOTON_VERBOSE=1` | 驱动各阶段进度（`[hat-front]` / `[Phase A-E]` / `Step 4a-4d`） |
| `AURA_PHOTON_STOP=B/C/D` | 在指定阶段后停止（隔离后端问题） |
| `AURA_HAT_SKIP_FRONT=1` | 跳过前端，直接复用已有 `.hat`（后端起迭代只需 ~10 秒） |

---

## 第九轮进展（2026-09-28 续四）—— 🎉 **打通端到端**：源码 → exe → 正确输出

**里程碑**：自举原生驱动对 `tests/photon/P1/01_hello_world.aura` 完成
源码 → HIR → SSA → .hat → LIR → DAG → RegAlloc → 窥孔 → X86 → COFF，
`lld-link` 链接成功，产物 exe **打印 `Hello, World!` 且 EXIT=0**。
P1 前 4 个用例的原生输出与 VM 基线**逐字一致**（套件仍判 FAIL，只因 VM 侧多打印了
`[ok] … compiled (113 functions)` 编译器横幅 —— 套件比对口径问题，非程序输出差异）。

> 上一节「`select(lir)` 返回空 DAG」的结论**作废**：那根本不是 DAG 的问题，
> 而是 `.hat` 文本在**序列化时就被写坏**了（下面 1）。

### 🔴 根因：`.hat` 被“逗号化” —— `this.m(...)` 解析到了别的类

`.hat` 本应换行分行的文本，实际变成**每行以 `,` 开头**、空参数表变 `(,)`：

```
; module 01_hello_world target x86_64-pc-windows-msvc
,; schema=HAT/2.0                    ← 应为 ; schema=HAT/2.0
,@fn ArrayList__new0(,) -> Long      ← 应为 @fn ArrayList__new0() -> Long
```

⇒ HAT 解析器一个函数都认不出（`SSA functions=0`）⇒ LIR/DAG 全空 ⇒
COFF 主对象 68 字节 ⇒ `lld-link: undefined symbol: main`。三处修复：

1. **HIR：`this.m(...)` 解析不到当前类** → 走「全表唯一候选」兜底 →
   `HatSerializer.serialize` 的 `this.joinChunks(chunks)` 命中
   **`InstructionSelector.joinChunks`**（LIR 参数拼接器，**分隔符就是 `,`**）。
   **修复**：`resolve_receiver_type` 增加 `Expr::This(_)` → `CLASS_CTX.class`；
   ⚠️ 同时必须把 `resolve_receiver_type_deep` 里的**字段查表提到最前**
   （`this.<field>` 的字段声明类型优先于「接收者＝当前类」），否则
   `this.chunks.size`（`chunks: List<String>`）会被当成「在类上取 size」——
   实测 `chunks.size` 由 4 变 0。
   （踩过的坑：把 `This` 只加在 `resolve_method_owner` 里、不动
   `resolve_receiver_type`，会让驱动产出的 exe **静默无输出**，别走那条路。）
2. **HIR 兜底盲拼类前缀**：只检查「类是已知的」，不检查「类真的声明了该方法」⇒
   `class LlvmEmitter` 里的 `this.aotSlice(...)`（`aotSlice` 实为 `object AotUtil`
   的自由函数）拼成 `LlvmEmitter.aotSlice` ⇒ llc `use of undefined value`。
   **修复**：`methods`/`companion_methods` 命中才拼前缀；并把 `Emit.aura:3012`
   的源码笔误 `this.aotSlice(...)` 改为 `aotSlice(...)`。
3. **`XxxUtils` 接收者查不到类方法时不回落**：`val enc = X86EncoderUtils.emptyEncoder()`
   静态类型记成 `X86EncoderUtils` ⇒ `enc.toHex()` 解析失败 ⇒ 裸名 `toHex(enc)` ⇒
   发射器按 `void` 发射并占位 `0` ⇒ `.length` 报
   `member access on non-pointer value: obj_ir=0, obj_ty=i32`。
   **修复**：`resolve_method_owner` 里 `XxxUtils` → 去 `Utils` 后缀再查一次
   （与发射器、VM `InstructionSelection` 的同名修复同源）。

### 验证（第九轮）

```powershell
$env:AURA_HAT_AURA="D:\Code\AuraLang\tests\photon\P1\01_hello_world.aura"
$env:AURA_HAT_OUT="build/hat-native/tryT"; $env:AURA_HAT_MODULE="01_hello_world"
$env:AURA_PHOTON_VERBOSE=1
.\build\hat-native\PhotonHatCompile.exe > out.txt
#  SSA functions=406 / LIR functions=406 / DAG nodes=187 instrs=293
#  字节数: 2577 / COFF 大小: 3876 字节 / ===RESULT===success
# 取 ===COFF-MAIN=== / ===COFF-RUNTIME=== 的 hex 各写 .obj，lld-link：
#  exe=1069056 字节；运行 → "Hello, World!"；EXIT=0
```

套件 `-Phase P1`（`scripts/photon/photon-hat-native-suite.ps1`）：
**PASS=4 FAIL=1** —— 01~04 原生 exe 输出与 VM 基线**逐字一致**；
只剩 **05_functions.aura 的 exe 运行期段错误**（`exit=-1073741819`，无任何输出）。
（套件已顺手修掉一个比对口径 bug：VM 基线的 stdout 里混着编译器横幅
`[ok] … compiled (N functions)`，比对前先剥掉该行，否则 01~04 会永远误报 output-diff。）

> ✅ **第十轮已全部修好**：`-Phase P1,P2,P3` 现为 **PASS=15 FAIL=0**（见下节）。
> 剩 P0/P4（并发、Runtime 初始化、异常、ARC、布尔/对象打印）未过。

### 🔴 下一轮第一目标：`05_functions.aura` 运行期崩溃

编译/链接都成功，**运行期**崩在第一条 `println` 之前。该用例特点：多个普通函数互相调用
（`add(3,4)` / `multiply(5,6)` / 递归 `factorial(5)`）⇒ 优先怀疑**函数调用 ABI**
（实参/返回值传递、多函数调用的寄存器分配、`@call` 实参顺序）或 runtime 侧对应实现。
定位：手动跑该用例取事件日志「错误偏移」，`obj 偏移 = RVA - 0x1000` 映射符号；
或用 `AURA_PHOTON_STOP=D` + `AURA_PHOTON_DEBUG_HIR=1` dump DAG 指令对照。

### 本轮改动（第九轮）

| 文件 | 内容 |
|---|---|
| `seed/compiler/src/codegen/hir.rs` | `This` → 当前类；兜底前缀须类真声明；`XxxUtils` 去后缀再查 |
| `aura/compiler/aura/lang/compiler/aot/Emit.aura` | `this.aotSlice(...)` → `aotSlice(...)`（源码笔误） |
| `seed/compiler/src/codegen/aot/emit.rs` | 成员访问报错补 `field=` / `owner=` / `obj=`（定位用） |
| `aura/photon/.../PhotonPipeline.aura` | `Step 4a-4d`、`dag=` 进度探针（仅 verbose 时打印） |

---

## 第十轮进展（2026-09-28 续五）—— **P1+P2+P3 全绿：PASS=15 FAIL=0**

```
powershell -File scripts\photon\photon-hat-native-suite.ps1 -Phase P1,P2,P3
→ PASS=15 FAIL=0
```

### 本轮修的 5 个缺陷（全在自举驱动的 Aura 前端，`SsaBuilder.aura` / 无 Rust 改动）

| # | 根因 | 现象 | 修复 |
|---|------|------|------|
| 10.1 | **自由函数调用被追加内存链尾参**：`fun add(a,b)` 的 `add(3,4)` 被写成 `@call @add(3,4,mem)`（3 实参） | 后端按「3 实参的 `add`」判为集合追加 → 改派 `Collections.listAppend` ⇒ `P1/05_functions` 一进 main 就 0xC0000005 | `SsaBuilder.buildCall` 的 mem-token 追加加 `&& !this.isFreeFunc(bareName)` |
| 10.2 | **`s.length()`（带括号）未映射到运行库 `strlen`**（属性式 `s.length` 早已映射） | `length` 的唯一属主是 `StringBuilder` ⇒ `@call @StringBuilder_length(s, mem)`，把字符串指针当 StringBuilder 读 ⇒ `P2/04` 的 `len=4404577004589182066`（VM 为 11） | `resolveMethodSymbol` 里 `recvTy=="String" && (length\|size)` → `"strlen"` |
| 10.3 | **局部量声明类型被截断**：HIR 把可见性以 `#<vis>\|` 前缀写进 `ty` 字段，`localTypes` 记录成 `s3\|#public\|String`，而查表按 `\|` 切分只取到 `#public` | 所有「按声明类型的调用点解析」集体失手（字符串/集合映射全废），正是 10.2 判据失效的原因 | `localTypeAdd` 加 `normAuraType` 剥 `#<vis>\|` 前缀；纯可见性（无类型）不登记 |
| 10.4 | **运行库全局裸名被加了类前缀** | `Syscalls.exit(0)`（`extern interface`，实现由运行库提供）被 `Hir.aura::lowerCall` 的 P1 fix 限定成 `Syscalls_exit` ⇒ `undefined symbol`（`P3/06_syscall_exit`） | `resolveMethodSymbol` 开头：`isRuntimeBareName(bare)` 直接返回裸名 |
| 10.5 | **`extern interface` 的 `Obj_method` 仍带前缀** | 同 10.4 的另一半：即使拿到 `Syscalls_exit` 也要还原成裸名 `exit` | 若 `bare` 前缀 ∈ `externCsv` 且后缀 ∈ `isRuntimeBareName` → 剥前缀（**不动** `Memory_alloc` / `ProcessOps_execve` 这类本就带前缀的符号） |
| — | 套件比对口径 | VM 基线 stdout 混着 `[ok] … compiled (N functions)` 横幅 | 套件比对前剥掉该行 |

### 剩余战线（P0/P4，本轮未修）

`-Phase P0,P4` → **PASS=1 FAIL=12**，可归为 5 类：

1. **布尔打印**：`init: 1` vs VM `init: true`、`Locked: 0` vs `Locked: false`
   （`SsaBuilder` 里已有「按声明返回类型补 `Boolean` 定型」的机制，但覆盖不全）；
2. **对象/reference 打印**：`mutex=140702384279600` vs VM `mutex=<ref#4>`；
3. **ARC**：`retain count: 2` vs VM `0`（`test_arc_refcount`）；
4. **异常**：`Exception: <空>` 且随后 0xC0000005（`test_exception`）；
5. **运行库符号缺口**：`Runtime_init` / `Runtime_isInitialized` /
   `Runtime_getMainThreadId` / `Runtime_getActiveThreadCount` / `Runtime_cleanup` /
   `ThreadOps_currentId` 在 `aura_runtime.obj` 里**没有定义**（`test_runtime_init`、
   `test_thread` 链接失败）—— 需要在 `PhotonRuntime.aura` 里补这些符号。

建议顺序：先 5（补符号，纯增量、能一次性点亮 2 个用例），再 1（布尔定型），
然后 3/4（ARC / 异常，语义较重），最后 2（对象打印格式）。

---

## 第十一轮进展（2026-09-29）—— **P0 已修复：P0+P1+P2+P3 = PASS=16 FAIL=0**

```
powershell -File scripts\photon\photon-hat-native-suite.ps1 -Phase P0,P1,P2,P3
→ TOTAL: PASS=16 FAIL=0
```

### 本轮修的 2 个缺陷（都在 `SsaBuilder.aura`）

| # | 根因 | 现象 | 修复 |
|---|------|------|------|
| 11.1 | **`if` 表达式（值位置）没有 SSA 分支**：`buildExpr` 只认 HirVar/Binary/Unary/Call/Member/Index/Block，`val x = if (c) a else b` 与 `toStr(if …)` 实参位落到默认分支 ⇒ 直接产出 `null`/`0` 常量 | `P0/01_string_eq` 的 `eq_aa/eq_ab/eq_ne_ab` 在 .hat 里全是 `@null : Basic`，打印 `0/0/0`（VM 为 `1/0/1`）。**注意 kind 是 `HirIfExpr`**（`Hir.aura::lowerIfExpr`，`when` 也降级成它），不是语句用的 `HirIf` —— `AURA_PHOTON_TRACE=1` 的 `[expr] default branch for kind=…` 可直接看到 | 新增 `buildIfExpr`：条件 → `CondBr(then,else)`；两分支各求值 → merge 块 `@phi(thenV, elseV)` 作为表达式值（类型取 then 分支实值类型）；并接入 `buildExpr`（`HirIf` / `HirIfExpr` 都走它） |
| 11.2 | **`mapVidOf` 的名字匹配不是「整行匹配」**：只检查「首字符 + 后面紧跟 `\|`」，**没检查是否行首**，且倒序扫描优先命中靠后的行 | 变量表 `a\|537\nb\|538\neq_aa\|548\n` 里查 `a` 命中了 `eq_aa` 末尾那个 `a` ⇒ `a` 被解析成 `eq_aa` 的值；查 `b` 命中 `eq_ab`。于是 `val eq_ab = if (a == b) 1 else 0` 编成 `@call @streq(@t548, @t538)`（@t548 是**上一个 if 的 Phi**），第三个判等更是拿前两个结果比 ⇒ 判等全错 | `mapVidOf` 增加「`p == 0` 或前一个字符是 `\n`」的行首判定 |

> 11.2 是**通用正确性**修复（变量名互为子串时全线错值），
> `varMap` / `varVersion` / `lookupVar` 三个表都走这个函数，一并受益。

### 复现/定位手法（本轮新增）

- `AURA_PHOTON_TRACE=1`：`buildExpr` 默认分支会打印 `[expr] default branch for kind=<kind>` ——
  **识别「前端没实现这个节点」的最快手段**（本轮两个 bug 都是这样定位的）；
- 变量错值类问题：直接读 `<out>/<module>.hat` 里对应 `@call` 的操作数，
  和源码逐一对齐（本轮靠 `@call @streq(@t548, ...)` 一眼看出操作数是「上一个 if 的结果」）。

### 剩余（P4，11 个）

`-Phase P4` → PASS=1 FAIL=11，分类见上一节。其中 `test_memory_alloc` 只差
「布尔打印」（`init: 1` vs VM `init: true`）、`probe_*` 多为对象引用打印格式，
`test_runtime_init` / `test_thread` 是运行库符号缺口（`Runtime_*` / `ThreadOps_currentId`）。

---

## 第十二轮进展（2026-09-29）—— P4：修掉 4 个真 bug，暴露 1 个硬前置 + 1 个口径阻塞

### ✅ 本轮修复（都有可复现证据）

| # | 缺陷 | 现象 → 修复 |
|---|------|-------------|
| 12.1 | **`toStr(<Boolean>)` 不会真值化**：运行库 `toStr` 只做整数→十进制，布尔在生成物里就是 0/1；后端只折叠 `toStr(<Bool **常量**>)` | `P4/test_memory_alloc` 打印 `init: 1`（VM `true`）→ 前端把 `toStr(bool)` 改写成三元式 `flag ? "true" : "false"`（`buildBoolToString`：两个字符串常量 + CondBr + Phi），并新增**类限定返回类型表** `classMethodRetTypes`（`init`/`isLocked` 等同名方法在全局表里是「歧义」，按接收者类查才准确）。**`test_memory_alloc` 转 PASS** |
| 12.2 | **`or`/`and`/`xor`/`shl`/`shr` 的源操作数取错节点**：`emitLogicBinary` / `emitXorRR` / `emitShiftByReg` 固定取 `nodes[0]`，而 `emitArith` 的约定是 `dst,src`（`nodes[0]` 就是**目的节点**）⇒ 发射成 `or %dst, %dst` | `a == 0 \|\| a < 16` 恒为**假** ⇒ `ARC.retain(1)` 的守卫失效、计数错 → 统一改用既有的 `divSrcNode(instr)`（取 `nodes[1]`，与 div/rem 一致）。**`test_arc_refcount` 转 PASS**（retain/release 回到 VM 的 0/0） |
| 12.3 | **`object X` 与同名 `extern interface X` 冲突**：`registerClass` 见到「名字已知」就整体 `return` ⇒ 后一个声明的**方法全丢**（`Runtime.aura` 里既有 `extern interface Runtime` 又有 `object Runtime`） | 改成「同类重复才跳过」，并按**逐个方法**登记 extern 成员（`externMethodsCsv`，新增 `registerClassBodyEx(..., isExternDecl)`）⇒ `Runtime_init` / `isInitialized` / `cleanup` 等**真实 Aura 实现**终于被生成（此前是把它们当自由函数发射成 `init`，调用点 `Runtime_init` 链接期未定义） |
| 12.4 | **`Memory_mmap` 的 Nt 系统调用栈参数偏移错 0x10**：第 5/6 个实参应写在 `[rsp+0x28]/[rsp+0x30]`，`emitPrologue(0x90)` 下即 `rbp-0x68/rbp-0x60`，旧实现写在 `-0x58/-0x50` | 内核读到残留栈数据 ⇒ `STATUS_INVALID_PARAMETER` ⇒ **mmap 恒返回 0**；而 `ArenaAllocator.init` 只判 `baseAddr == -1` ⇒ 把 0 当成功 ⇒「mmap 堆」形同虚设、`alloc` 恒返回 0。修复后 `Memory.mmap(0,{4K,8K,1M,64M},3,34,-1,0)` 全部返回**真实指针** ✓（P4 第 1 项 mmap 堆的真根因） |

> 12.4 的副作用（重要）：`probe_mmap` 由 **PASS 变 FAIL** —— 因为它的 VM 基准
> 打印的是 **0**（VM 的 `Memory.mmap` 是桩），native 修对之后两边反而不一致了。
> 这正是下面 12.6 的现象。

### 🔴 新发现·硬前置：`PhotonObjectWriter` 在函数数变化时错位

往 runtime 对象里**追加 8 个函数**（`ThreadOps_*` / `Runtime_arc*`）后：

- `RELCHK-FINAL funcs=128` → **120**（追加 8 个反而少了 8 个 ⇒ 顶掉了已有函数）
- `Memory_mmap` 开始返回 0（即使把 12.4 修好也复现）

⇒ **必须先修 `PhotonObjectWriter` 的函数/符号表**（函数数变化时的偏移/容量），
才能把 `ThreadOps_currentId` 等接进 runtime 对象。本轮已把那 8 个函数**回退**
（在 `PhotonRuntime.buildRuntimeObject` 里留了注释与现成的 encoder 函数
`emitThreadOpsCurrentId` / `emitStubZero` / `emitStubVoid`），等前置修好再接。

### 🔴 阻塞项（需拍板）：P4 差分基准是 **VM 桩**

P4 用例的期望值来自 VM（`aura run`），而 **VM 侧这批能力本身就是桩**：

| 用例 | VM（桩） | native（越来越对） |
|---|---|---|
| `probe_mmap` | `mmap1: 0` | 真实指针（12.4 修好后）→ **反而 FAIL** |
| `07_mutex_ops` | `After lock: false`（VM 的 lock 没生效） | `false/true/false`（正确） |
| `06_gc_collect` | `null/null/null/null` | `false/false/true/1` |
| `probe_env` | 把变量名当值返回 `CNT=[AURA_ARGV_COUNT]` | `CNT=[]`（真实） |
| `probe_arena` | `capacity: 0 / M usedSize: null` | `capacity: 1048576`（真实） |
| `probe_mutex` | `mutex=<ref#4>`（VM 的引用打印格式） | 原始地址 |
| `test_arc_refcount` | `0/0`（VM 的 ARC 桩） | 修好 12.2 后也是 `0/0` ✓ 对上了 |

两条路线，选一条（推荐 A）：

- **A. 修 VM 侧**：把这批 native/内嵌 stdlib 补成真实实现（`Memory.mmap`、
  `Memory.atomic*`、GC 状态、ARC 计数、`Env.get`、对象引用打印）——
  差分才有意义，且能顺带修掉 VM 的同类缺陷；代价是 Rust VM 侧工作量。
- **B. 给 P4 换基准**：为 `tests/photon/P4/*` 增加 native 期望值文件，
  套件对 P4 用期望值比对（不动 VM）；快，但要为每条用例确定「正确输出」。

### 第十二轮成绩

- `-Phase P0,P1,P2,P3` → **16/16 不变**（无退化）；
- `-Phase P4` → PASS=1 FAIL=11；其中 `test_arc_refcount` 通过，
  `test_memory_alloc` / `probe_mmap` 因「native 更正确」而在口径上翻转，
  `test_thread` 仍链接失败（`ThreadOps_currentId`，等 12.5 前置），
  `test_exception` 仍崩（native 异常支持未做）。
