# 08 · Photon JIT 能力缺口分析与补全计划

> **定位**：详细分析 Photon 后端 JIT 能力的当前状态、缺失项、与 Cranelift 的差距，并给出补全开发计划。
> **核心结论**：JIT 基础设施（JitBackend / compileEncodeOnly / JitExec）**设计正确、部分实现**，但 `compileEncodeOnly` 漏了重定位收集、`emitFunction` 缺了重定位修正、存根地址是占位符、多函数链接和数据段处理未实现。高级 JIT 特性（stack maps / guards / deopt / OSR）全部缺失。
> **配套文档**：`07-Photon-JIT自举设计方案.md`（自举方案）、`docs/photon/photon-self-contained-design-v3.md`（后端设计）
> **文档日期**：2026-09-25

---

## 一、当前状态快照

### 1.1 Photon JIT 已有组件（真实实现，非占位）

| 组件 | 文件 | 行数 | 实现状态 | 说明 |
|------|------|------|----------|------|
| JIT 后端 | `JitBackend.aura` | 697 | ✅ 真实实现 | W^X 内存管理、分派表、存根生成、原生执行入口 |
| 编码管线 | `PhotonPipeline.compileEncodeOnly` | 62 | ⚠️ 部分实现 | HIR→SSA→LIR→DAG→RegAlloc→Encode，产出机器码 hex |
| X86 编码 | `X86Emitter.aura` | 646 | ✅ 完整实现 | DAG→机器码 hex + 重定位记录（AOT 路径可用） |
| X86 编码器 | `x86_64/X86Encoder.aura` | 772 | ✅ 完整实现 | 指令编码，支持标签/跳转/重定位 |
| 原生执行 | `JitExec.aura` | 36 | ✅ 声明完整 | `call0`/`callI64` extern，由 AOT 发射器降低为 `inttoptr+call` |
| JIT 入口 | `PhotonPipeline.compileJit` | 3 | ✅ 别名 | `compileEncodeOnly` 的命名入口 |
| Syscall 发射 | `SyscallEmitter.aura` | 136 | ✅ 完整实现 | Linux/Windows syscall 指令生成 |

### 1.2 Photon JIT 缺失组件（未实现）

| 缺失项 | 说明 | 归因 |
|--------|------|------|
| 重定位收集 | `compileEncodeOnly` 不调用 `getRelocRecords()` | **写漏了**（AOT 路径有，JIT 路径没有） |
| 重定位修正 | `emitFunction` 写入后不修正地址 | **没实现** |
| 存根地址修正 | `buildDispatchStub` 的 `00 00 00 00` 占位符 | **没实现**（注释说"需要链接器修正"） |
| 多函数链接 | JIT 模式下函数间交叉引用无法解析 | **没实现**（AOT 有 lld-link，JIT 无） |
| 数据段处理 | 字符串常量/GOT 条目在 JIT 模式无处理 | **没实现** |
| 重定位数据结构 | `BackendResult` 有 `relocationCount` 但永远为 0 | **写漏了**（字段定义了但没填） |
| Stack maps | GC 无法扫描 JIT 帧 | **没实现**（Cranelift 核心特性） |
| Guards | 投机优化的类型保护指令 | **没实现** |
| Deoptimization | JIT→解释器回退（栈帧映射） | **骨架**（占位符蹦床） |
| OSR | 栈上替换（循环热点优化） | **没实现** |
| 类型反馈 | IC/BC 反馈向量 | **没实现** |
| 多级编译 | T0/T1/T2 分层编译 | **没实现** |
| Code patching | 运行时修改已加载机器码 | **没实现** |

### 1.3 架构约束（非 bug）

| 约束 | 原因 | 解除条件 |
|------|------|----------|
| `vmMode=true` 默认 | 种子 VM 无法解析 `Memory`/`JitExec` extern | 自举完成后调用 `enableNativeExec()` |
| JIT 仅原生运行时可用 | `Memory.alloc`/`mprotect`/`JitExec.callI64` 需真实内存操作 | Stage-2+ 自举产物 |

---

## 二、逐项缺口详解

### 2.1 缺口一：`compileEncodeOnly` 不返回重定位（写漏了）

#### 当前代码

**AOT 路径 `compileHir`（第 286 行）**——正确收集：
```aura
// Phase E: 指令编码 → 目标文件 → 链接
val emitter = X86EmitterUtils.emptyEmitter()
emitter.setAllocator(allocator)
emitter.emitFunction(dag, moduleName)
val machineCodeHex: String = emitter.getOutputHex()     // ✅ 机器码
val relocRecords: String = emitter.getRelocRecords()    // ✅ 重定位
val funcOffsets: String = emitter.getFuncOffsets()       // ✅ 函数偏移表
writer.relocs = relocRecords                            // ✅ 传给 COFF writer
writer.functionOffsets = funcOffsets                      // ✅ 传给 COFF writer
```

**JIT 路径 `compileEncodeOnly`（第 1723 行）**——漏了重定位：
```aura
// Phase E Step 4: 指令编码（仅此一步，不做 COFF/link）
val emitter = X86EmitterUtils.emptyEmitter()
emitter.setAllocator(allocator)
emitter.emitFunction(dag, moduleName)
val machineCodeHex: String = emitter.getOutputHex()     // ✅ 只有 hex
result.machineCodeHex = machineCodeHex                    // ✅ 只返回 hex
// ❌ 没有 emitter.getRelocRecords()
// ❌ 没有 emitter.getFuncOffsets()
// ❌ 没有 selector.stringConstants
```

#### `BackendResult` 结构（第 55 行）

```aura
class BackendResult {
    var success: Boolean = false
    var errorMessage: String = ""
    var objectFilePath: String = ""
    var executablePath: String = ""
    var machineCodeHex: String = ""       // ✅ JIT 填充
    var coffHex: String = ""              // ❌ JIT 不用
    var relocationCount: Int = 0          // ⚠️ JIT 永远为 0（写漏了）
    var functionCount: Int = 0
    var encodedByteCount: Int = 0
    var coffSize: Int = 0
    var linkCommand: String = ""
    var linkExitCode: Int = -1
}
```

**缺失字段**：
- `relocRecords: String` —— 重定位记录字符串（AOT 路径用 `emitter.getRelocRecords()` 获取）
- `funcOffsets: String` —— 函数偏移表（`"funcName|offset;;funcName|offset"`）
- `stringConsts: String` —— 字符串常量列表
- `dataSectionHex: String` —— 数据段 hex（`.rdata`/`.data`）

#### 修复方案

```aura
// BackendResult 增加字段
var relocRecords: String = ""        // 重定位记录
var funcOffsets: String = ""          // 函数偏移表
var stringConsts: String = ""         // 字符串常量
var dataSectionHex: String = ""       // 数据段 hex

// compileEncodeOnly 修复
val machineCodeHex: String = emitter.getOutputHex()
val relocRecords: String = emitter.getRelocRecords()      // 新增
val funcOffsets: String = emitter.getFuncOffsets()         // 新增
val stringConsts: String = selector.stringConstants        // 新增

result.machineCodeHex = machineCodeHex
result.relocRecords = relocRecords                         // 新增
result.funcOffsets = funcOffsets                            // 新增
result.stringConsts = stringConsts                          // 新增
result.encodedByteCount = machineCodeHex.length / 2
result.relocationCount = this.countRelocRecords(relocRecords)  // 新增
```

**修复工作量**：~30 行代码修改，1 天。

---

### 2.2 缺口二：`emitFunction` 不修正重定位（没实现）

#### 当前代码

```aura
// JitBackend.aura 第 429-475 行
fun emitFunction(funcName: String, machineCode: String): Int {
    val codeBytes: Int = machineCode.length / 2
    // ... 分配内存 ...
    this.writeCode(base, offset, machineCode)   // 写入 hex
    this.setExecutable(base, this.codeSize)     // mprotect RX
    return entry                                 // 返回入口
    // ❌ 没有重定位修正
}
```

#### 问题

写入机器码后直接切 RX 权限。机器码中的地址引用（函数调用、跳转、数据引用）都是占位符或错误值，运行时访问会崩溃。

#### 修复方案

需要新增重定位修正步骤：

```aura
// emitFunction 修改后
fun emitFunction(funcName: String, machineCode: String, 
                  relocRecords: String, dataHex: String): Int {
    val codeBytes: Int = machineCode.length / 2
    
    // 1. 分配 RW 内存
    val base = this.allocateExecMemory(this.alignToPage(codeBytes + 64))
    
    // 2. 写入机器码
    this.writeCode(base, 0, machineCode)
    
    // 3. 写入数据段（字符串常量等）
    val dataBase = this.allocateDataMemory(...)
    this.writeData(dataBase, 0, dataHex)
    
    // 4. 修正重定位 ← 新增
    this.applyRelocations(base, codeBytes, relocRecords, dataBase)
    
    // 5. 切换为 RX 权限
    this.setExecutable(base, this.codeSize)
    
    return entry
}

// 新增：重定位修正
private fun applyRelocations(codeBase: Int, codeSize: Int, 
                              relocRecords: String, dataBase: Int): Unit {
    // 解析 "offset|symbol|type;;offset|symbol|type" 格式
    // 对每个重定位：
    //   RIP32: write32(codeBase+offset, targetAddr - (codeBase+offset+4))
    //   ABS64: write64(codeBase+offset, targetAddr)
    //   DATA_REF: write64(codeBase+offset, dataBase + symbolOffset)
}
```

**修复工作量**：~200 行代码新增，3-4 天。

---

### 2.3 缺口三：存根地址占位符未修正（没实现）

#### 当前代码

```aura
// JitBackend.aura 第 331-341 行
fun buildDispatchStub(slot: Int): String {
    // mov rax, [rip + offset]  — 间接通过 RIP 寻址
    // 实际编码需要链接器修正重定位         ← 注释说需要链接器
    // 简化：使用 RIP 相对寻址的占位符
    out = "48" + "8B" + "05" + "00" + "00" + "00" + "00"  // 00 00 00 00 占位符
    out = out + "FF" + "E0"                                  // jmp rax
    return out
}

// 第 343-377 行 buildDeoptTrampoline 同样有占位符
// lea rax, [rip + trampoline_target]
out = "48" + "8D" + "05" + "00" + "00" + "00" + "00"        // 00 00 00 00 占位符
```

#### 问题

- `buildDispatchStub` 的 `00 00 00 00` 应该是分派表条目的 RIP 相对偏移
- `buildDeoptTrampoline` 的 `00 00 00 00` 应该是 VM 解释器入口的 RIP 相对偏移
- 在 JIT 模式下没有链接器修正这些占位符
- 运行时执行到这些指令会跳转到地址 0 或错误地址，导致崩溃

#### 修复方案

存根生成时无法知道最终地址（需要在分配到内存后才能计算）。因此需要**两阶段**：

```text
阶段 1（编译时）：生成存根骨架 + 记录重定位占位符位置
阶段 2（装载时）：分配到内存后，计算实际偏移并 patch 占位符
```

```aura
// 修改后的存根生成
fun buildDispatchStub(slot: Int): StubResult {
    // 返回存根 hex + 重定位位置
    return StubResult(
        hex = "48 8B 05 00 00 00 00 FF E0",
        relocations = listOf(
            Relocation(offset=3, type=RIP32, target="dispatch_slot_$slot")
        )
    )
}

// 装载时修正
fun patchStub(stubBase: Int, targetAddr: Int): Unit {
    // RIP32 修正：target - (stubBase + 3 + 4) = target - (stubBase + 7)
    val relOffset: Int = targetAddr - (stubBase + 7)
    Memory.write32((stubBase + 3) as Long, relOffset as Int)
}
```

**修复工作量**：~150 行代码修改，2 天。

---

### 2.4 缺口四：多函数交叉引用无法解析（没实现）

#### 问题

JIT 编译函数 A 时，函数 A 可能调用函数 B。函数 B 的地址在函数 A 编译时尚未确定（B 可能尚未编译，或尚未分配到内存）。

在 AOT 模式下：
```text
函数 A 编译 → call rel32(占位符) → COFF 目标文件（含重定位表）
函数 B 编译 → 独立目标文件
lld-link → 解析所有重定位 → 函数 A 的 call 修正为函数 B 的实际地址
```

在 JIT 模式下：
```text
函数 A 编译 → call rel32(占位符) → hex → W^X 内存
函数 B 编译 → call rel32(占位符) → hex → W^X 内存
❌ 没有链接步骤 → 函数 A 的 call 目标地址永远是 0
```

#### 修复方案

**方案 A：编译时链接（推荐）**

在 `compileEncodeOnly` 阶段，同时编译所有需要的函数，然后在写入内存前统一修正交叉引用：

```text
1. 收集所有待编译函数（调用图）
2. 逐个编译为 hex + relocations
3. 计算所有函数的相对偏移
4. 修正所有重定位（函数间交叉引用）
5. 写入 W^X 内存
```

```aura
// 新增：JIT 编译批次
fun compileBatch(funcList: List<FuncEntry>): BatchResult {
    // 1. 编译所有函数
    val results: List<FunctionResult> = []
    for (func in funcList) {
        val r = pipeline.compileEncodeOnly(func.hir, ...)
        results.add(r)
    }
    
    // 2. 计算偏移表（函数名 → 相对偏移）
    val offsets: Map<String, Int> = {}
    var totalSize: Int = 0
    for (r in results) {
        offsets.put(r.funcName, totalSize)
        totalSize = totalSize + r.encodedByteCount
    }
    
    // 3. 修正重定位
    for (r in results) {
        this.fixRelocations(r.relocRecords, offsets)
    }
    
    // 4. 写入内存
    val base = this.allocateExecMemory(totalSize)
    for (r in results) {
        this.writeCode(base, offsets.get(r.funcName), r.machineCodeHex)
    }
    this.setExecutable(base, totalSize)
    
    return BatchResult(base, offsets)
}
```

**方案 B：运行时分派表间接调用**

所有函数间调用都通过分派表间接跳转（`call [rip + dispatchTable + slot*8]`），编译时不需要知道目标地址：

```asm
; 函数 A 调用函数 B（通过分派表）
mov rax, [rip + dispatchTable + 8*slot_B]  ; 间接调用
jmp rax
```

**推荐**：方案 A（编译时链接）为主，方案 B（分派表间接调用）为备选。方案 A 更快（直接 call vs 间接 call），方案 B 更简单（无需编译时知道所有地址）。

**修复工作量**：~300 行代码新增，5 天。

---

### 2.5 缺口五：数据段处理缺失（没实现）

#### 问题

JIT 代码可能引用：
- 字符串常量（如 `"Hello, World!"`）
- 常量池（整数/浮点常量表）
- GOT 条目（全局对象表）
- 类型信息表（类元数据）

在 AOT 模式下，这些数据放在 COFF 的 `.rdata`/`.data` 节，链接器分配地址。在 JIT 模式下，`compileEncodeOnly` 不收集这些数据。

#### 修复方案

```text
1. compileEncodeOnly 收集字符串常量 → stringConsts
2. emitFunction 分配数据内存（RW，不需要 RX）
3. 写入数据段
4. 修正代码中的数据引用重定位
```

```aura
// emitFunction 增强
fun emitFunction(funcName: String, machineCode: String,
                 relocRecords: String, stringConsts: String): Int {
    
    // 分配代码内存（RX）
    val codeBase = this.allocateExecMemory(...)
    
    // 分配数据内存（RW，不需要 RX）
    val dataBase = this.allocateDataMemory(...)
    
    // 写入数据段
    if (stringConsts != "") {
        this.writeDataSection(dataBase, stringConsts)
    }
    
    // 写入代码
    this.writeCode(codeBase, 0, machineCode)
    
    // 修正重定位（包括数据引用）
    this.applyRelocations(codeBase, relocRecords, dataBase)
    
    // 切换为 RX
    this.setExecutable(codeBase, codeSize)
    
    return entry
}
```

**修复工作量**：~150 行代码新增，2 天。

---

### 2.6 缺口六：vmMode 约束（架构约束）

#### 当前状态

```aura
var vmMode: Boolean = true  // 默认 true
// 注释：种子 VM 无法解析 Memory/JitExec extern
// 需要调用 enableNativeExec() 才能关闭
```

#### 为什么不能改默认值

如果默认 `vmMode=false`，种子 VM 执行 JIT 代码时：
1. `Memory.alloc` → VM 尝试调用 → `未解析的函数调用` → 崩溃
2. `Memory.mprotect` → VM 尝试调用 → 崩溃
3. `JitExec.callI64` → VM 尝试调用 → 崩溃

**这是正确的设计**——种子 VM 下不应该有真实内存操作。

#### 解除条件

```text
Stage-1: Rust 编译器 → aura-compiler-c2.exe（JIT 基础设施 vmMode=true）
Stage-2: c2.exe 自我编译 → native2.exe（JIT 基础设施 vmMode=false，enableNativeExec 已调用）
Stage-3: native2.exe 编译用户程序（用户程序 JIT 可用）
```

**这不是缺口，是自举链的自然结果**——JIT 只能在自举之后使用。

---

### 2.7 缺口七：高级 JIT 特性缺失（Cranelift 核心特性）

这些特性是 Cranelift 的核心竞争力，Photon 从未实现。按优先级排序：

#### 7.1 Stack maps（GC 必需）

**Cranelift 能力**：
- 编译时为每个 JIT 函数生成 stack map
- Stack map 记录每个调用点处哪些寄存器/栈槽包含 GC 根
- GC 扫描时读取 stack map，标记可达对象

**Photon 缺失**：
- 没有 stack map 数据结构
- 没有 GC 根标记逻辑
- JIT 帧无法被 GC 安全扫描

**影响**：
- JIT 编译的代码如果使用 GC 管理的对象（字符串、数组、对象），GC 扫描时无法识别 JIT 帧中的 GC 根
- 可能导致 use-after-free 或 GC 崩溃
- **这是阻止 JIT 处理复杂代码（字符串/对象操作）的关键障碍**

**修复方案**：
```text
1. 在 X86Emitter 中记录每个 call 点的寄存器状态
2. 生成 stack map 数据结构（每个调用点 → GC 根列表）
3. 将 stack map 附加到函数元数据
4. GC 扫描时读取 stack map，标记 JIT 帧中的 GC 根
```

**工作量**：~500 行代码，2 周。

#### 7.2 Guards（投机优化基础）

**Cranelift 能力**：
- 生成 guard 指令（类型检查 + 条件跳转）
- 类型不匹配时跳入 deopt 路径
- 支持投机内联、类型特化

**Photon 缺失**：
- 没有 guard 指令生成
- 无法做投机优化

**影响**：
- 只能生成保守代码（无法假设类型）
- 性能不如 Cranelift（Cranelift 的 guard 机制允许激进优化后安全回退）

**修复方案**：
```text
1. 在 InstructionSelection 中生成 guard 节点
2. X86Emitter 发射 guard 指令（cmp + jne → deopt_trampoline）
3. Deopt 蹦床跳转到解释器
```

**工作量**：~300 行代码，1.5 周。

#### 7.3 Deoptimization（JIT→解释器回退）

**Cranelift 能力**：
- JIT 代码中 guard 失败时，转移到解释器
- 需要 JIT 帧 → VM 帧的栈帧映射
- 恢复局部变量到 VM 栈

**Photon 当前状态**：
- `buildDeoptTrampoline` 有骨架（第 343-377 行）
- 但有 `00 00 00 00` 占位符（未修正）
- 没有栈帧映射逻辑

**修复方案**：
```text
1. 实现栈帧映射（JIT 帧布局 → VM 帧布局）
2. 实现 deopt 蹦床（保存 JIT 寄存器 → 恢复 VM 栈 → 跳转到解释器）
3. 与 guard 指令配合（guard 失败 → deopt trampoline）
```

**工作量**：~400 行代码，1.5 周。

#### 7.4 OSR（On-Stack Replacement）

**Cranelift 能力**：
- 在循环中检测热点
- 在循环执行期间切换到 JIT 编译的版本
- 不需要函数调用边界

**Photon 缺失**：完全没有实现。

**影响**：
- 只能优化完整函数，不能优化循环热点
- 对于 `for (i=0; i<n; i++)` 这类循环，无法在循环内部切换

**修复方案**：
```text
1. 在循环中插入 counter（每 N 次迭代检查一次）
2. 计数器溢出时跳转到 JIT 编译的循环体
3. 需要循环变量的栈帧映射
```

**工作量**：~300 行代码，1.5 周。

#### 7.5 类型反馈（Type Feedback）

**Cranelift 能力**：
- 运行时收集类型信息（IC/BC 反馈向量）
- 基于反馈做投机优化
- 反馈失效时回退

**Photon 缺失**：完全没有实现。

**影响**：
- 只能生成静态保守代码
- 无法做反馈驱动优化（如单态类型特化）

**修复方案**：
```text
1. 在解释器中收集类型反馈
2. 编译时读取反馈，生成特化代码
3. 运行时检查反馈有效性
```

**工作量**：~500 行代码，2 周。

---

## 三、与 Cranelift 的完整对比

### 3.1 功能对比矩阵

| 功能 | Cranelift | Photon（当前） | Photon（补全后） | 优先级 |
|------|-----------|----------------|------------------|--------|
| 进程内编译 | ✅ | ✅（compileEncodeOnly） | ✅ | 已有 |
| 机器码生成 | ✅ | ✅（X86Emitter） | ✅ | 已有 |
| W^X 内存 | ✅ | ✅（JitBackend） | ✅ | 已有 |
| 分派表 | ✅ | ✅（JitBackend） | ✅ | 已有 |
| 重定位修正 | ✅（编译时） | ❌ | ✅（运行时修正） | **P0** |
| 多函数链接 | ✅（编译时） | ❌ | ✅（编译时批次） | **P0** |
| 数据段处理 | ✅ | ❌ | ✅ | **P1** |
| Stack maps | ✅ | ❌ | ✅ | **P1** |
| Guards | ✅ | ❌ | ✅ | **P2** |
| Deoptimization | ✅ | ⚠️ 骨架 | ✅ | **P2** |
| OSR | ✅ | ❌ | ✅ | **P3** |
| 类型反馈 | ✅ | ❌ | ✅ | **P3** |
| 多级编译 | ✅ | ❌ | ⚠️ 可选 | P4 |
| Code patching | ✅ | ❌ | ⚠️ 可选 | P4 |
| 编译速度 | 1-5ms | 估计 5-20ms | 估计 3-10ms（优化后） | — |

### 3.2 优先级定义

| 优先级 | 定义 | 目标 |
|--------|------|------|
| **P0** | 必须实现，否则 JIT 无法工作 | 基础 JIT 功能（编译→装载→执行） |
| **P1** | 应该实现，否则 JIT 功能不完整 | 安全运行（GC、数据段） |
| **P2** | 建议实现，提升性能 | 优化能力（guard、deopt） |
| **P3** | 可选实现，高级特性 | 极致优化（OSR、类型反馈） |
| **P4** | 远期目标 | 完整 JIT 框架（多级编译） |

---

## 四、补全开发计划

### 总览

```text
Phase 0: 基础验证（0.5 周）
  └─ 确认 Photon JIT 管线在原生运行时可产出可执行机器码

Phase 1: P0 基础功能（3 周）
  ├─ 重定位收集（compileEncodeOnly 修复）
  ├─ 重定位修正（emitFunction 增强）
  ├─ 存根地址修正
  └─ 多函数编译时链接

Phase 2: P1 安全运行（2 周）
  ├─ 数据段处理
  ├─ Stack maps
  └─ GC 集成测试

Phase 3: P2 优化能力（3 周）
  ├─ Guards
  ├─ Deoptimization
  └─ 投机优化

Phase 4: P3 高级特性（3 周）
  ├─ OSR
  ├─ 类型反馈
  └─ 编译速度优化

总计：11.5 周
```

---

### Phase 0：基础验证（0.5 周）

**目标**：确认 Photon JIT 管线在原生运行时能产出可执行机器码。

| # | 任务 | 文件 | 工作量 | 依赖 |
|---|------|------|--------|------|
| 0.1 | 编写最小 JIT 测试程序 | `tests/photon/jit_smoke_test.aura` | 1 天 | 无 |
| 0.2 | 在原生运行时执行（AOT 编译测试程序） | `scripts/test-jit-native.ps1` | 1 天 | 0.1 |
| 0.3 | 验证 `JitBackend.enableNativeExec()` 可工作 | 同上 | 0.5 天 | 0.1-0.2 |

**验收**：
- `JitBackend.allocateExecMemory` 返回非零地址
- `JitBackend.writeCode` 实际写入字节
- `JitBackend.setExecutable` 调用 `Memory.mprotect` 成功
- `JitBackend.executeNative` 通过 `JitExec.callI64` 调用成功

**风险**：如果 `Memory`/`JitExec` extern 在原生运行时不可用，需要修复 AOT 发射器。

---

### Phase 1：P0 基础功能（3 周）

**目标**：让 JIT 编译→装载→执行完整闭环可用。

#### Phase 1.1：重定位收集（1 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.1.1 | `BackendResult` 增加 `relocRecords`/`funcOffsets`/`stringConsts` 字段 | `PhotonPipeline.aura` | 0.5 天 |
| 1.1.2 | `compileEncodeOnly` 调用 `getRelocRecords()`/`getFuncOffsets()` | `PhotonPipeline.aura` | 0.5 天 |

**修改内容**：

```aura
// BackendResult 增加字段
class BackendResult {
    // ... 现有字段 ...
    var relocRecords: String = ""       // 新增
    var funcOffsets: String = ""        // 新增
    var stringConsts: String = ""       // 新增
    var dataSectionHex: String = ""     // 新增
}

// compileEncodeOnly 修复
fun compileEncodeOnly(hir: Hir, outDir: String, moduleName: String): BackendResult {
    // ... Phase A-D 不变 ...
    
    // Phase E: 修复
    val emitter = X86EmitterUtils.emptyEmitter()
    emitter.setAllocator(allocator)
    emitter.emitFunction(dag, moduleName)
    
    result.machineCodeHex = emitter.getOutputHex()
    result.relocRecords = emitter.getRelocRecords()      // 新增
    result.funcOffsets = emitter.getFuncOffsets()          // 新增
    result.stringConsts = selector.stringConstants         // 新增
    result.encodedByteCount = result.machineCodeHex.length / 2
    result.relocationCount = this.countRelocRecords(result.relocRecords)  // 新增
    
    return result
}

// 新增辅助方法
private fun countRelocRecords(records: String): Int {
    if (records == "") { return 0 }
    return records.split(";;").size
}
```

#### Phase 1.2：重定位修正（3-4 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.2.1 | 定义重定位数据结构 | `JitRelocation.aura`（新） | 1 天 |
| 1.2.2 | 实现 `applyRelocations` 方法 | `JitBackend.aura` | 2 天 |
| 1.2.3 | 修改 `emitFunction` 签名，接受重定位参数 | `JitBackend.aura` | 0.5 天 |
| 1.2.4 | 重定位单元测试 | `tests/photon/jit_reloc_test.aura` | 0.5 天 |

**重定位数据结构**：

```aura
// JitRelocation.aura（新文件）

/// 重定位类型
object RelocType {
    const val RIP32: Int = 0      // 相对偏移（call rel32, jmp rel32）
    const val ABS64: Int = 1      // 绝对地址（mov rax, imm64）
    const val DATA_REF: Int = 2   // 数据引用（lea rax, [rip + disp32]）
    const val PCREL32: Int = 3    // PC 相对（lea rax, [rip + disp32]）
}

/// 重定位条目
class Relocation {
    var offset: Int = 0    // 在机器码中的字节偏移
    var symbol: String = "" // 目标符号名
    var type: Int = 0      // 重定位类型（RelocType）

    init(offset: Int, symbol: String, relocType: Int) {
        this.offset = offset
        this.symbol = symbol
        this.type = relocType
    }
}

/// 重定位处理器
object JitRelocationUtils {

    /// 解析重定位记录字符串为列表
    /// 格式："offset|symbol|type;;offset|symbol|type;;..."
    fun parseRelocs(records: String): List<Relocation> {
        val result = ArrayList<Relocation>()
        if (records == "") { return result }
        val entries = records.split(";;")
        var i = 0
        while (i < entries.size) {
            val parts = entries[i].split("|")
            if (parts.size >= 3) {
                val offset = parseInt(parts[0])
                val symbol = parts[1]
                val type = parseInt(parts[2])
                result.add(Relocation(offset, symbol, type))
            }
            i = i + 1
        }
        return result
    }

    /// 应用重定位修正
    fun applyRelocations(codeBase: Int, codeSize: Int, 
                         relocRecords: String, dataBase: Int,
                         funcOffsets: String): Unit {
        val relocs = JitRelocationUtils.parseRelocs(relocRecords)
        val funcMap = JitRelocationUtils.parseFuncOffsets(funcOffsets)
        
        var i = 0
        while (i < relocs.size) {
            val reloc = relocs[i]
            val targetAddr = JitRelocationUtils.resolveSymbol(
                reloc.symbol, funcMap, codeBase, dataBase)
            
            if (reloc.type == RelocType.RIP32) {
                // RIP32: target - (codeBase + offset + 4)
                val insnEnd = codeBase + reloc.offset + 4
                val relOffset = targetAddr - insnEnd
                Memory.write32((codeBase + reloc.offset) as Long, relOffset as Int)
            } else if (reloc.type == RelocType.ABS64) {
                // ABS64: 直接写入目标地址
                Memory.write64((codeBase + reloc.offset) as Long, targetAddr as Long)
            } else if (reloc.type == RelocType.DATA_REF) {
                // DATA_REF: 写入数据段中的偏移
                val dataOffset = JitRelocationUtils.dataSymbolOffset(
                    reloc.symbol, dataBase)
                Memory.write64((codeBase + reloc.offset) as Long, dataOffset as Long)
            } else if (reloc.type == RelocType.PCREL32) {
                // PCREL32: target - (codeBase + offset + 8) [lea 指令 8 字节]
                val insnEnd = codeBase + reloc.offset + 8
                val relOffset = targetAddr - insnEnd
                Memory.write32((codeBase + reloc.offset + 4) as Long, relOffset as Int)
            }
            
            i = i + 1
        }
    }
}
```

#### Phase 1.3：存根地址修正（2 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.3.1 | 修改 `buildDispatchStub` 返回 `StubResult`（hex + 重定位位置） | `JitBackend.aura` | 1 天 |
| 1.3.2 | 修改 `buildDeoptTrampoline` 返回 `StubResult` | `JitBackend.aura` | 0.5 天 |
| 1.3.3 | 实现 `patchStub` 方法（运行时修正占位符） | `JitBackend.aura` | 0.5 天 |

**修改内容**：

```aura
/// 存根结果（hex + 重定位信息）
class StubResult {
    var hex: String = ""
    var relocations: List<Relocation> = ArrayList<Relocation>()
    
    init(hex: String, relocs: List<Relocation>) {
        this.hex = hex
        this.relocations = relocs
    }
}

// 修改后的 buildDispatchStub
fun buildDispatchStub(slot: Int): StubResult {
    val hex = "48 8B 05 00 00 00 00 FF E0"
    // 重定位位置：offset=3（0x48 0x8B 0x05 [disp32]）
    // 类型：RIP32，目标：dispatch_slot_<slot>
    val relocs = ArrayList<Relocation>()
    relocs.add(Relocation(3, "dispatch_slot_$slot", RelocType.RIP32))
    return StubResult(hex, relocs)
}

// 新增：修正存根
fun patchStub(stubBase: Int, stubResult: StubResult): Unit {
    var i = 0
    while (i < stubResult.relocations.size) {
        val reloc = stubResult.relocations[i]
        if (reloc.type == RelocType.RIP32) {
            // 查找分派表条目
            val slot = parseSlotFromSymbol(reloc.symbol)
            val targetAddr = this.getDispatchEntry(slot)
            // RIP32 修正
            val insnEnd = stubBase + reloc.offset + 7  // 指令总长 7 字节
            val relOffset = targetAddr - insnEnd
            Memory.write32((stubBase + reloc.offset) as Long, relOffset as Int)
        }
        i = i + 1
    }
}
```

#### Phase 1.4：多函数编译时链接（5 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.4.1 | 实现 `compileBatch` 方法（批量编译） | `JitBackend.aura` | 2 天 |
| 1.4.2 | 实现函数间交叉引用修正 | `JitBackend.aura` | 1 天 |
| 1.4.3 | 实现分派表间接调用备选方案 | `JitBackend.aura` | 1 天 |
| 1.4.4 | 多函数 JIT 测试 | `tests/photon/jit_multifunc_test.aura` | 1 天 |

**核心逻辑**：

```aura
/// 批量编译结果
class BatchResult {
    var success: Boolean = false
    var errorMessage: String = ""
    var codeBase: Int = 0
    var codeSize: Int = 0
    var funcOffsets: String = ""  // "name|offset;;name|offset"
    var funcCount: Int = 0
    var totalRelocCount: Int = 0
}

/// 批量编译多个函数
fun compileBatch(funcList: List<FuncEntry>): BatchResult {
    val result = BatchResult()
    val results: List<BackendResult> = ArrayList<BackendResult>()
    
    // 1. 逐个编译
    var i = 0
    while (i < funcList.size) {
        val func = funcList[i]
        val r = this.pipeline.compileEncodeOnly(func.hir, this.outDir, func.name)
        results.add(r)
        i = i + 1
    }
    
    // 2. 计算偏移表
    var totalSize = 0
    var offsets = ""
    i = 0
    while (i < results.size) {
        val r = results[i]
        val offset = totalSize
        offsets = offsets + funcList[i].name + "|" + offset + ";;"
        totalSize = totalSize + r.encodedByteCount
        // 16 字节对齐
        val rem = totalSize % 16
        if (rem != 0) { totalSize = totalSize + (16 - rem) }
        i = i + 1
    }
    result.funcOffsets = offsets
    
    // 3. 分配内存（一次性）
    val base = this.allocateExecMemory(this.alignToPage(totalSize))
    if (base == 0) {
        result.errorMessage = "内存分配失败"
        return result
    }
    
    // 4. 写入所有函数
    i = 0
    while (i < results.size) {
        val r = results[i]
        val offset = parseOffset(offsets, funcList[i].name)
        this.writeCode(base, offset, r.machineCodeHex)
        i = i + 1
    }
    
    // 5. 修正所有重定位（函数间交叉引用）
    i = 0
    while (i < results.size) {
        this.applyRelocations(base + parseOffset(offsets, funcList[i].name),
                              results[i].encodedByteCount,
                              results[i].relocRecords,
                              0,  // dataBase 待 Phase 2
                              offsets)
        i = i + 1
    }
    
    // 6. 切换为 RX
    this.setExecutable(base, this.codeSize)
    
    // 7. 注册分派表
    i = 0
    while (i < results.size) {
        val name = funcList[i].name
        val offset = parseOffset(offsets, name)
        this.registerFunction(name, base + offset)
        i = i + 1
    }
    
    result.success = true
    result.codeBase = base
    result.codeSize = totalSize
    result.funcCount = results.size
    return result
}
```

#### Phase 1.5：集成测试（2 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 1.5.1 | 单函数 JIT 测试（Hello World） | `tests/photon/jit_single_test.aura` | 1 天 |
| 1.5.2 | 多函数 JIT 测试（函数调用） | `tests/photon/jit_multifunc_test.aura` | 1 天 |

**验收标准**：
- 单函数：`JitBackend.compileMachineCode` → `emitFunction` → `executeNative` 返回正确结果
- 多函数：函数 A 调用函数 B，两者都 JIT 编译，`executeNative` 返回正确结果
- 重定位修正后，call 指令跳转到正确地址

---

### Phase 2：P1 安全运行（2 周）

**目标**：JIT 代码能安全使用 GC 管理的对象（字符串、数组、对象）。

#### Phase 2.1：数据段处理（2 天）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 2.1.1 | `emitFunction` 增加数据段分配 | `JitBackend.aura` | 1 天 |
| 2.1.2 | 实现 `writeDataSection` 方法 | `JitBackend.aura` | 0.5 天 |
| 2.1.3 | 数据引用重定位修正 | `JitRelocation.aura` | 0.5 天 |

#### Phase 2.2：Stack maps（1.5 周）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 2.2.1 | 定义 stack map 数据结构 | `JitStackMap.aura`（新） | 2 天 |
| 2.2.2 | X86Emitter 记录 call 点寄存器状态 | `X86Emitter.aura` | 3 天 |
| 2.2.3 | 生成 stack map 表（call 点 → GC 根列表） | `X86Emitter.aura` | 3 天 |
| 2.2.4 | 将 stack map 附加到函数元数据 | `JitBackend.aura` | 2 天 |
| 2.2.5 | GC 扫描时读取 stack map | GC 模块修改 | 2 天 |

**Stack map 数据结构设计**：

```aura
/// Stack map 条目：每个 call 点 → GC 根寄存器列表
class StackMapEntry {
    var pcOffset: Int = 0           // call 指令在函数内的偏移
    var rootCount: Int = 0          // GC 根数量
    var rootRegs: String = ""       // 逗号分隔的寄存器名（如 "rbx,rdi,rbp"）
    var rootSpillSlots: String = "" // 逗号分隔的栈槽偏移（如 "8,16,24"）
}

/// 函数 Stack map
class FunctionStackMap {
    var funcName: String = ""
    var entries: List<StackMapEntry> = ArrayList<StackMapEntry>()
    var totalSize: Int = 0          // stack map 总字节数
}

/// Stack map 表
class StackMapTable {
    var maps: List<FunctionStackMap> = ArrayList<FunctionStackMap>()
    
    /// 查找 PC 对应的 stack map 条目
    fun lookup(funcName: String, pcOffset: Int): StackMapEntry { ... }
}
```

**GC 集成**：
```text
GC 扫描流程：
1. 遍历所有 JIT 函数
2. 对每个函数，获取当前 PC（从栈帧中读取）
3. 用 PC 查找 stack map 条目
4. 标记条目中列出的寄存器/栈槽中的 GC 根
```

#### Phase 2.3：安全集成测试（1 天）

| # | 任务 | 工作量 |
|---|------|--------|
| 2.3.1 | JIT 编译含字符串操作的程序 | 0.5 天 |
| 2.3.2 | 触发 GC 后验证无崩溃/无 use-after-free | 0.5 天 |

---

### Phase 3：P2 优化能力（3 周）

**目标**：JIT 代码能做投机优化，性能接近 Cranelift。

#### Phase 3.1：Guards（1.5 周）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 3.1.1 | 定义 guard 节点类型 | `MachineDag.aura` | 1 天 |
| 3.1.2 | InstructionSelection 生成 guard 节点 | `InstructionSelection.aura` | 3 天 |
| 3.1.3 | X86Encoder 发射 guard 指令（cmp + jne） | `X86Encoder.aura` | 2 天 |
| 3.1.4 | X86Emitter 处理 guard 发射 | `X86Emitter.aura` | 2 天 |
| 3.1.5 | Guard 单元测试 | `tests/photon/jit_guard_test.aura` | 2 天 |

**Guard 指令设计**：
```asm
; 类型检查 guard
cmp [rax + type_offset], expected_type    ; 检查对象类型
jne deopt_trampoline                      ; 类型不匹配 → 回退解释器

; 值检查 guard
cmp rdx, max_value                        ; 检查值范围
ja deopt_trampoline                       ; 超出范围 → 回退
```

#### Phase 3.2：Deoptimization（1.5 周）

| # | 任务 | 文件 | 工作量 |
|---|------|------|--------|
| 3.2.1 | 设计 JIT 帧 → VM 帧栈帧映射 | 设计文档 | 1 天 |
| 3.2.2 | 实现 `buildDeoptTrampoline`（真实地址修正） | `JitBackend.aura` | 2 天 |
| 3.2.3 | 实现栈帧转换逻辑（保存 JIT 寄存器 → 恢复 VM 栈） | `JitDeopt.aura`（新） | 3 天 |
| 3.2.4 | 与 guard 指令集成（guard 失败 → deopt） | `X86Emitter.aura` | 2 天 |
| 3.2.5 | Deopt 单元测试 | `tests/photon/jit_deopt_test.aura` | 2 天 |

**栈帧映射设计**：
```text
JIT 帧布局：
  [rsp]  = saved rbp
  [rsp+8] = return address
  [rsp+16] = local_0
  [rsp+24] = local_1
  ...

VM 帧布局：
  [frame.ip]  = 下一条指令
  [frame.sp]  = 栈顶
  [frame.locals[0]] = local_0
  [frame.locals[1]] = local_1
  ...

Deopt 转换：
1. 保存 JIT 寄存器状态到栈
2. 构建 VM 帧（ip = 原始字节码 IP, sp = 栈状态, locals = JIT 局部变量）
3. 跳转到 VM 解释器入口
```

---

### Phase 4：P3 高级特性（3 周）

**目标**：极致优化，性能最大化。

#### Phase 4.1：OSR（1.5 周）

| # | 任务 | 工作量 |
|---|------|--------|
| 4.1.1 | 循环计数器插入（编译时） | 3 天 |
| 4.1.2 | OSR 触发检测（运行时） | 2 天 |
| 4.1.3 | 循环变量栈帧映射 | 2 天 |
| 4.1.4 | OSR 单元测试 | 2 天 |

#### Phase 4.2：类型反馈（1 周）

| # | 任务 | 工作量 |
|---|------|--------|
| 4.2.1 | 解释器中收集类型反馈（IC/BC） | 2 天 |
| 4.2.2 | 编译时读取反馈，生成特化代码 | 3 天 |
| 4.2.3 | 反馈失效检测 + 回退 | 2 天 |

#### Phase 4.3：编译速度优化（2 天）

| # | 任务 | 工作量 |
|---|------|--------|
| 4.3.1 | optLevel=0 快速编译（跳过 Peephole 等） | 1 天 |
| 4.3.2 | 编译延迟基准测试 | 1 天 |

---

### 完整工期汇总

| 阶段 | 优先级 | 工期 | 人力 |
|------|--------|------|------|
| Phase 0：基础验证 | — | 0.5 周 | 1 人 |
| Phase 1.1：重定位收集 | P0 | 1 天 | 1 人 |
| Phase 1.2：重定位修正 | P0 | 4 天 | 1 人 |
| Phase 1.3：存根地址修正 | P0 | 2 天 | 1 人 |
| Phase 1.4：多函数编译时链接 | P0 | 5 天 | 1 人 |
| Phase 1.5：集成测试 | P0 | 2 天 | 1 人 |
| Phase 2.1：数据段处理 | P1 | 2 天 | 1 人 |
| Phase 2.2：Stack maps | P1 | 7 天 | 1 人 |
| Phase 2.3：安全集成测试 | P1 | 1 天 | 1 人 |
| Phase 3.1：Guards | P2 | 7 天 | 1 人 |
| Phase 3.2：Deoptimization | P2 | 7 天 | 1 人 |
| Phase 4.1：OSR | P3 | 7 天 | 1 人 |
| Phase 4.2：类型反馈 | P3 | 5 天 | 1 人 |
| Phase 4.3：编译速度优化 | P3 | 2 天 | 1 人 |
| **总计** | | **56 天 ≈ 11.2 周** | **1 人** |

---

## 五、文件变更清单

### 5.1 新增文件

| 文件 | 用途 | 阶段 |
|------|------|------|
| `jit/JitRelocation.aura` | 重定位数据结构 + 修正逻辑 | Phase 1 |
| `jit/JitStackMap.aura` | Stack map 数据结构 | Phase 2 |
| `jit/JitDeopt.aura` | Deoptimization 栈帧转换 | Phase 3 |
| `tests/photon/jit_smoke_test.aura` | 基础 JIT 测试 | Phase 0 |
| `tests/photon/jit_single_test.aura` | 单函数 JIT 测试 | Phase 1 |
| `tests/photon/jit_multifunc_test.aura` | 多函数 JIT 测试 | Phase 1 |
| `tests/photon/jit_reloc_test.aura` | 重定位单元测试 | Phase 1 |
| `tests/photon/jit_guard_test.aura` | Guard 测试 | Phase 3 |
| `tests/photon/jit_deopt_test.aura` | Deopt 测试 | Phase 3 |
| `tests/photon/jit_osr_test.aura` | OSR 测试 | Phase 4 |

### 5.2 修改文件

| 文件 | 修改内容 | 阶段 |
|------|----------|------|
| `PhotonPipeline.aura` | `BackendResult` 增加字段；`compileEncodeOnly` 收集重定位 | Phase 1 |
| `JitBackend.aura` | `emitFunction` 增加重定位参数；新增 `compileBatch`/`applyRelocations`/`patchStub` | Phase 1 |
| `X86Emitter.aura` | 记录 call 点寄存器状态（stack maps） | Phase 2 |
| `X86Encoder.aura` | 发射 guard 指令 | Phase 3 |
| `MachineDag.aura` | 新增 guard 节点类型 | Phase 3 |
| `InstructionSelection.aura` | 生成 guard 节点 | Phase 3 |

---

## 六、风险与缓解

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| `Memory`/`JitExec` extern 在原生运行时不可用 | 低 | 高 | Phase 0 先验证；备用方案：C 运行时 + FFI |
| 重定位修正逻辑有 bug | 中 | 高 | 每个重定位类型独立单元测试；与 AOT 路径对比验证 |
| Stack maps 设计过于复杂 | 中 | 中 | 先实现简单版本（只标记调用点寄存器），后续优化 |
| Deoptimization 栈帧映射错误 | 中 | 高 | 与 VM 帧布局严格对比；差分测试 |
| 编译速度不达预期（>10ms） | 中 | 中 | optLevel=0 快速编译；后台线程编译 |
| GC 与 JIT 交互崩溃 | 低 | 高 | 充分集成测试；回退到保守代码生成 |
| Photon 代码量增长过快 | 低 | 中 | 严格分阶段；每阶段独立可测试可回退 |

---

## 七、决策记录

### D1：运行时修正 vs 编译时链接

- **决策**：Phase 1 先实现运行时修正（简单），Phase 1.4 实现编译时链接（快速）
- **原因**：运行时修正更灵活（支持动态编译），编译时链接更快（直接 call）
- **影响**：初始版本稍慢，后续可优化

### D2：Stack maps 在 Phase 2 而非 Phase 1

- **决策**：Phase 1 先不实现 stack maps，Phase 2 再补
- **原因**：Phase 1 的目标是"JIT 能工作"，stack maps 是"JIT 安全"
- **风险**：Phase 1 的 JIT 代码不能安全使用 GC 对象
- **缓解**：Phase 1 仅测试整数/算术操作（不涉及 GC）

### D3：Guards + Deopt 在 Phase 3

- **决策**：Guards 和 Deoptimization 在 Phase 3 实现
- **原因**：这些是优化特性，不是基础功能
- **影响**：Phase 1-2 的 JIT 只能生成保守代码（无投机优化）

### D4：OSR 和类型反馈在 Phase 4

- **决策**：OSR 和类型反馈作为高级特性，在 Phase 4 实现
- **原因**：这些是极致优化，需要前面所有基础设施就绪
- **影响**：Phase 1-3 的 JIT 已可达到基础性能水平

---

## 八、附录

### A. 重定位格式参考

X86Emitter 的 `getRelocRecords()` 返回格式：
```text
"offset|symbol|type;;offset|symbol|type;;..."
```

示例：
```text
"0|main|RIP32;;56|helper|ABS64;;120|.str_hello|DATA_REF"
```

### B. 函数偏移表格式

X86Emitter 的 `getFuncOffsets()` 返回格式：
```text
"funcName|offset;;funcName|offset"
```

示例：
```text
"main|0;;helper|64;;util_1|128"
```

### C. 重定位类型对照表

| 类型 | 值 | 场景 | 指令示例 | 修正公式 |
|------|---|------|----------|----------|
| RIP32 | 0 | call/jmp 相对偏移 | `E8 xx xx xx xx` | `target - (base+offset+4)` |
| ABS64 | 1 | 绝对地址立即数 | `48 B8 xx xx xx xx xx xx xx xx` | `target` |
| DATA_REF | 2 | 数据引用 | `48 8D 05 xx xx xx xx` | `dataBase + symbolOffset` |
| PCREL32 | 3 | PC 相对加载 | `48 8D 05 xx xx xx xx` | `target - (base+offset+8)` |

### D. Stack map 格式示例

```text
函数: fibonacci
  PC offset 0:   入口（无 call）
  PC offset 12:  call → guard: rbx=gc_root, rsi=gc_root, [rbp-8]=gc_root
  PC offset 24:  call → guard: rbx=gc_root, rsi=gc_root, [rbp-16]=gc_root
  PC offset 36:  call → guard: 无 GC 根
```
