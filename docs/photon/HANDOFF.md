# AuraLang Photon 自举（Step 4c）交接文档

> 用途：跨对话交接。读完本文可独立继续推进 Photon 后端自举。
> 状态时间点：自举链路「零未定义符号 + 可运行 + CLI 参数通道打通 + 无死循环」，
> 剩余唯一拦路石是 **字符串判等 / 分支控制流（P0）**。

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

自举产物实测证据：

```
[MAIN]   base=2 cnt=6 a0=[Main.exe] a1=[tests\photon\P1\02_simple_vars.aura] a2=[-o] a3=[build\bmain\self.phir]
[CMP]    run=1 same=1 diff=1 ab=1 empty=1     ← "a"=="b" 也是 1；cliArg(1)=="" 也是 1
[RUNCLI] argc=6 a1=[…] a3=[…]
Usage: …                                     ← base=2 ⇒ i 从 3 起 ⇒ 走 usage
```

⇒ **所有 `==` 判真** ⇒ `base=2` ⇒ 走 usage ⇒ Step 4c 未闭环。

两条判别路径（任一即可定位）：

1. 在 `main` 里每个 `if` **之后立即打印**对应 `cX` —— 区分「分支体无条件执行」与「多个局部量共用同一槽」；
2. 查 `emitJcc` / `emitJmp` 的**跳转距离与标签回填**（`main` 是模块最大函数；若按 **rel8** 编码而无范围处理 ⇒ 控制流落到错误目标，恰好表现为"小函数对、大函数错"）。

### P1 —— Step 4c 闭环

让自举产物编译 `tests\photon\P1\02_simple_vars.aura` 并产出 `build\bmain\self.phir`；随后撤掉启动标记与守卫。

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
**唯一拦路石是字符串判等 / 分支控制流（P0）** —— 修掉它 Step 4c 即闭环，之后按 P1 → P4 收尾。

**另有一条独立主线**：类支持缺口 **A3 / A4 / A6**（自举前端无类意识、类方法无 `self` 且裸名发射、
无虚方法分派 —— 见 §2.5，原始定义在 `implementation-deviation-analysis.md` §12.2/§12.3）。
它与 Photon 主链路**互不阻塞**，但两者都依赖同一个回归基线（`photon-hat-native-suite` 15/15），
建议：**先清 P0 闭环 Step 4c，再动 A3/A4/A6**（A 系列开工前务必先用 `class_probe` 复核现状）。
