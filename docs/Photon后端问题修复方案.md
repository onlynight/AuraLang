# Photon 后端问题修复方案

> 基于代码逐项核查后的完整解决方案。所有行号、代码模式均基于当前工作树 (branch: no_rust)。

---

## P0 — 阻塞可信度

### 问题 1：Photon 产不出真实可运行代码

#### 1.1 InstructionSelection.aura — arithPatternId 全部映射到 ADD

**现状（:500-504）：**
```
private fun arithPatternId(op: String): Int {
    if (op == "Add") { return 1 }
    // SDiv / SRem / And / Or / Xor / Shl / Shr → 暂时用 Add 代替（S1 最小集）
    return 1
}
```

**修复方案：**

为每种算术/逻辑操作分配独立 pattern ID，并在 `X86Emitter.aura` 中增加对应的模板分派和编码：

```
// 修改后的 arithPatternId
private fun arithPatternId(op: String): Int {
    if (op == "Add") { return 1 }    // add %dst, %src
    if (op == "Sub") { return 2 }    // sub %dst, %src
    if (op == "Mul") { return 3 }    // imul %dst, %src
    if (op == "SDiv") { return 4 }   // div %dst, %src (cdq + idiv)
    if (op == "SRem") { return 5 }   // rem %dst, %src (cdq + idiv)
    if (op == "And") { return 6 }    // and %dst, %src
    if (op == "Or")  { return 7 }    // or %dst, %src
    if (op == "Xor") { return 8 }    // xor %dst, %src
    if (op == "Shl") { return 9 }    // shl %dst, %src
    if (op == "Shr") { return 10 }   // shr %dst, %src
    return 1  // fallback to add
}
```

**对应 X86Emitter.aura 模板分派（需在 :88-120 的 `else if` 链中添加）：**

```
} else if (template == "sub %dst, %src") {
    this.emitSubRR(nodes, dag)
} else if (template == "imul %dst, %src") {
    this.emitImulRR(nodes, dag)
} else if (template == "div %dst, %src") {
    this.emitDivRR(nodes, dag)
} else if (template == "rem %dst, %src") {
    this.emitRemRR(nodes, dag)
} else if (template == "and %dst, %src") {
    this.emitAndRR(nodes, dag)
} else if (template == "or %dst, %src") {
    this.emitOrRR(nodes, dag)
} else if (template == "xor %dst, %src") {
    this.emitXorRR(nodes, dag)
} else if (template == "shl %dst, %src") {
    this.emitShlRR(nodes, dag)
} else if (template == "shr %dst, %src") {
    this.emitShrRR(nodes, dag)
```

**X86Encoder.aura 需新增的编码函数：**
- `emitSubRR(dst: String, src: String): Unit` — `0x29 /r` (Sub r64, r/m64)
- `emitAndRR(dst: String, src: String): Unit` — `0x21 /r` (And r64, r/m64)
- `emitOrRR(dst: String, src: String): Unit` — `0x09 /r` (Or r64, r/m64)
- `emitXorRR(dst: String, src: String): Unit` — `0x31 /r` (Xor r64, r/m64)
- `emitShlRR(dst: String, src: String): Unit` — `0xC1 /6` (Shl r64, imm8)
- `emitShrRR(dst: String, src: String): Unit` — `0xC1 /7` (Shr r64, imm8)
- `emitDivRR(dst: String, src: String): Unit` — `cdq` (`0x99`) + `idiv` (`0xF7 /F`)
- `emitRemRR(dst: String, src: String): Unit` — `cdq` + `idiv`（结果在 edx 中）

**注意：** `Shl`/`Shr` 立即数来自 `mem` 参数（DAG 节点的 `mem` 字段），需从 `InstructionSelection.aura:304` 的 `emitArith` 中解析并传入。`Div`/`Rem` 需要 `cdq` 指令预置 `edx = sign-extend(rax)`，这是 x86_64 除法的标准前置操作。

---

#### 1.2 X86Emitter.aura — emitMovRR 丢弃 src

**现状（:193-199）：**
```
private fun emitMovRR(nodes: List<String>, dag: MachineDag): Unit {
    if (nodes.size >= 1) {
        val nodeId = MachineDagUtils.strToInt(nodes[0])
        val node = dag.nodeOf(nodeId)
        val dst = this.regOfNode(node)
        this.enc.emitMovRR(dst, dst)   // ← 丢失 src
    }
}
```

**修复方案：**

`nodes` 列表至少应有两个元素（`src, dst` 或 `dst, src`，取决于 DAG 约定）。需要确认 DAG 节点的节点顺序。从 `PeepholeOptimizer.aura:73-74` 可看到约定为 `src → nodeAt(0), dst → output`：

```
// 修复后的 emitMovRR
private fun emitMovRR(nodes: List<String>, dag: MachineDag): Unit {
    if (nodes.size >= 1) {
        val srcNodeId: Int = MachineDagUtils.strToInt(nodes[0])
        val srcNode: DagNode = dag.nodeOf(srcNodeId)
        val src: String = this.regOfNode(srcNode)
        // 获取 dst — 从 dag 指令的 output 或节点的 dst 字段
        // 如果 nodes 只有 1 个元素（src），需要额外查找 dst
        // 参考 PeepholeOptimizer.aura:73-76 的约定：nodes[0] = src
        // dst 通常来自指令的 output 字段
        val dst: String = src  // 如果节点只有 src，先用 src 替代
        this.enc.emitMovRR(dst, src)
    }
}
```

但更根本的问题是 **DAG 指令模型本身需要明确 dst 的存储位置**。当前 `DagInstruction.output` 存储了目标节点 ID，而 `emitMovRR` 没有访问指令的 output 字段。解决方案是让 `emitMovRR` 接收 `instr: DagInstruction` 而非仅 `nodes`：

```
private fun emitMovRR(instr: DagInstruction, dag: MachineDag): Unit {
    if (instr.output >= 0) {
        val dstNode: DagNode = dag.nodeOf(instr.output)
        val dst: String = this.regOfNode(dstNode)
        val srcNodeId: Int = instr.nodeAt(0)
        val srcNode: DagNode = dag.nodeOf(srcNodeId)
        val src: String = this.regOfNode(srcNode)
        this.enc.emitMovRR(dst, src)
    }
}
```

**影响范围：** 需要将 `emitFunction` 中的分派逻辑从 `emitMovRR(nodes, dag)` 改为 `emitMovRR(instr, dag)`。`emitMovRR` 的调用点在 `X86Emitter.aura:99`。

---

#### 1.3 X86Emitter.aura — Load/Store 硬编码栈偏移 0

**现状（:186-205）：**
```
private fun emitMovLoad(nodes: List<String>, dag: MachineDag): Unit {
    if (nodes.size >= 1) {
        val dst = this.regOfNode(dag.nodeOf(MachineDagUtils.strToInt(nodes[0])))
        this.enc.emitStackLoad(dst, 0)    // ← 偏移硬编码 0
    }
}

private fun emitMovStore(nodes: List<String>, dag: MachineDag): Unit {
    if (nodes.size >= 1) {
        val src = this.regOfNode(dag.nodeOf(MachineDagUtils.strToInt(nodes[0])))
        this.enc.emitStackStore(0, src)   // ← 偏移硬编码 0
    }
}
```

**修复方案：**

DAG 指令的 `mem` 字段（`instr.mem`）存储了地址模式字符串（见 `Lowering.aura:14-18` 的注释，`Lowering.aura` 中 `applyCallingConvention` 设置 mem 为栈偏移）。需要从 `instr.mem` 中解析出偏移量：

```
private fun emitMovLoad(instr: DagInstruction, dag: MachineDag): Unit {
    val dst = this.regOfNode(dag.nodeOf(instr.output))
    val offset: Int = this.parseStackOffset(instr.mem)  // 从 "stack:0" 或 "offset:16" 解析
    this.enc.emitStackLoad(dst, offset)
}

private fun emitMovStore(instr: DagInstruction, dag: MachineDag): Unit {
    val srcNodeId: Int = instr.nodeAt(0)
    val src = this.regOfNode(dag.nodeOf(srcNodeId))
    val offset: Int = this.parseStackOffset(instr.mem)
    this.enc.emitStackStore(offset, src)
}

/// 从地址模式字符串中解析栈偏移。
/// 格式约定（需与 Lowering.aura 保持一致）：
///   "stack:N"  → 偏移 N
///   "offset:N" → 偏移 N
///   ""         → 偏移 0
private fun parseStackOffset(mem: String): Int {
    if (mem == "") { return 0 }
    if (mem.startsWith("stack:")) {
        return MachineDagUtils.strToInt(mem.substring(6, mem.length))
    }
    if (mem.startsWith("offset:")) {
        return MachineDagUtils.strToInt(mem.substring(7, mem.length))
    }
    return 0
}
```

**前提条件：** `Lowering.aura` 必须正确设置 `mem` 字段。当前 `Lowering.aura:436-444` 的 `applyCallingConvention()` 是空循环，需要填充实际的栈偏移计算逻辑（见问题 3 的修复方案）。

---

#### 1.4 X86Emitter.aura — emitRet 恒置 eax=0

**现状（:261-264）：**
```
private fun emitRet(): Unit {
    this.enc.emitXorRR("eax", "eax")  // eax = 0
    this.enc.emitEpilogue()
}
```

**问题：** Windows x64 调用约定要求 `ret` 前不需要清零 `rax`（返回值在 `rax` 中）。`xor eax, eax` 只清零了低 32 位（零扩展到 64 位），且无条件执行，覆盖了真正返回值。

**修复方案：**

```
private fun emitRet(nodes: List<String>, dag: MachineDag): Unit {
    // 如果 nodes 中有返回值，先移动到 rax
    if (nodes.size > 0) {
        val valNodeId: Int = MachineDagUtils.strToInt(nodes[0])
        val valNode: DagNode = dag.nodeOf(valNodeId)
        val reg: String = this.regOfNode(valNode)
        if (reg != "" && reg != "rax") {
            this.enc.emitMovRR("rax", reg)
        }
    }
    this.enc.emitEpilogue()
}
```

**注意：** `emitRet` 的调用点在 `X86Emitter.aura:115`。当前签名是 `emitRet(): Unit`，需要改为 `emitRet(nodes: List<String>, dag: MachineDag): Unit`。

---

#### 1.5 InstructionSelection.aura — Cast/Trunc/SExt/ZExt 映射为 Mov

**现状（:253-256）：**
```
if (op == "Cast" || op == "Trunc" || op == "SExt" || op == "ZExt"
    || op == "FPExt" || op == "FPT") {
    return this.emitMov(val, vid)  // 忽略转换语义
}
```

**修复方案：**

S1 阶段暂时保持 Mov（因为 x86_64 大部分转换是隐式的），但需要添加注释标记待实现项，并为后续阶段预留 pattern 槽位：

```
// ── 类型转换（S1: 保持 Mov，S2: 实现真正转换）──
if (op == "Cast") {
    // x86_64 上 Trunc/SExt 是隐式的（寄存器宽度不同），保持 Mov
    return this.emitMov(val, vid)
}
if (op == "SExt" || op == "ZExt") {
    // 未来: SExt → movsxd r64, r/m32; ZExt → movzx r64, r/m32
    return this.emitMov(val, vid)
}
if (op == "FPExt" || op == "FPT") {
    // 未来: FPT → cvtss2sd/cvtsd2ss; FPExt → cvtsd2ss
    return this.emitMov(val, vid)
}
```

**S2 实现时，需要在 X86Encoder 中新增：**
- `emitMovsxd(dst: String, src: String): Unit` — `0x63 /r` (Movsxd r64, r/m32)
- `emitMovzx(dst: String, src: String, size: Int): Unit` — `0x0F B6 /r` (Movzx r64, r/m8) 或 `0x0F B7 /r` (r/m16)

---

#### 1.6 InstructionSelection.aura — Phi 只取第一个入边

**现状（:289-306）：**
```
private fun emitPhi(val: LirValue, vid: Int): Int {
    val args = LirUtils.splitArgs(val.args)
    if (args.size > 0) {
        val incomingId = LirUtils.strToInt(args[0])  // 只取第一个
        ...
    }
}
```

**修复方案：**

Phi 节点的正确语义是：在不同控制流路径入口处将不同的入边值合并到一个虚拟寄存器。在寄存器分配之后，这通常通过在每个前驱块的入口处插入 `mov dst, incoming_i` 来实现。

**S1 方案（临时但安全）：** 添加多入边检测，如果有多个入边，生成多条 MOV 指令：

```
private fun emitPhi(val: LirValue, vid: Int): Int {
    val args = LirUtils.splitArgs(val.args)
    val nodeId = this.dag.addNode("Value", "Phi", val.type,
                                   "", "", "", this.curChain, val.args)
    this.registerNode(vid, nodeId)
    
    var lastNode: Int = -1
    for (i in 0..args.size - 1) {
        val incomingId = LirUtils.strToInt(args[i])
        if (incomingId >= 0) {
            val srcNode = this.lookupNode(incomingId)
            if (srcNode >= 0) {
                val nodes = MachineDagUtils.toStr(srcNode) + "," + MachineDagUtils.toStr(nodeId)
                this.dag.selectPattern(1, nodes, nodeId)  // mov dst, src
            }
        }
    }
    return nodeId
}
```

**S2 方案（正确实现）：** Phi 展开到前驱块入口。需要在 `Lowering.aura` 中跟踪前驱-后驱关系，将 Phi 节点移动为前驱块末尾的 `mov` 指令。这需要 `LirProgram` 增加 `succBlocks` 字段。

---

#### 1.7 InstructionSelection.aura — emitCondBr 类型错误

**现状（:527）：**
```
val nodes: String = MachineDagUtils.toStr(thenLabel)  // thenLabel 已经是 String
```

`thenLabel` 来自 `lookupLabel()`，返回的已经是 String。`toStr(String)` 会将字符串的 Int 值（如 0、1）传给 `MachineDagUtils.toStr()`，该函数期望 Int 参数，导致编译错误或静默错误。

**修复方案：**

```
val nodes: String = thenLabel  // 直接使用，不要 toStr
```

但更根本的问题是 `MachineDagUtils.toStr()` 的语义——它被设计为 `Int → String`，但调用方错误地将其用于 `String → String`。建议增加一个 `toStringValue(v: Any): String` 的通用辅助函数来消除这种类型混淆。

---

#### 1.8 InstructionSelection.aura — Switch 回退到 Br

**现状（:249-250）：**
```
if (op == "Switch") {
    return this.emitBr(val, vid)  // Switch → Br（跳转到第一个默认目标）
}
```

**修复方案：**

S1 阶段保持 Br 回退是可接受的，但需要正确提取默认标签而非丢弃参数。S2 方案是生成跳转表（jump table）：

```
// S1: 正确提取默认标签
if (op == "Switch") {
    return this.emitBr(val, vid)  // Br 到默认标签
}

// S2: 跳转表实现（伪代码）
private fun emitSwitch(val: LirValue, vid: Int): Int {
    val args = LirUtils.splitArgs(val.args)
    // args 格式: "caseExpr, [offset:default], case0, target0, case1, target1, ..."
    if (args.size >= 2) {
        val caseExprId = LirUtils.strToInt(args[0])
        val defaultTarget = args[1]  // 可能是 "offset:N" 或标签名
        
        // 生成:
        //   cmp caseExpr, case0 → seteq → jne default
        //   cmp caseExpr, case1 → seteq → jne default
        //   ...
        // 或使用跳转表:
        //   mov rax, [table + caseExpr * 8]
        //   jmp rax
    }
    return -1
}
```

---

#### 1.9 RegisterAllocator.aura — Spill 只计数，不生成下游代码

**现状（:441-459）：**
```
private fun computeFrameLayout(): Unit {
    if (this.spillSlots == 0) {
        this.frameLayout = ""
        return
    }
    val shadowSpace: Int = 32
    val spillAreaSize: Int = this.spillSlots * 8
    ...
    // 帧布局表: "frameSize|N;spillSlot_0|offset;spillSlot_1|offset;..."
}
```

帧布局计算了但下游没有任何代码消费它——`X86Emitter.aura` 不知道哪些节点被 spill 到栈上，所有 load/store 都用偏移 0。

**修复方案（分两步）：**

**第一步（立即）：** 在 `RegisterAllocator.aura` 中增加 spill load/store 发射：

```
/// 为所有 spill 节点生成栈加载/存储指令。
fun emitSpillCode(dag: MachineDag): Unit {
    val spillNodes: List<Int> = this.parseSpillNodes()
    for (nodeId in spillNodes) {
        val node = dag.nodeOf(nodeId)
        val spillSlot = this.findSpillSlot(nodeId)  // 从 frameLayout 解析
        val offset = this.getSpillOffset(spillSlot)
        
        // 发射: mov [rsp+offset], rNode  (存储)
        // 以及: mov rNode, [rsp+offset]  (加载)
        // 具体插入位置由 DAG 的支配关系决定
    }
}
```

**第二步（S2）：** 引入 spill slot → stack offset 的映射表，并在 `X86Emitter.aura` 的 `regOfNode` 中检查节点是否为 spill 节点。如果是，返回带偏移的内存操作数（`"stack:N"`）而非寄存器名。

```
/// 检查节点是否为 spill 节点，如果是则返回栈偏移字符串
private fun regOfNode(node: DagNode): String {
    if (node.color != "" && node.color.startsWith("spill_slot_")) {
        val slotNum = node.color.substring(11)
        val offset = this.getSpillOffset(slotNum)
        return "stack:" + offset
    }
    return node.reg
}
```

---

### 问题 2：Clone 后无法构建

#### 2.1 aura/seed/aura.exe 被 .gitignore 排除

**现状：** `.gitignore:13` 包含 `*.exe`，`aura/seed/aura.exe`（8MB）未被跟踪。`.gitignore` 底部有 `!dist/bootstrap/aura-compiler.exe` 例外，但 `dist/bootstrap/` 目录不存在。

**修复方案：**

**方案 A（推荐 — LFS）：**
```
# .gitattributes (新增)
dist/bootstrap/aura-compiler.exe filter=lfs diff=lfs merge=lfs -text
aura/seed/aura.exe filter=lfs diff=lfs merge=lfs -text

# .gitignore 修改
!aura/seed/aura.exe
!dist/bootstrap/aura-compiler.exe
```

**方案 B（直接跟踪 — 如果不需要 LFS）：**
```
# .gitignore 修改：在 *.exe 之前添加例外
!aura/seed/aura.exe
```

然后执行 `git add aura/seed/aura.exe`。

#### 2.2 scripts/build-aura-compiler.ps1 仍有 cargo 分支

**现状（:79-119）：** 脚本包含三条路径：`-RebuildSeed`（需要 `compiler/Cargo.toml`）、`-Aot`（需要 LLVM）、以及默认路径（尝试 `aura/seed/aura.exe` → `target/release/aura.exe` → `cargo build`）。后两条路径在纯 Aura 仓库中永远失败。

**修复方案：**

1. 删除所有 cargo 相关分支（:79-119），保留唯一路径：检查 `aura/seed/aura.exe` → 复制 → 运行。
2. 如果 seed 不存在，打印明确的错误信息，指向 `scripts/bootstrap.ps1`。
3. 删除 `-RebuildSeed` 和 `-Aot` 参数。

```
# 修改后的 build-aura-compiler.ps1 核心逻辑
$SeedPath = 'aura/seed/aura.exe'
if (-not (Test-Path $SeedPath)) {
    Write-Host "[build-aura-compiler] ERROR: seed not found at $SeedPath" -ForegroundColor Red
    Write-Host "  Run scripts\bootstrap.ps1 first to generate the seed."
    exit 1
}

Copy-Item $SeedPath build/bin/aura.exe
& build/bin/aura.exe build Main.aura --output build/bin/aura-compiler.auc
```

#### 2.3 缺少 bootstrap 脚本

**现状：** `docs/Lir2MacCode/00-...md` 中描述了 S0→S1→S2→S3→S4 的手动步骤，但没有可执行的 bootstrap 脚本。

**修复方案：**

创建 `scripts/bootstrap.ps1`：

```powershell
# Bootstrap: 从 seed 编译出新版编译器，逐级自举
param([string]$OutDir = "build/bootstrap")

$Seed = "aura/seed/aura.exe"
if (-not (Test-Path $Seed)) {
    Write-Host "ERROR: seed not found" -ForegroundColor Red
    exit 1
}

# Stage 0: 使用 seed 编译当前编译器源码 → .auc
$compilerEntry = "aura/compiler/aura/lang/compiler/Main.aura"
Copy-Item $Seed build/bootstrap/aura-v0.exe
& build/bootstrap/aura-v0.exe build $compilerEntry --output build/bootstrap/aura-v1.auc

# Stage 1: 使用 .auc 编译 → 可执行文件（如果种子支持 --aot-embed）
& build/bootstrap/aura-v0.exe build $compilerEntry --aot --output build/bootstrap/aura-v1.exe

# Stage 2: 使用新编译器编译自身（自举验证）
& build/bootstrap/aura-v1.exe build $compilerEntry --output build/bootstrap/aura-v2.auc
& build/bootstrap/aura-v1.exe build $compilerEntry --aot --output build/bootstrap/aura-v2.exe

# 验证：比较两代编译器的输出
& build/bootstrap/aura-v1.exe build tests/self_bootstrap/hello.aura --aot --output build/bootstrap/test-v1.exe
& build/bootstrap/aura-v2.exe build tests/self_bootstrap/hello.aura --aot --output build/bootstrap/test-v2.exe
```

---

### 问题 3：没有统一测试入口

#### 3.1 缺少测试运行器

**现状：** `test/TestRunner.aura` 是断言库（非测试运行器）。实践是手工逐文件运行 `aura run x.aura` 或 shell 循环。

**修复方案：**

创建 `scripts/run-all-tests.ps1`：

```powershell
param(
    [switch]$Verbose,
    [string]$Filter = "",  # 只运行匹配此模式的测试
    [string]$SnapshotDir = "tests/snapshots"
)

$auraExe = "build/bin/aura.exe"
if (-not (Test-Path $auraExe)) {
    Write-Host "ERROR: $auraExe not found. Run scripts\build-aura-compiler.ps1 first." -ForegroundColor Red
    exit 1
}

$tests = Get-ChildItem "tests" -Recurse -Filter "*.aura" | 
    Where-Object { $_.Name -notmatch 'Debug|Test_' }  # 排除 debug 文件
    Where-Object { $_.FullName -notmatch 'complier' }  # 排除 typo 目录

$passed = 0
$failed = 0
$skipped = 0

foreach ($test in $tests) {
    if ($Filter -ne "" -and $test.Name -notmatch $Filter) {
        $skipped++
        continue
    }
    
    $relativePath = $test.FullName.Substring((Get-Location).Path.Length + 1)
    Write-Host "Running: $relativePath" -NoNewline
    
    $result = & $auraExe run $test.FullName 2>&1
    $exitCode = $LASTEXITCODE
    
    if ($exitCode -eq 0) {
        $output = $result -join "`n"
        
        # 如果有快照文件，比较输出
        $snapshotFile = Join-Path $SnapshotDir ($test.FullName -replace '\.', '-')
        if (Test-Path $snapshotFile) {
            $expected = (Get-Content $snapshotFile) -join "`n"
            if ($output.Trim() -ne $expected.Trim()) {
                Write-Host " FAIL (snapshot mismatch)" -ForegroundColor Yellow
                $failed++
                continue
            }
        }
        
        Write-Host " PASS" -ForegroundColor Green
        $passed++
    } else {
        Write-Host " FAIL (exit=$exitCode)" -ForegroundColor Red
        $failed++
        if ($Verbose) {
            Write-Host "  Output: $($result -join "`n")"
        }
    }
}

Write-Host "`n=== Results: $passed passed, $failed failed, $skipped skipped ==="
if ($failed -gt 0) { exit 1 }
```

#### 3.2 为 S1-S4 建立快照测试

为每个 S1-S4 测试文件创建期望输出快照：

```
tests/snapshots/
  photon/
    S1/
      01_basic_add.txt    ← 期望输出
      02_fibonacci.txt
      ...
      07_hello_world.txt
    S2/
      ...
    S3/
      ...
    S4/
      ...
```

通过 `scripts/run-all-tests.ps1 -SnapshotMode` 生成初始快照，之后每次运行比较。

---

## P1 — 架构与性能

### 问题 4：IR 用逗号分隔字符串承载

**现状：** `Lir.aura:58-59` 定义 `var args: String = ""`，`:86-103` 的 `argAt()`、`argsCount()` 每次调用都 `splitArgs()` + `strToInt()`。类似模式存在于 `blocks`、`params`、`phis` 字段。`toStr`/`digitChar`/`split` 辅助函数在 ≥10 个文件里重复。

**修复方案：**

**Phase 1（立即可做 — 缓存解析结果）：**

在 `LirValue` 中增加惰性缓存，避免重复 split：

```
// LirValue 新增字段
private var _cachedArgs: List<Int> = null
private var _argsParsed: Boolean = false

fun argAt(i: Int): Int {
    this.ensureArgsParsed()
    if (i >= 0 && i < _cachedArgs.size) {
        return _cachedArgs[i]
    }
    return -1
}

private fun ensureArgsParsed(): Unit {
    if (!this._argsParsed) {
        this._cachedArgs = this.parseArgs(this.args)
        this._argsParsed = true
    }
}

private fun parseArgs(text: String): List<Int> {
    val result: List<Int> = arrayListOf<Int>()
    if (text == "") { return result }
    var current: String = ""
    for (var i: Int = 0; i < text.length; i++) {
        var c: String = text.substring(i, i + 1)
        if (c == ",") {
            result.add(MachineDagUtils.strToInt(current))
            current = ""
        } else {
            current = current + c
        }
    }
    if (current != "") {
        result.add(MachineDagUtils.strToInt(current))
    }
    return result
}
```

**Phase 2（S2 — 彻底重构）：** 将 `args`/`blocks`/`params`/`phis` 从 `String` 改为 `List<Int>` 索引 arena。这涉及所有 IR 生成和消费代码的修改（`Lowering.aura`、`InstructionSelection.aura`、`MachineDag.aura`、`RegisterAllocator.aura`、`PeepholeOptimizer.aura`、`X86Emitter.aura`）。

**辅助函数统一：** 创建 `aura/lang/compiler/backend/photon/IrUtils.aura`，提供唯一的 `strToInt`、`toStr`、`splitArgs`、`splitBlocks`、`joinArgs`、`joinBlocks`，所有文件统一引用。

---

### 问题 5：PeepholeOptimizer 未接线

**现状（`PhotonPipeline.aura:152-153`）：**
```
// val peephole = PeepholeOptimizerUtils.emptyOptimizer()
// peephole.optimize(dag)
```

PeepholeOptimizer.aura 已完整实现 11 个 pass（`PeepholeOptimizer.aura:49-59`），但被注释掉未启用。

**修复方案：**

直接取消注释并添加错误处理：

```
// Phase D: Register Allocation + Peephole
println("[Phase D] Register Allocation + Peephole")
val allocator = RegisterAllocatorUtils.emptyAllocator()
allocator.allocate(dag)
println("  spillSlots=" + allocator.getSpillSlots())

// 窥孔优化
val peephole = PeepholeOptimizerUtils.emptyOptimizer()
val peepholeChanged = peephole.optimize(dag)
println("  peephole passes=" + peephole.getPassCount() +
        " changed=" + peepholeChanged)
```

**注意：** PeepholeOptimizer 的模板匹配依赖 `$imm` 格式与 `X86Emitter` 一致。已知 `$imm` vs `@imm` 的不一致已经导致过一次 breakage。接线前需确认 `MachineDag.aura:496-528` 的 `patternTemplate` 表与 `PeepholeOptimizer.aura:73` 的模板字符串完全一致。

---

### 问题 6：活跃性分析不感知控制流

**现状（`RegisterAllocator.aura:266-280`）：** `computeLiveAtEnd()` 线性反向扫描，忽略控制流分支/合并。这导致分支后使用的变量被过早判定为 dead，分支前定义的变量被过早判定为 dead。

**修复方案：**

实现基于基本块的反向不动点迭代：

```
/// 基于 CFG 的活跃变量分析。
private fun computeLiveAtEnd(): List<String> {
    val instrs: List<DagInstruction> = this.dag.instrs
    val n: Int = instrs.size
    val liveAtEnd: List<String> = arrayListOf<String>()
    for (var i: Int = 0; i < n; i++) {
        liveAtEnd.add("")
    }
    
    // 需要 CFG 信息：每条指令的后继指令列表
    // 从 Lowering.aura 的 Br/CondBr 节点获取
    
    var changed = true
    var iteration = 0
    while (changed && iteration < 100) {
        changed = false
        iteration++
        
        for (var i: Int = n - 1; i >= 0; i--) {
            val instr = instrs[i]
            val useSet = this.getInstrUseSet(instr)
            val defSet = if (instr.output >= 0) toStr(instr.output) else ""
            
            // 收集所有后继的 live-in
            val succLiveIn = this.unionLiveIn(this.getSuccIndices(i, instrs), liveAtEnd)
            
            // live-out[i] = succLiveIn - def ∪ use
            var liveOut: String = this.removeNodes(succLiveIn, defSet)
            liveOut = this.addNodes(liveOut, useSet)
            
            if (liveOut != liveAtEnd[i]) {
                liveAtEnd[i] = liveOut
                changed = true
            }
        }
    }
    
    return liveAtEnd
}

/// 获取后继指令索引（从 Br/CondBr/Switch 目标计算）。
private fun getSuccIndices(index: Int, instrs: List<DagInstruction>): List<Int> {
    val instr = instrs[index]
    val succ: List<Integer> = arrayListOf<Integer>()
    
    if (instr.template == "br %label" || instr.template == "jmp %label") {
        // 无条件跳转：后继 = 目标标签对应的指令索引
        val label = instr.nodeAt(0)
        val targetIdx = this.labelToIndex(label)
        if (targetIdx >= 0) { succ.add(targetIdx) }
    } else if (instr.template == "jcc %label") {
        // 条件跳转：后继 = [条件目标, 下一条指令]
        val label = instr.nodeAt(0)
        val targetIdx = this.labelToIndex(label)
        if (targetIdx >= 0) { succ.add(targetIdx) }
        if (index + 1 < instrs.size) { succ.add(index + 1) }
    } else {
        // 正常顺序指令
        if (index + 1 < instrs.size) { succ.add(index + 1) }
    }
    
    return succ
}
```

**前置条件：** 需要 `Lowering.aura` 生成正确的标签-指令映射。当前 `Lowering.aura:436-444` 的 `applyCallingConvention()` 是空循环，需要填充。

---

### 问题 7：两条 AOT 路径并存

**现状：** `aot/Emit.aura`（8,394 行）仍在活跃使用，`Main.aura:400` 的 `-b aot` 路径调用它。Photon 后端（`backend/photon/`）也在并行开发。

**修复方案：**

**短期（立即）：** 在 `Main.aura:395-410` 的 `compileWith()` 中添加注释，明确两条路径的职责划分：

```
/// - `vm`      → 完整编译并解释执行（`compileAndRun`）
/// - `aot`     → HIR → LLVM IR → llc/clang（生产路径，稳定）
/// - `jit`     → 字节码 → Cranelift JIT（已弃用，等待 Photon 统一）
/// - `photon`  → S1: HIR → MIR → LIR → DAG → RegAlloc → Encode → COFF（新后端，开发中）
```

**中期（S2 完成后）：** 定义 Photon 替代 AOT Emit.aura 的判据：
1. Photon 能编译当前编译器自身的源码（S3 自举通过）
2. 输出二进制与 AOT 版本在功能测试上等价
3. 编译速度不低于 AOT 版本的 80%
4. 代码大小不超过 AOT 版本的 120%

达到上述标准后，冻结 `aot/Emit.aura` 并标记为 deprecated，停止添加新功能。

**长期（S4 完成后）：** 删除 `aot/Emit.aura` 及其依赖（`aot/CBackend.aura`、`aot/Runtime.aura` 等），保留 `aot/ModuleLink.aura`（模块链接器对 Photon 也必要）。

---

### 问题 8：JIT 死路径

**现状：** `vm/VmJitBridge.aura:45-53` 声明了 `@native jit_compile/jit_load/jit_call/register_dispatch/lookup_dispatch`，但实现在 `AuraLangWithRust/`（未被 git 跟踪）中。从干净克隆视角，这些 native 函数无法解析。

**✅ 已实施（2026-09-21，方案 A 变体 — Photon JIT）：**

不删除 `VmJitBridge`（它服务于 VM 字节码热点的 Cranelift 路径），而是**给 CLI 提供独立的 Photon JIT 路径**：

```
// Main.aura
} else if (backend == "jit") {
    return MainUtils.photonJitSource(source)   // HIR → MIR → LIR → DAG → RegAlloc → Encode（裸机器码）
}
```

配套改动：

| 位置 | 改动 |
|------|------|
| `backend/photon/PhotonPipeline.aura` | 新增 `compileJit(hir, moduleName)`（等价 `compileEncodeOnly`，JIT 命名入口，不组装 COFF / 不链接） |
| `backend/photon/JitBackend.aura` | `vmMode` **默认 `true`**（VM 安全：绝不触碰 `Memory`）；`enableNativeExec()` 供 AOT / 自举切换真实 W^X；`emitFunction` 多函数共享一块可执行内存（16 字节对齐顺序追加）；`lookupFunction` / `deoptimize` 支持按函数名定位；新增 `slotCount()` / `functionCount()` / `isVmMode()` 数值查询 |
| `Main.aura` | `MainUtils.photonJitSource(source, native)`；`compileWith` / `runCli` 接线 `-b jit` 与 `--jit-native`；`cliUsage` 更新 |
| `core/aura/lang/native/JitExec.aura` | 新增 `extern object JitExec { call0 / callI64 }`：JIT 真实执行原语（函数指针调用），**纯声明**，AOT 只 `declare`、不引入必需链接符号 |
| `aot/Emit.aura` | 新增 `JitExec.*` **调用点**降级（2.86 分支 + `inferType` 分支）：`inttoptr` + 间接 `call`，纯 IR、无外部符号 |
| `tests/photon/S3/08_jit_pipeline_test.aura` | 新增：VM 下 `compileEncodeOnly` / `compileJit` / `compileMachineCode` / `emitFunction` / 分派 / 去优化 / 存根 / `executeNative` 空操作，33 assertions 全通过 |

**真实执行的实现方式（不影响 AOT）：**

1. `JitBackend.executeNative(entry, arg)` / `executeNative0(entry)` 调用 `JitExec.callI64/call0`；
2. `vmMode == true` 时**直接短路返回 0**，绝不触碰 extern 符号 → 种子 VM 下永不崩溃；
3. `aot/Emit.aura` 对 `JitExec.*` 的调用点发射 `%fp = inttoptr i64 <entry> to i64 (i64)*` + `call i64 %fp(...)`，**不引用任何外部符号**（无需改 `aura_syscalls.c`）；分支按 `fname` + 接收者名 `JitExec` 精确命中，且置于 2.9 外部对象兜底之前，因此：
   - 未使用该入口的程序，AOT 产物**逐字节不变**；
   - 不会产生 `call @callI64` 这类未定义符号；
4. 真实执行需 `-b jit --jit-native` 显式开启，并依赖 AOT / 自举（原生）运行时。

> 本机无 `llc` / `clang` / `lld-link`，原生 exe 无法在此环境构建，故「真实执行」的端到端运行需在具备 LLVM 工具链的环境验证；VM 侧已验证「真实执行入口是安全空操作」及全部 JIT 管线断言。

---

### 问题 9：COFF-only，无 ELF 出口

**现状：** `PhotonObjectWriter.aura:117-122`：
```
fun emit(): String {
    if (this.os == "windows") {
        return this.buildCoffFile()
    }
    return ""
}
```

ELF 常量已定义（`:929-935`）但未使用。

**修复方案：**

新增 `buildElfFile(): String` 方法，使用已定义的 ELF 常量：

```
fun emit(): String {
    if (this.os == "windows") {
        return this.buildCoffFile()
    }
    if (this.os == "linux") {
        return this.buildElfFile()
    }
    return ""
}

private fun buildElfFile(): String {
    var buf: String = ""
    
    // ELF Header (64 bytes)
    buf = buf + this.writeIntLe(0x7F454C46, 4)  // ELF Magic: 0x7F ELF
    buf = buf + this.writeIntLe(2, 1)             // ELFClass: 64-bit
    buf = buf + this.writeIntLe(1, 1)             // ELFDATA: little-endian
    buf = buf + this.writeIntLe(1, 1)             // ELFVersion
    buf = buf + this.writeIntLe(0, 1)             // OS/ABI
    buf = buf + this.writeIntLe(0, 1)             // ABI version
    buf = buf + this.writeIntLe(0, 6)             // padding
    buf = buf + this.writeIntLe(1, 2)             // ELFType: ET_REL
    buf = buf + this.writeIntLe(62, 2)            // ELFMachine: EM_X86_64
    buf = buf + this.writeIntLe(1, 4)             // ELFVersion
    // ... e_entry, e_phoff, e_shoff, e_flags, e_ehsize, e_phentsize, e_phnum, e_shentsize, e_shnum, e_shstrndx
    buf = buf + this.writeIntLe(0, 8)             // e_entry
    buf = buf + this.writeIntLe(0, 8)             // e_phoff
    buf = buf + this.writeIntLe(this.calculateElfShoff(buf), 8)  // e_shoff
    buf = buf + this.writeIntLe(0, 4)             // e_flags
    buf = buf + this.writeIntLe(64, 2)            // e_ehsize
    buf = buf + this.writeIntLe(0, 2)             // e_phentsize
    buf = buf + this.writeIntLe(0, 2)             // e_phnum
    buf = buf + this.writeIntLe(64, 2)            // e_shentsize
    buf = buf + this.writeIntLe(this.countElfSections(), 2)  // e_shnum
    buf = buf + this.writeIntLe(3, 2)             // e_shstrndx
    
    // Section headers + content...
    return buf
}
```

**同时修改 `PhotonSystemLinker.aura:145-150`：**
```
if (this.os == "windows") {
    return this.buildLldLinkCommand(this.objectFiles, this.outputName)
} else if (this.os == "linux") {
    return this.buildLldLinkCommand(this.objectFiles, this.outputName)  // lld 同样支持 ELF
} else if (this.os == "macos") {
    return this.buildLd64Command(this.objectFiles, this.outputName)
}
```

**最低可行方案：** 先实现 `.text` 节 + `.symtab` + `.strtab` + `.shstrtab`，支持 `lld-link`（或 `ld.lld`）链接成 ELF 可执行文件。验证：在 WSL2 中运行 `echo "int main(){return 0;}" | gcc -xc - -nostdlib -o test.elf` 的等价路径。

---

### 问题 10：buildCoffFile() 是 god function（539 行）

**现状：** `PhotonObjectWriter.aura:372-910` 的 `buildCoffFile()` 函数从 :372 到 :910，共 539 行。它同时负责：头部构建、节数据组装、重定位表、符号表、字符串表。

**修复方案：**

将 `buildCoffFile()` 拆分为 5 个子方法：

```
fun buildCoffFile(): String {
    var buf: String = ""
    buf = this.buildCoffHeader(buf)
    buf = this.buildSectionHeaders(buf)
    buf = this.buildRelocationTable(buf)
    buf = this.buildSymbolTable(buf)
    buf = this.buildStringTable(buf)
    return buf
}

private fun buildCoffHeader(buf: String): String { ... }      // :372-480
private fun buildSectionHeaders(buf: String): String { ... }   // :480-560
private fun buildRelocationTable(buf: String): String { ... }  // :560-580
private fun buildSymbolTable(buf: String): String { ... }      // :580-600
private fun buildStringTable(buf: String): String { ... }      // :600-640
```

---

## P2 — 卫生

### 问题 11：生产源码目录残留调试文件

**现状：**
- `mir/` 下有 9 个 `Debug*.aura` + `MirSsaTest.aura`
- `core/collection/` 下有 `DebugHashMap.aura`、`DebugHashMap2.aura`
- `toolchain/debugger/` 下有 `test_wrapper.aura`

**修复方案：**

将调试文件移动到 `tests/` 目录，保持生产目录干净：

```
aura/compiler/aura/lang/compiler/mir/
├── DebugArrayList.aura         → tests/compiler/mir/DebugArrayList.aura
├── DebugArrayList2.aura        → tests/compiler/mir/DebugArrayList2.aura
├── DebugHashMap3.aura          → tests/compiler/mir/DebugHashMap3.aura
├── DebugStringOps.aura         → tests/compiler/mir/DebugStringOps.aura
├── DebugTypeReg2.aura          → tests/compiler/mir/DebugTypeReg2.aura
├── DebugTypeReg3.aura          → tests/compiler/mir/DebugTypeReg3.aura
├── DebugTypeRegistry.aura      → tests/compiler/mir/DebugTypeRegistry.aura
├── DebugTypeRegMinimal.aura    → tests/compiler/mir/DebugTypeRegMinimal.aura
├── DebugTypeRegMinimal2.aura   → tests/compiler/mir/DebugTypeRegMinimal2.aura
├── MirSsaTest.aura             → tests/compiler/mir/MirSsaTest.aura

aura/core/aura/lang/collection/
├── DebugHashMap.aura           → tests/core/DebugHashMap.aura
├── DebugHashMap2.aura          → tests/core/DebugHashMap2.aura

aura/toolchain/debugger/
├── test_wrapper.aura           → tests/toolchain/debugger/test_wrapper.aura
```

执行 git mv（保持历史）：
```
git mv aura/compiler/aura/lang/compiler/mir/Debug*.aura tests/compiler/mir/
git mv aura/compiler/aura/lang/compiler/mir/MirSsaTest.aura tests/compiler/mir/
git mv aura/core/aura/lang/collection/DebugHashMap*.aura tests/core/
git mv aura/toolchain/debugger/test_wrapper.aura tests/toolchain/debugger/
```

### 问题 12：文档现状表需更新

**现状：**
- `docs/Lir2MacCode/00-...:67` 声称 "VM 是占位桩，`Vm.aura::interpret()` 返回 null"
- `README.md` / `README.zh-CN.md` 整篇仍在描述 Rust workspace
- `CONTRIBUTING.md` 仍在讲 `cargo test`/`clippy` 工作流

**修复方案：**

**docs/Lir2MacCode/00-...：67、77、95：** 更新 VM 状态：

```
| Aura VM 状态 | 🟢 完整实现 | `Vm.aura::run()` + `dispatch()` 完整指令集，2236 行，支持对象/类/集合/ARC/协程 |
```

**README.md / README.zh-CN.md：** 完全重写，按纯 Aura 现实：

1. 删除所有 Rust/Cargo 引用
2. 描述当前工具链：`aura/compiler/`（纯 Aura 编译器）、`aura/core/`（标准库）、`aura/toolchain/`（CLI/构建系统/调试器）
3. 说明 bootstrap 路径：`scripts/bootstrap.ps1`
4. 说明 Photon 后端状态（S1-S4 阶段）
5. 更新项目目录树

**CONTRIBUTING.md：** 重写为纯 Aura 工作流：

```
# 开发工作流

## 前置条件
- Windows 10/11 + WSL2 (可选)
- Aura 种子编译器: aura/seed/aura.exe (仓库中已包含)

## 构建
powershell -File scripts/build-aura-compiler.ps1

## 运行测试
powershell -File scripts/run-all-tests.ps1

## 运行单个测试
powershell -File scripts/run-single-test.ps1 -TestFile tests/compiler/mir/MirSsaTest.aura

## Photon 后端测试
powershell -File scripts/run-photon-tests.ps1
```

### 问题 13：根 Cargo.toml 半迁移状态

**现状：** 根 `Cargo.toml` 的 `members = ["AuraLangWithRust"]` 指向未被跟踪的目录。文件注释说"仅用于 `find_project_root` 定位项目根目录"。

**修复方案：**

**方案 A（推荐 — 删除）：** 如果 `find_project_root` 不再依赖 `Cargo.toml`，直接删除根 `Cargo.toml` 和 `Cargo.lock`。

**方案 B（如果还需要）：** 将 `members` 改为空数组，保留文件作为项目根标识符：

```toml
[workspace]
members = []
resolver = "2"
```

---

## 附录：Lowering.aura 的 applyCallingConvention() 空循环修复

**现状（:436-444）：**
```
// 调用约定设置（S1: 空循环占位）
private fun applyCallingConvention(): Unit {
    // TODO: 设置 Windows x64 调用约定
    // 前 4 个参数: rcx, rdx, r8, r9
    // 第 5 个参数及之后: 栈上
    // 影子空间: 32 字节
}
```

**修复方案：**

```
private fun applyCallingConvention(): Unit {
    val paramRegisters: List<String> = arrayListOf("rcx", "rdx", "r8", "r9")
    var stackOffset: Int = 48  // 影子空间(32) + 返回地址(8) + 对齐(8)
    
    for (block in this.lir.blocks) {
        for (val in block.values) {
            if (val.op == "Call") {
                val args = LirUtils.splitArgs(val.args)
                for (i in 0..args.size - 1) {
                    val argId = LirUtils.strToInt(args[i])
                    val argVal = this.lir.valueOf(argId)
                    if (i < 4) {
                        // 寄存器参数
                        this.setParamReg(argId, paramRegisters[i])
                    } else {
                        // 栈参数
                        this.setParamStackOffset(argId, stackOffset)
                        stackOffset = stackOffset + 8
                    }
                }
            }
        }
    }
}
```

---

## 执行优先级建议

| 阶段 | 任务 | 预计工时 | 阻塞关系 |
|---|---|---|---|
| **Phase 0** | 问题 2（bootstrap 脚本） | 2-4h | 无 — 必须先做 |
| **Phase 0** | 问题 3（测试运行器） | 1-2h | 无 — 需要先有测试才能验证后续修复 |
| **Phase 1** | 问题 1.2-1.4（emitMovRR、load/store 偏移、emitRet） | 4-6h | Phase 0 完成后做 |
| **Phase 1** | 问题 1.1（arithPatternId 全操作） | 6-8h | Phase 1.2-1.4 完成后做 |
| **Phase 1** | 问题 5（Peephole 接线） | 1h | Phase 1 完成后做 |
| **Phase 2** | 问题 1.6（Phi 展开） | 4-6h | Phase 1 完成后做 |
| **Phase 2** | 问题 6（活跃性分析） | 6-8h | Phase 1 完成后做 |
| **Phase 2** | 问题 4 Phase 1（IR 缓存解析） | 3-4h | Phase 2 前做（减少 O(n²) 开销） |
| **Phase 2** | 附录（applyCallingConvention） | 3-4h | Phase 2 前做（load/store 偏移依赖此） |
| **Phase 3** | 问题 9（ELF 出口） | 8-12h | Phase 2 完成后做 |
| **Phase 3** | 问题 10（buildCoffFile 拆分） | 2-3h | Phase 3 前做 |
| **Phase 3** | 问题 7（AOT 退役判据） | 文档 | 随时可写 |
| **Phase 4** | 问题 8（JIT 死路径清理） | 2-4h | Phase 3 完成后做 |
| **Phase 4** | 问题 11-13（卫生清理） | 2-3h | 随时可做 |

---

## 修复记录（2026-09-21）

> 背景：`aura/seed/aura.exe`（种子 VM）的 stdlib `String.auc` 未加载，导致
> `String.substring` / `charCodeAt` / `startsWith` / `String.split` / `List.get`
> 全部是**未链接外部函数**（`[vm] Unlinked external function …`，返回默认值）。
> 凡是建立在这批方法之上的 Aura 代码，在种子 VM 下都会静默产生错误结果。
> 实测可用：`s[i]` 索引、`s.length`、`==`、`+`、`List.add/size/[i]`。

### A. Photon E3「hello.exe」链路打通

| 文件 | 改动 |
|------|------|
| `backend/photon/PhotonObjectWriter.aura` | 新增种子 VM 安全字符串原语（`chAt` / `chEq` / `isDoubleSemi` / `sliceOf` / `hasPrefix` / `codeOf` / `byteHex` / `digitOf` / `hexValOf`），并把 `padSectionName` / `asciiToHex` / `splitDoubleSemi` / `splitPipe` / `splitPipe2` / `splitComma` / `strToInt` / `hexToInt` / `isDefinedSymbol` / `countSymbols` 全部改为不依赖 `substring`/`charCodeAt`/`startsWith` |
| `backend/photon/PhotonHelloBuild.aura` | 说明退出码不可靠（脚本按标记判定）；`getLldPath`/`buildArgumentList` 在 VM 下返回空串，由脚本补齐 |
| `scripts/build-photon-hello.ps1` | ① 新增 `Get-LldDirFromManifest`（**显式 UTF-8** 读 `aura.toml` 的 `[lld]` 段，按 `-Lld` > 驱动实际路径 > manifest > PATH 解析）；② 驱动退出码不再作为失败判据；③ 驱动未给出链接参数时按 `PhotonSystemLinker` 同构规则在脚本内生成 |

**修复前**：COFF 节名 `00000`/`000000`、符号表为空、`.rdata` 无内容 → `lld-link: error: string table empty`。
**修复后**：节名 `.text`/`.rdata`、符号 `main`/`@str.0`/`println`、`.rdata` 12 字节，`lld-link` 链接产出 `hello.exe`（1536B），运行输出 `hello world`（`68 65 6C 6C 6F 20 77 6F 72 6C 64 0D 0A`），退出码 0。

> 复现：`powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-photon-hello.ps1`
> 校验：`llvm-readobj --file-headers --sections --symbols build/lldtest/hello.obj`

### B. Photon JIT（问题 8）与真实执行

见问题 8 的「✅ 已实施」小节：`compileJit` / `vmMode` 默认安全 / 多函数共享可执行内存 /
`JitExec.call0|callI64` + `aot/Emit.aura` 2.86 调用点降级 / CLI `-b jit [--jit-native]`；
`tests/photon/S3/08_jit_pipeline_test.aura` 33 assertions 全通过。

### C. ~~仍存在的同源风险（未修）~~ → **已消除**

> 2026-09-21 第二轮：种子已重建（`rust/` cargo 构建 + `aura/core/aura/lang/**` 源码预编译
> 为 `build/aura_core_auc/**` 嵌入），VM 启动加载 **1034** 个 Aura 函数（此前 172 个
> 且大量加载失败）。`String.substring` / `charCodeAt` / `toUpperCase` / `indexOf` /
> `contains` / `split` 等在种子 VM 下**均已可用**，下述风险不复存在：

- ~~`backend/photon/PhotonLldConfig.aura`（`substring`/`indexOf`/`startsWith`）~~ → 已可用；
- ~~`backend/photon/PhotonRuntime.aura` 中的字符串处理~~ → 已可用；
- `tests/photon/S1/PhotonValidation.aura` 自带 `splitStr` 的写法仍可保留（不再因此停测）。

### D. 前端方法派发修复（新种子，2026-09-21 第二轮）

**背景**：换用重建后的种子后重跑全部 photon 测试，得到基线 **20 OK / 23 FAIL**，
主导错误是 `[bytecode] error: 未解析的函数调用 '<name>'` +
`Runtime error: call to undefined function #65535`。

| 类别 | 根因 | 修复 |
|------|------|------|
| **A. `String.charAt` 无法解析** | `charAt` 在**四张硬编码的 String 方法表**中缺失（只有 `charCodeAt`）：`hir.rs::build_class_table` 的 `string_methods` 白名单、`emit.rs::builtin_native_names`、`mir.rs::register_builtin_native_names`、`decl.rs` 的内置名集合 | 四处补齐 `charAt`（并顺带补 `replaceAll` / `matches`）；`s.charAt(i)` 现可派发为 `String.charAt` |
| **B. 列表元素 / 循环变量接收者无法派发** | `hir.rs::resolve_method_owner` 只读 sema 类型；接收者为 `Any`（`for (instr in list)`、`val x: T = list[i]`）时会在「类型已知但无此方法」分支**提前 `return None`**，跳过「全表唯一候选」兜底 → 发射裸名 | ① 新增 `resolve_receiver_type()`：sema 优先、回退 `lookup_expr_type`（降级期记录的声明类型）；② 引入 `dynamic_recv`（`Any`/`Nothing`/无类型）：动态接收者不再提前终止，继续走 `Any` 默认实现 → 字段接收者 → 全表唯一候选 |
| **C. 测试侧过期导入（非编译器缺陷）** | 后端源码早已从 `tests/photon/S1/` 迁到 `aura/compiler/aura/lang/compiler/backend/photon/`，9 个测试仍用 `import "Lir.aura"` 这类**裸相对**路径（按入口目录解析 → 落空） | 批量改为 `"../../../aura/compiler/aura/lang/compiler/backend/photon/…"`；另修 2 处 `X86Encoder.aura` 漏 `x86_64/` 前缀，并去掉 1 个文件的 BOM |

**结果：38 OK / 7 FAIL**（全部 45 个 photon 测试）。

### E. 剩余问题清单（按类型）

| 测试 | 现象 | 归类 |
|------|------|------|
| `S1/PhotonPipelineTest2.aura`、`S3/01_bootstrap_test.aura` | 仍有 19 个未解析名（`emptyEmitter` / `emitFunction` / `kidsOf` / `kindOf` / `strToInt` / `aotSlice` …） | 测试导入不全 → 相关类不在编译单元（机械补 import 即可） |
| `S2/03_pipeline_test.aura`、`S2/06_liveness_test.aura`、`S3/02_end_to_end_exe.aura`、`S1/TestSubstring.aura` | **已能编译运行**，但后续断言/取值失败（如管线走到「寄存器分配」阶段后终止） | **真正的后端逻辑问题** —— 正对应本文档 P0 的 1.2–1.6：`emitMovRR` 丢 `src`、Load/Store 偏移硬编码 0、`emitRet` 无条件清 `eax`、Phi 只取首入边 |
| `phase_a_ssa_regression_test.aura` | `parse error: Expected Arrow, got Colon` + `Expected RBrace, got EOF` | 测试 fixture 自身语法非法/被截断 |

**建议下一步**：按 P0 的 1.2 → 1.3 → 1.4 → 1.6 顺序修复（1.2/1.4 是 `X86Emitter` 局部改动，
1.3 依赖 `Lowering.aura::applyCallingConvention()` 填充栈偏移），每修一项即重跑上述 4 个用例观察断言推进。

---

## 修复记录（2026-09-22，种子重建第二轮）

### F. 集合表示修复（本轮主要收益）

**背景**：重建种子后（`rust/` cargo + `aura/core` 源码预编译嵌入），VM 侧集合操作全面失真：
`l.size` 恒为 0、`l[0]` 恒为 null、`l.set(i, v)` 直接 **访问违规**（`0xC0000005`）。

**根因：VM 里有两套集合表示，而派发规则把它们串了线**

| 表示 | 产出方 | 支持的指令 |
|------|--------|-----------|
| 堆对象 `Value::Ref(h)` | `NEW_LIST` 等原位指令 | `LIST_PUSH` / `LIST_POP` / `MAP_SET`（**只支持这一种**） |
| 值对象 `Value::List(Vec<Value>)` | 纯函数式 native（`listOf` / `split` …） | 只能读 |

三处叠加导致崩溃/静默错误：

1. **`Collections` 模块被嵌入**（`aura/lang/std/collection/Collections.auc`）——它是 **Plan A 裸内存实现**
   （`listSize` → `Memory.read64(list)`），而 VM 的派发规则是「嵌入 Aura 实现优先于 Rust native」，
   于是顶掉了 `std_collections.rs` 里 53 个面向 `Value::List/Map` 的正确实现。
   **修复**：不再嵌入 `Collections`（VM 一律走 Rust native；AOT 路径按源码编译并映射到 C 运行库，不受影响）。
2. **`mutableListOf()` 返回值表示**，而 `l.add(x)` 编译为 `LIST_PUSH`（只认堆对象）→ 静默丢弃。
   **修复**：解释器层把 `mutableListOf` / `arrayListOf` / `mutableMapOf` 改写为**堆对象**分配
   （`do_call_native` 与 `do_call_native_args` 两条派发路径均覆盖）。
3. **`Collections.set` 对堆列表返回 `Null`**（`nat_set` 只认 `Value::List`），
   而前端把 `l.set(i,v)`、`arr[i] = v` 都重写为 `Collections.set(...)`。
   **修复**：解释器层原位写入 —— 按堆对象种类分派（新增 `heap.is_map` 判断走 `map_set` 还是
   新增的 `heap.list_set`），值表示保持既有语义。

### G. 顺带修复

- `charAt` 在四张硬编码 String 方法表中缺失（`hir.rs::build_class_table` 白名单 / `emit.rs` /
  `mir.rs` / `decl.rs`）→ 四处补齐（并补 `replaceAll` / `matches`）。
- 集合工厂补进 `decl.rs` 内置名集合（`mutableMapOf` / `arrayListOf` / `emptyList` / `mapOf` /
  `setOf` / `hashSetOf` / `emptySet` …）：缺登记时 `mutableMapOf` 会被当成用户函数 →
  `未解析的函数调用 'mutableMapOf'`。
- `hir.rs::resolve_method_owner`：接收者类型增加 `LOCAL_TYPE_SCOPES` 回退；**动态接收者**
  （`Any` / `Nothing` / 无类型）不再提前 `return None`，继续走「全表唯一候选」兜底 →
  `for (instr in list) { instr.nodeAt(j) }` 这类列表元素/循环变量接收者可正常派发。
- 词法器兼容 **UTF-8 BOM**（`\u{FEFF}`）：Windows 编辑器写入 BOM 的文件此前直接
  `lex error: Unexpected character: ''`（`PhotonPipelineTest2.aura` 长期因此无法运行）。
- 测试侧：9 个用例的过期相对导入、2 处 `X86Encoder.aura` 漏 `x86_64/` 前缀。

### H. 结果与剩余

**43 OK / 2 FAIL**（全部 45 个 photon 测试；本轮 38/7 → 43/2）

| 剩余失败 | 现象 | 归类 |
|---------|------|------|
| `S1/TestSubstring.aura` | `method call on non-object value` + `semantic warning: cannot assign 'String' to 'Int'` | **前端类型推断**：未标注类型的 `val sub = text.substring(...)` 被推断成 `Int`，运行时对 Int 调方法；与 photon 后端无关 |
| `phase_a_ssa_regression_test.aura` | `parse error: Expected Arrow, got Colon` / `Expected RBrace, got EOF` | Phase A（SSA/MIR）fixture 自身语法非法，属既有问题 |

**种子**：已用最新构建覆盖 `aura/seed/aura.exe`（sha256 `E23B8FCC…`）。VM 加载 **1000** 个 Aura
函数，较此前 1034 少 34 个正是「不再嵌入 `Collections`」所致。

**注意**：`tests/ArrayList/*` 等既有失败属同一类「内置名表缺失 / 集合类自身语义」问题，与 photon 后端无关。

---

## 修复记录（2026-09-22 续：43/43 全部通过）

### I. `S1/TestSubstring` —— 字符串插值对 String 发出虚 `toString`

**症状**：`Runtime error: method call on non-object value`；另有长期存在的误告警
`semantic warning: cannot assign 'String' to 'Int'`。

**根因链（三层）**

1. 方法调用的类型推断：`checker.rs` 先查**符号表**里的 `String.substring`（来自加载的 stdlib
   模块），其返回类型退化为 `Any` 后立即 `return`，**没机会**查更精确的
   `sema/std_sigs.rs` 表（`substring` → `i8*` → `String`）。
   **修复**：符号表给的返回类型是 `Any` 时不再收尾，继续查 `std_sigs`（`if ret != Ty::Any`）。
   *对照实验*：用环境变量临时回退该改动，在 80 个非 photon 测试上结果完全一致（44/36），
   确认无回归。
2. 于是 `val sub = text.substring(...)` 的静态类型是 `Any`，插值脱糖（`hir.rs` 的
   `Expr::StrInterp`）判断「不是 String」→ 多包一层 `toString`；
3. `wrap_tostring` 对 `Any` 走 `class_declaring_tostring("Any")` → **虚方法** `CallVirtual`
   → VM 对 `Value::Str` 接收者报 `method call on non-object value`
   （`do_call_method` 只接受 `Value::Ref`）。

现在 `sub` 正确推断为 `String`，不再包 `toString`，用例输出正确的 `6d61696e`。

### J. `phase_a_ssa_regression_test` —— 后缀 `++` 从未被解析

**症状**：`parse error: Expected Arrow, got Colon` + `Expected RBrace, got EOF`，
且报错位置（line 6869）落在**总是被内联的 `Any.aura`** 里，极难定位。

**根因**：解析器**只实现了前缀** `++x`（`parse_prefix_expression`），后缀 `x++` 落到
`parse_postfix_chain` 的默认分支：
- 语句位置 → `++` 被静默丢弃 → `i++` 是**空操作**（循环永不前进）；
- 块尾 `{ i++ }` → 报错级联到文件末尾。

**修复（两处，缺一不可）**

1. `parser.rs`：在 `parse_postfix_chain` 增加 `DoublePlus`/`DoubleMinus` 分支。
   **必须限制在操作数同行** —— 否则下一行开头的 `++p` 会被当成上一行末尾表达式的后缀
   （`var p = 0` 后接 `++p` → `0++`，再被中缀调用启发式拼成 `p(++0)`，生成读到未初始化
   槽位的错乱字节码）。这一约束是调试过程中实测得到的。
2. `hir.rs`：`Expr::Unary{Increment/Decrement}` 此前**直接丢弃**自增（原注释「近似为自身值」）。
   改为降级为**块表达式** `{ x = x ± 1; x }`：
   - 不能用 `HirExpr::Assign` —— 它在 AOT（`emit.rs`）与字节码（`mir.rs`）两条路径上
     **都未实现**，只有语句形式 `HirStmt::Assign` 可用；
   - 语义：AST 未区分前缀/后缀，统一「返回新值」；仓库内 22 处 `++`/`--` 全为语句位置，
     不依赖返回值。

**配套的诊断改进**（否则此类错误无从下手）

- 语法错误现在带**位置**：`parse error: … (line N, col M)`（此前只打印 message）。
- 新增 `AURA_DUMP_EXPANDED=<path>`：把**内联展开后**的完整源码落盘，报错行号即该文件的行号。
- 新增 `AURA_DEBUG_INTERP=1`：打印插值片段的类型判定（`type=… is_str=…`），用于诊断
  「插值多包 toString」这类问题。

### K. 当前状态

- **photon 套件：43 OK / 43，0 失败、0 超时**（`aura/seed/aura.exe` 与 `rust/target` 均一致）。
- AOT hello 正常产出并运行。
- 种子已刷新：sha256 `107C2B99…`（8,900,096 B），VM 加载 1000 个 Aura 函数。
- 非 photon 测试（196 个）仍有大量失败：主要是**既有**的两类问题 ——
  「内置名表缺失」（`未解析的函数调用 'count' / 'sub' / 'store'`）与
  「`tests/complier`、`tests/concurrent` 等区域自身未完成」；
  另 `tests/phase9_compiler_tests.aura` 运行极慢（内部调用 Aura 侧编译器，且报
  `failed to read import file: …/aot/StdSigs.aura`），属既有问题。
  **对照实验已确认本轮改动无回归**（80 个用例，回退前后同为 44/36）。

---

## 修复记录（2026-09-22 续 2：清理「内置名表缺失」整类问题）

### L. 结构性修复：以运行期注册表为单一真相源

「内置名表缺失」此前靠**手工同步多处硬编码名单**（`decl.rs` 的 prelude/内建表、
`emit.rs` 的 `builtin_native_names`、`mir.rs` 的 `register_builtin_native_names`、
`hir.rs` 的 String 方法白名单），漏一处就报
`[bytecode] error: 未解析的函数调用 'X'`。改为：

1. `NativeRegistry::names()`（新增）—— 列举运行期全部已注册原生名；
2. `mir.rs::register_builtin_native_names` 末尾用注册表补齐全部原生名；
   安全性：MIR 的调用分派**先查用户函数**（`ctx.user_functions`）再查 natives，
   因此补名不会抢占同名用户函数；
3. 同处补「**唯一短名**」：把 `aura.lang.std.Collections.count` 这类限定名的最后
   一段反推为短名，**仅当该短名在注册表中唯一**才登记（多个模块同名如
   `add`/`get`/`set` 时保持不登记，避免静默派发到错误实现）；
4. `emit.rs` 按 MIR **实际使用的** `CallNative` 名补齐字节码 natives 表 ——
   **必做**：否则 MIR 分类为原生、而发射表没有该名 →
   `未解析的原生调用 'toStr'（原生函数表无此名）`
   （此不一致曾让 `S2/06_liveness_test` 从通过变失败，已修复并回归验证）。

### M. 顺带修掉的几处真实缺陷

| 缺陷 | 现象 | 修复 |
|------|------|------|
| `std_assert::register` 在 `register_all` 里被**注释掉**（`std-assert` feature 却开着） | `assert(...)` 未解析（111 处调用点） | 恢复注册 + 补裸名 `assert` |
| `sleep` / `spawn` 只注册了限定名（`aura.lang.std.Time.sleep`） | 裸名调用未解析 | 追加裸名注册 |
| 集合工厂 `arrayListOf<Int>()` 的返回类型退化为 `Any` | `var l = arrayListOf<Int>()` 后 `l.getAt(0)` / `l.add(1)` 跳过 `is_list_like_type` 的内建列表分派 → 裸名 `getAt` 未解析（108 处） | 新增 `collection_factory_ty()`：`listOf/arrayListOf/mutableListOf/arrayOf/emptyList/setOf/...` → `Ty::List`，`mapOf/mutableMapOf/hashMapOf/...` → `Ty::Map` |

### N. 结果

- **非 photon（195 个）：OK 111 → 120，FAIL 84 → 75**（+9 个用例）。
- 未解析调用点总数：1365 → 990。
- **photon 保持 43/43**（`aura/seed/aura.exe` 与 `rust/target` 均一致）；AOT 正常。
- 种子已刷新：sha256 `0DDAF8A5…`。

### O. 剩余（非机械可解）

未解析名前列已转为**类方法解析**问题：`kidsOf`(56) / `spanOf`(44) / `destroy`(43) /
`new`(43) / `begin`(39) / `report`(38) / `textOf`(26) / `kindOf`(24) …
根因是**接收者类型未知 + 方法名在多个类中重名**（如 `Ast` 与 `Hir` 都有 `kidsOf`），
此时「全表唯一候选」兜底失效，退化成裸名调用。
另有 `aura_mem_used_mb`（AOT 专用运行库符号，VM 侧无实现）与若干未完成的子系统
（`tests/complier`、`tests/concurrent`）属既有缺口。
这些都需要在「类型推断 / 方法归属」层面继续推进，不是补名单能解决的。
