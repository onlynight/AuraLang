# 顶层变量/方法与 VM/Vtable 生命周期分析（聚焦 Aura 代码）

> 仅分析，不修改代码。
> 分析对象：当前工作区中所有 Aura 侧 VM 运行时、GC、JIT、并发原语代码。

---

## 1. 全局状态盘点：object 单例 = 进程全局

### 1.1 `internal object`（单例，所有 VM 实例共享）

| 文件 | 符号 | 内含可变字段 | 问题 |
|------|------|-------------|------|
| `compiler/vm/Vm.aura` | `object Vm` | 无字段，但 `create()` 返回 Map | 🟢 |
| `compiler/vm/Frames.aura` | `object Frames` | 无字段，`new()` 返回 Map | 🟢 |
| `compiler/vm/Opcodes.aura` | `object Opcodes` | 无字段，全 const | 🟢 |
| `compiler/gc/Gc.aura` | `object Gc` | 无字段，`create()` 返回 Map | 🟢 |
| `compiler/gc/MarkSweep.aura` | `object MarkSweep` | 无字段 | 🟢 |
| `compiler/gc/Concurrent.aura` | `object Concurrent` | 无字段 | 🟢 |
| `compiler/gc/Incremental.aura` | `object Incremental` | 无字段 | 🟢 |
| `compiler/memory/Arc.aura` | `object Arc` | 无字段 | 🟢 |
| `compiler/memory/Memory.aura` | `object Memory` | 无字段 | 🟢 |
| `compiler/memory/MemoryPool.aura` | `object MemoryPool` | 无字段 | 🟢 |
| `compiler/runtime/GcTrigger.aura` | `object GcTrigger` | 无字段 | 🟢 |
| `compiler/runtime/Coroutine.aura` | `object Coroutine` | 无字段 | 🟢 |
| **`core/coroutine/Coroutine.aura`** | **`object Coroutine`** | **`nextId: Int`, `coroutines: List<Map>`** | 🔴 **全局可变** |
| `core/std/Atomic.aura` | `object Atomic` | 无字段，仅签名 | 🟡 签名委托全局 Rust 注册表 |
| `core/std/Mutex.aura` | `object Mutex` | 无字段，仅签名 | 🟡 同上 |
| `core/std/Thread.aura` | `object Thread` | 无字段，仅签名 | 🟡 同上 |
| `core/std/Future.aura` | `object Future` | 无字段，仅签名 | 🟡 同上 |
| `core/std/Condvar.aura` | `object Condvar` | 无字段，仅签名 | 🟡 同上 |
| `core/std/RwLock.aura` | `object RwLock` | 无字段，仅签名 | 🟡 同上 |
| `core/std/Barrier.aura` | `object Barrier` | 无字段，仅签名 | 🟡 同上 |
| `compiler/vm/VmJitBridge.aura` | `object VmJitConfig` | 无字段，全 const | 🟢 |

### 1.2 关键问题：`core/coroutine/Coroutine.aura`

```aurora
internal object Coroutine {
    private var nextId: Int = 1                          // ← 全局递增
    private var coroutines: List<Map<String, Any>> = mutableListOf()  // ← 全局列表

    fun spawn(body: () -> Any): Int {
        val id = nextId
        nextId = nextId + 1
        val coroutineData = mutableListOf<String, Any>()
        coroutineData.add("body", body)      // ← 捕获闭包存入全局
        coroutineData.add("status", "waiting")
        coroutineData.add("result", null)
        coroutines.add(coroutineData)          // ← 永不清理
        return id
    }

    fun ask(co_id: Int): Any {
        if (co_id < 0 || co_id >= coroutines.size) return null
        val coroutine = coroutines[co_id]
        if (coroutine["status"] == "done") {
            return coroutine["result"]
        }
        val body = coroutine["body"] as () -> Any
        val result = body()                   // ← 立即执行，不是真正协程
        coroutine["result"] = result
        coroutine["status"] = "done"
        return result
    }
}
```

**四个致命问题**：

1. **全局可变状态**：`nextId` 和 `coroutines` 是所有 VM 实例共享的进程全局。多 VM 并发 spawn 协程时，ID 冲突、列表互相污染。

2. **列表永不清理**：`coroutines` 只增不减。即使 `ask` 执行完毕，条目仍占用内存，闭包 `body` 的捕获引用也无法释放。

3. **`ask` 是同步执行**：`spawn` + `ask` 实际上是同步调用——`ask` 内联执行 body 后才返回。这不是协程，是同步函数调用加了一层包装。真正的协程需要让出执行权。

4. **无生命周期管理**：协程 ID 是连续整数，但 `coroutines[co_id]` 用 ID 直接索引列表。如果 VM A 的协程 1 和 VM B 的协程 1 同时存在，会互相覆盖。

### 1.3 std 并发原语：签名委托全局 Rust 注册表

```aurora
internal object Atomic {
    fun new(initial: Int): Int     // 返回 ID
    fun load(atomic_id: Int): Int  // 按 ID 查找
    ...
}
```

这些 `object` 本身无可变字段（是纯签名），但 `new()` 返回的 `Int` 索引的是 **Rust 侧的进程全局注册表**（`RAW_MUTEX_REGISTRY`, `ATOMIC_REGISTRY`, 等）。Aura 侧无法控制这些注册表的生命周期——当 VM 实例被销毁时，这些句柄变成悬空索引。

---

## 2. VmRunner：字符串即一切 —— 灾难性性能

### 2.1 全部状态都是字符串

```aurora
class VmRunner {
    var bytecode: String = ""       // 每行一条指令
    var constPool: String = ""      // 每行一个常量
    var stack: String = ""          // 每行一个栈值
    var stackDepth: Int = 0         // 手动跟踪
    var locals: String = ""         // 每行 slotIdx|value
    var globals: String = ""        // 每行 name|value
    var funcTable: String = ""      // 每行 name|offset|params
    var callStack: String = ""      // 每行 returnIp|frameId
    var savedLocals: String = ""    // \u0001 分隔的局部变量块
}
```

**没有任何一个状态使用 `ArrayList`、`HashMap`、`List` 或数组。** 全部用字符串 + 线性扫描。

### 2.2 栈操作 O(n²)

```aurora
fun push(value: String): Unit {
    this.stack = this.stack + value + "\n"   // ← 每次复制整个栈
}

fun pop(): String {
    val value: String = this.fieldAtRow(this.stack, this.stackDepth - 1, "\n")
    var pos: Int = this.stack.length - 1
    if (pos >= 0 && toStr(this.stack[pos]) == "\n") { pos = pos - 1 }
    while (pos >= 0 && toStr(this.stack[pos]) != "\n") { pos = pos - 1 }  // ← 线性扫描
    this.stack = this.stackSlice(this.stack, 0, pos + 1)  // ← 又复制一次
    this.stackDepth = this.stackDepth - 1
    return value
}
```

- `push`：O(n) —— 字符串拼接复制整个栈。
- `pop`：O(n) —— 扫描找 `\n`，然后 substring 复制。
- n 次栈操作的总成本：**O(n²)**。
- 1000 次栈操作 ≈ 500 万次字符复制。

### 2.3 局部变量操作 O(n) per lookup

```aurora
fun getLocal(slotIdx: Int): String {
    val key: String = toStr(slotIdx)
    var pos: Int = 0
    var len: Int = this.locals.length
    var lineStart: Int = 0
    while (pos < len) {
        if (toStr(this.locals[pos]) == "\n") {
            val line: String = this.stackSlice(this.locals, lineStart, pos - lineStart)
            if (this.fieldAtCol(line, 0, "|") == key) {   // ← 每行都 fieldAtCol
                return this.fieldAtCol(line, 1, "|")
            }
            lineStart = pos + 1
        }
        pos = pos + 1
    }
    return "0"
}

fun setLocal(slotIdx: Int, value: String): Unit {
    this.locals = this.localsRemoveSlot(slotIdx)  // ← 重建整个 locals 字符串
    this.locals = this.locals + toStr(slotIdx) + "|" + value + "\n"  // ← 拼接
}
```

- `getLocal`：O(n) —— 扫描所有行找 slotIdx。
- `setLocal`：O(n) —— `localsRemoveSlot` 重建 + 拼接。
- `localsRemoveSlot`：O(n) —— 扫描 + 拼接，字符级复制。

### 2.4 stackSlice 字符级复制

```aurora
fun stackSlice(text: String, start: Int, length: Int): String {
    var result: String = ""
    var end: Int = start + length
    if (end > text.length) { end = text.length }
    var i: Int = start
    while (i < end) {
        result = result + toStr(text[i])  // ← 每个字符一次字符串拼接
        i = i + 1
    }
    return result
}
```

- 每个字符一次 `+` 拼接。
- 调用一次 `stackSlice` 的 O(n) 字符串分配，n = length。
- `pop` 调用 1 次 `stackSlice`，`getLocal` 调用 ~2 次/行。
- 总复杂度：**O(n² × m)**，其中 n = 栈深度/变量数，m = 操作次数。

### 2.5 dispatch 全量 if 检查

```aurora
fun dispatch(opcode: String, arg1: String, arg2: String, arg3: String): Unit {
    if (opcode == "CONST_INT") { ... }      // ← 独立 if，不是 else if
    if (opcode == "CONST_FLOAT") { ... }
    if (opcode == "CONST_STRING") { ... }
    // ... 30 个独立 if 语句
}
```

- 每条指令检查 30 次字符串比较。
- 不是 `else if` 链，所以即使匹配了 `CONST_INT`，后续 29 个 `if` 仍会执行。
- 应该用 `when` 表达式或 `else if` 链。

### 2.6 vrToInt O(n×10)

```aurora
fun vrToInt(text: String): Int {
    var total: Int = 0
    var i: Int = 0
    var len: Int = text.length
    var neg: Boolean = false
    if (len > 0 && toStr(text[0]) == "-") { neg = true; i = 1 }
    while (i < len) {
        val c: String = toStr(text[i])
        var digit: Int = -1
        var j: Int = 0
        while (j < 10) {
            if (toStr(j) == c) {    // ← toStr(j) 每次调用都分配
                digit = j
                break
            }
            j = j + 1
        }
        if (digit >= 0) { total = total * 10 + digit }
        i = i + 1
    }
    if (neg) { return -total }
    return total
}
```

- 每个字符调用 `toStr(text[i])`（分配）和 10 次 `toStr(j)`（分配）。
- 解析 "12345" = 5 × (1 + 10) = 55 次字符串分配。
- 被 `LOAD_LOCAL`、`STORE_LOCAL`、算术运算、跳转等所有指令调用。
- 应该用 `parseInt()` 内置函数（如果 VM 支持）或预计算查表。

---

## 3. FrameManager：同样的字符串问题

```aurora
class FrameManager {
    var frameChain: String = ""      // 每行 frameId|funcName|ip|arity|locals|upvalues
    var stackMemory: String = ""     // 每行 slotIdx|value
}
```

- `allocateFrame`：拼接一行到 `frameChain` → O(n)。
- `deallocateFrame`：只减 `callDepth`，**不释放任何内存**（注释："简化：不实际回收内存"）。
- `updateFrameField`：扫描全部行，重建每一行，拼接成新字符串 → O(n)。
- `rebuildLine`：字符级重建行 → O(m)，m = 行长度。

`deallocateFrame` 的"不回收"意味着帧只增不减，内存随调用深度线性增长。

---

## 4. GC 系统：全部是空壳

### 4.1 Gc.aura

```aurora
internal object Gc {
    fun malloc(size: Int): Any { return null }        // ← 分配返回 null
    fun free(ptr: Any): Unit { return }               // ← 释放什么都不做
    fun realloc(ptr: Any, size: Int): Any { return ptr }
    fun mark(state: Map<String, Any>): Boolean {
        state["status"] = GC_MARKING
        return true
    }                                                  // ← 只改状态，不标记
    fun sweep(state: Map<String, Any>): Int {
        state["status"] = GC_SWEEPING
        val collected = 0                              // ← 永远收集 0 个
        state["collectedCount"] = ...
        state["status"] = GC_IDLE
        return collected
    }
    fun shouldCollect(state: Map<String, Any>): Boolean {
        return (state["status"] as Int) == GC_IDLE     // ← 永远是 IDLE，永远返回 true
    }
}
```

- **`malloc` 返回 null**：分配永远失败。
- **`free` 什么都不做**：内存永不释放。
- **`mark` 不标记任何对象**：只改状态字段。
- **`sweep` 返回 0**：永远不回收任何东西。
- **`shouldCollect` 永远返回 true**：状态永远是 IDLE。

### 4.2 MarkSweep.aura

```aurora
fun markAll(roots: List<Integer>): Int {
    var marked = 0
    for (root in roots) {
        if (root != null) { marked = marked + 1 }   // ← 只计数非 null 根，不实际标记
    }
    return marked
}

fun sweepAll(heap: List<Any>, marked: List<Integer>): Int {
    var freed = 0
    for (i in 0 until heap.size) {
        if (!marked.contains(i)) { freed = freed + 1 }  // ← 只计数，不回收
    }
    return freed
}
```

- `markAll` 只统计非 null 根数量，不遍历对象图。
- `sweepAll` 只统计未标记对象数量，不释放内存。
- `collect` 只是标记→清除的串联，但两者都是空操作。

### 4.3 Concurrent / Incremental / GcTrigger

- `Concurrent.start/stop`：只改 `running` 布尔值。
- `Incremental.step`：只递增 `progress`。
- `GcTrigger.allocated/freed`：只加减 `current`。
- 全部是状态计数器，没有任何实际 GC 行为。

### 4.4 Memory / MemoryPool / Arc

- `Memory.allocate` → 返回 null。
- `Memory.free` → 空操作。
- `MemoryPool.allocate` → 返回 null。
- `Arc.wrap` → 创建 Map 存 `value` 和 `count`。
- `Arc.retain/release` → 改 `count` 字段。
- **但没有任何对象引用 Arc**：`Arc` 的 Map 不连接到 VM 的对象图。`Arc` 的引用计数从未被 VM 的实际对象操作触发。

### 4.5 后果

- **零内存回收**：VM 运行期间内存只增不减。
- **无对象图遍历**：mark-sweep 的"标记"阶段不遍历任何指针。
- **ARC 不生效**：`IncRef`/`DecRef` 指令在 Rust VM 中有实现，但 Aura 侧的 `Arc` 从未被调用。
- **GcTrigger 无用**：阈值检查永远返回 true，但 `collect` 什么都不做。

---

## 5. JIT 系统：多层字符串状态 + 编译流程空壳

### 5.1 JitState：字符串记录表

```aurora
class JitState {
    var compiled: String = ""        // 行表：每行一个 func idx
    var skipped: String = ""         // 记录表：idx|reason
    var dispatch: String = ""        // 记录表：idx|entryToken
    var callCounts: String = ""      // 记录表：idx|count
    var threshold: Int = 10000
    var capacity: Int = 0
}
```

- `isCompiled(idx)`：`jitLineHas` → 线性扫描 O(n)。
- `callCount(idx)`：`jitRecordGet` → 线性扫描 O(n)。
- `incCallCount(idx)`：`jitRecordSet` → 线性扫描 + 重建字符串 O(n)。
- 热点阈值 10000 次调用 → 10000 × O(n) 扫描 = O(10000n)。
- 如果已编译 100 个函数（n=100），每次调用计数 = 10000 × 100 = 100 万次扫描。

### 5.2 JitCoreCompiler：字符串缓存

```aurora
class JitCoreCompiler {
    var compiledFuncs: String = ""     // 行表
    var unitCodeMap: String = ""       // 记录表：func|decoded_instr
    var unitLocalsMap: String = ""     // 记录表：func|locals
    var decodeFailed: Boolean = false
}
```

- `isCompiled(func)`：`jitLineHas` → O(n)。
- `unitCodeOf(func)`：扫描 `unitCodeMap` 的所有行，过滤出匹配的 func → O(n)。
- `unitLocalsOf(func)`：`jitRecordGet` → O(n)。
- `invalidate(func)`：重建 `compiledFuncs` 字符串 → O(n)。

### 5.3 JitCoreVm：字符串栈 + 字符串 locals

```aurora
class JitCoreVm {
    var locals: String = ""   // 记录表：slot|value
    var stack: String = ""    // 行表
    var ip: Int = 0
    var halt: Boolean = false
    var result: String = ""
    var error: String = ""
}

fun push(value: String): Unit {
    this.stack = this.stack + value + "\n"   // ← 同 VmRunner，O(n)
}

fun pop(): String {
    val n: Int = jitLineCount(this.stack)    // ← 线性扫描计数
    if (n == 0) { ... return "0" }
    val value: String = jitLineAt(this.stack, n - 1)  // ← 线性扫描到第 n-1 行
    var out: String = ""
    var i: Int = 0
    while (i < n - 1) {
        out = out + jitLineAt(this.stack, i) + "\n"  // ← 重建栈
    }
    this.stack = out
    return value
}
```

- 和 VmRunner 一样，`push` O(n)，`pop` O(n)。
- `jitLineCount` 每次扫描整个字符串计数行数。
- `jitLineAt` 每次扫描到指定行。
- `pop` 重建整个栈。

### 5.4 VmJitBridge：FFI 编译流程是空壳

```aurora
fun generateClifText(code: String): String {
    if (code == "") { return "" }
    return ";; Clif IR for func\n" + code    // ← 直接把字节码当 Clif IR
}

fun registerDispatch(bridge: VmJitBridge, idx: Int, entry_token: String) {
    // 通过 @native 调用 bootstrap 的 register_dispatch
    // 这里简化处理：直接返回
    // 实际实现需要 @native fun register_dispatch(idx: Int, token: String): Unit
}

fun lookupDispatchToken(idx: Int): String {
    // 通过 @native 调用 bootstrap 的 lookup_dispatch
    // 这里简化处理：直接返回空串
    // 实际实现需要 @native fun lookup_dispatch(idx: Int): String
    return ""                                // ← 永远返回空串
}
```

- `generateClifText` 直接把字节码文本当 Cranelift IR——这是无效的 IR，`jit_compile` 必定失败。
- `registerDispatch` 是空函数——JIT 编译的函数永远无法被 `jit_call` 找到。
- `lookupDispatchToken` 返回空串——`nativeCall` 永远走 fallback。
- **整个 JIT 编译路径是死代码**：编译必定失败 → 跳过 → 回退解释器。

### 5.5 JitRuntime：段格式校验（唯一有效的部分）

`JitRuntime.aura` 实现了 `.auc` 段格式校验、描述符计数、分发表重建等纯计算逻辑。这部分是字符串上的确定性操作，逻辑正确但性能同样受字符串扫描拖累。

---

## 6. Coroutine（compiler/runtime）：空壳

```aurora
internal object Coroutine {
    fun create(): Map<String, Any> {
        val co = Collections.mutableMapOf()
        co["id"] = 0
        co["state"] = "created"
        co["stack"] = Collections.emptyList()
        return co
    }
    fun start(co: Map<String, Any>): Boolean {
        co["state"] = "running"
        return true
    }
    fun yield(co: Map<String, Any>): Unit {
        co["state"] = "suspended"
    }
    fun isRunning(co: Map<String, Any>): Boolean {
        return co["state"] == "running"
    }
}
```

- 只改状态字符串。
- 不保存/恢复调用栈。
- 不调度协程。
- 不执行任何用户代码。
- `stack` 字段是 `emptyList()`，从未被使用。

---

## 7. TestRunner：唯一的正确范式

```aurora
class TestRunner {
    var passed: Int = 0
    var failed: Int = 0
    var total: Int = 0
    var quiet: Boolean = false
    var suite: String = ""
}
```

- 使用 `class`（实例）而非 `object`（单例）—— **状态挂在实例上，每个测试文件独立**。
- 文件头注释明确说明："VM 中 `object` 可变字段 + 带参方法存在已知缺陷，因此框架状态必须挂在实例上，而非单例上"。
- 这是整个代码库中**唯一正确使用了实例状态**的文件。
- 但断言用 `toStr(a) == toStr(b)` 比较——如果 `toStr` 对复杂对象返回不确定的字符串，断言可能误判。

---

## 8. 内存影响总结

| 问题 | 根因 | 影响 |
|------|------|------|
| GC 全空壳 | `malloc`→null, `free`→noop, `sweep`→0 | 零内存回收，进程内存单调增长 |
| ARC 不生效 | `Arc.wrap` 不连接到对象图 | 引用计数指令空转 |
| GcTrigger 无用 | `shouldCollect` 永远 true 但 `collect` 空操作 | GC 触发无效果 |
| 协程列表不回收 | `coroutines.add()` 永不清理 | 全局泄漏，闭包引用永驻 |
| 帧不释放 | `deallocateFrame` 只减计数，不回收 | 内存随调用深度线性增长 |
| 字符串复制 | `push`/`pop`/`setLocal` 全用 `+` 拼接 | 每次操作 O(n) 分配，旧字符串成为垃圾（但 GC 不回收） |

**净效果**：每个 VM 实例运行期间，内存 = 字符串操作产生的临时字符串 + 累积的协程/帧/局部变量表。**GC 不回收任何一字节**。

---

## 9. 性能影响总结

### 9.1 时间复杂度对比

| 操作 | 当前（字符串） | 最优（ArrayList/HashMap） | 差距 |
|------|---------------|--------------------------|------|
| 栈 push/pop | O(n) | O(1) | n× |
| 局部变量 get/set | O(n) | O(1) | n× |
| 全局变量 get | O(n) | O(1) | n× |
| 函数表查找 | O(n) | O(1) | n× |
| 帧字段更新 | O(n) | O(1) | n× |
| 调用计数（JIT） | O(n) | O(1) | n× |
| 指令分派 | O(m) m=指令数 | O(1)（when） | m× |
| 整数字符串解析 | O(len×10) | O(len) | 10× |
| JIT 热点检测 | O(10000×n) | O(1) | 10000n× |

### 9.2 量化估算

假设 VM 执行 10000 条指令，平均栈深度 16，局部变量 32 个：

- **栈操作**：10000 × 16 = 160,000 字符复制/操作
- **局部变量**：10000 × 32 = 320,000 字符扫描/操作
- **指令分派**：10000 × 30 = 300,000 次字符串比较
- **整数解析**：10000 × 5 × 11 = 550,000 次字符串分配
- **总计**：~130 万次字符串操作，每操作 O(n) → **130 万次 × 平均长度 ~100 字符 = 1.3 亿次字符操作**

如果使用 `ArrayList<String>` 栈 + `HashMap<Int, String>` 局部变量 + `when` 分派 + `parseInt`：
- **总计**：10000 × 4 = 40,000 次 O(1) 操作

**差距约 3000×**。

---

## 10. 安全性影响

### 10.1 `core/Coroutine` 全局状态跨 VM 污染

如果两个 VM 实例在同一进程中运行：

```
VM-A: Coroutine.spawn(bodyA) → id=1, coroutines=[{bodyA}]
VM-B: Coroutine.spawn(bodyB) → id=2, coroutines=[{bodyA}, {bodyB}]
VM-A: Coroutine.ask(1) → 返回 bodyA 结果（正确，但 bodyB 也残留）
VM-A drop → bodyA 闭包仍在全局 coroutines 中（泄漏）
```

### 10.2 std 原语句柄跨 VM 混淆

```
VM-A: val m1 = Mutex.new() → Rust 全局 RAW_MUTEX_REGISTRY[0]
VM-B: val m2 = Mutex.new() → Rust 全局 RAW_MUTEX_REGISTRY[1]
VM-A drop → m1=0 的 RawMutex 仍在 RAW_MUTEX_REGISTRY 中
VM-B: Mutex.new() → RAW_MUTEX_REGISTRY[0]（复用槽位）
// 如果 VM-A 的某个残留闭包调用 Mutex.lock(0)，会锁住 VM-B 的锁
```

### 10.3 JIT 分发表空表导致死代码

`VmJitBridge.nativeCall` → `lookupDispatchToken` → 返回 "" → 永远 fallback。
这意味着 JIT 编译路径 **完全不可用**，所有函数永远走解释器。
如果未来有人修改 `lookupDispatchToken` 返回非空字符串，但 `registerDispatch` 仍是空函数，则 JIT 会调用未注册的 entry_token → 未定义行为。

---

## 11. 最优写法方案

### 方案 A：全局状态 → 实例状态（object → class）

**规则**：所有含可变字段的组件必须用 `class`（实例），禁止用 `object`（单例）。

```aurora
// 错误 ❌
internal object Coroutine {
    private var nextId: Int = 1
    private var coroutines: List<Map<String, Any>> = mutableListOf()
}

// 正确 ✅
class Coroutine {
    var nextId: Int = 1
    var coroutines: ArrayList<Map<String, Any>> = ArrayList.of()
}
```

**收益**：
- 每个 VM 实例持有独立的协程状态。
- VM 销毁时，实例引用断开，GC 可回收。
- 无跨 VM 污染。

**适用**：`core/coroutine/Coroutine.aura`、`compiler/runtime/Coroutine.aura`、以及所有含可变字段的 `object`。

### 方案 B：字符串栈/locals/globals → 真实集合

```aurora
// 错误 ❌
class VmRunner {
    var stack: String = ""
    fun push(value: String) { this.stack = this.stack + value + "\n" }
    fun pop(): String { /* 线性扫描 + substring */ }
}

// 正确 ✅
class VmRunner {
    var stack: ArrayList<String> = ArrayList.of(64)
    var stackDepth: Int = 0

    fun push(value: String): Unit {
        this.stack.add(value)
        this.stackDepth = this.stack.size
    }

    fun pop(): String {
        if (this.stackDepth == 0) {
            this.error = "Stack underflow"
            this.running = false
            return ""
        }
        this.stackDepth = this.stackDepth - 1
        return this.stack.removeLast()  // O(1)
    }

    fun peek(): String {
        if (this.stackDepth == 0) return ""
        return this.stack.get(this.stackDepth - 1)
    }
}
```

**适用**：`VmRunner.aura` 的 `stack`、`locals`、`globals`、`callStack`、`savedLocals`、`FrameManager.aura` 的 `frameChain`、`stackMemory`、`JitCoreVm.aura` 的 `stack`/`locals`、`JitState.aura` 的 `compiled`/`skipped`/`dispatch`/`callCounts`。

### 方案 C：dispatch 改用 when 表达式

```aurora
// 错误 ❌ —— 30 个独立 if，每条指令 30 次比较
fun dispatch(opcode: String, arg1: String, arg2: String, arg3: String): Unit {
    if (opcode == "CONST_INT") { ... }
    if (opcode == "CONST_FLOAT") { ... }
    // ... 30 个
}

// 正确 ✅ —— when 短路，匹配后立即返回
fun dispatch(opcode: String, arg1: String, arg2: String, arg3: String): Unit {
    when (opcode) {
        "CONST_INT"    -> { this.push(this.getConst(arg1)) }
        "CONST_FLOAT"  -> { this.push(this.getConst(arg1)) }
        "CONST_STRING" -> { this.push(this.getConst(arg1)) }
        "LOAD_LOCAL"   -> { this.push(this.getLocal(vrToInt(arg1))) }
        // ...
        else -> { /* TODO */ }
    }
}
```

**收益**：从 O(m) 降到 O(1)（哈希）或 O(log m)（二分）。

### 方案 D：实现真正的 GC

**最小可行 GC**：

```aurora
class GcHeap {
    var objects: ArrayList<Map<String, Any>> = ArrayList.of(256)
    var freeList: ArrayList<Int> = ArrayList.of()
    var markBit: ArrayList<Boolean> = ArrayList.of()
    var rootSet: ArrayList<Int> = ArrayList.of()

    fun alloc(): Int {
        if (freeList.size > 0) {
            val idx = freeList.removeLast()
            objects[idx]["marked"] = false
            return idx
        }
        val idx = objects.size
        objects.add(Collections.mutableMapOf())
        return idx
    }

    fun addRoot(idx: Int): Unit { rootSet.add(idx) }
    fun removeRoot(idx: Int): Unit {
        var i = 0
        while (i < rootSet.size) {
            if (rootSet.get(i) == idx) { rootSet.removeAt(i); return }
            i = i + 1
        }
    }

    fun collect(): Int {
        // mark: 从 rootSet 遍历对象图
        for (idx in rootSet) {
            mark(idx)
        }
        // sweep: 回收未标记对象
        var freed = 0
        var i = 0
        while (i < objects.size) {
            if (!(objects.get(i)["marked"] as Boolean)) {
                freeList.add(i)
                freed = freed + 1
            }
            objects.get(i)["marked"] = false
            i = i + 1
        }
        return freed
    }

    private fun mark(idx: Int): Unit {
        if (idx < 0 || idx >= objects.size) return
        if (objects.get(idx)["marked"] as Boolean) return
        objects.get(idx)["marked"] = true
        // 遍历对象的所有字段值，如果是堆引用则递归标记
        val fields = objects.get(idx)
        for (key in fields.keys) {
            val val = fields[key]
            if (val is Map<String, Any>) {
                // val 本身是对象引用 → 需要提取其 idx
                // 这需要一个间接层：Map 的第一个字段存 idx
            }
        }
    }
}
```

**注意**：Aura VM 的 `Any` 类型不区分堆对象引用和立即值。要实现 GC，需要：
1. 堆对象用 ID（Int）表示，不直接用 Map 值。
2. 对象存储为 `ArrayList<Map>`，每个 Map 的字段存的是字段值（如果是引用则存 ID）。
3. `mark` 遍历对象图需要知道哪些字段是引用。

### 方案 E：Arc 引用计数 + 即时回收

```aurora
class ArcHeap {
    var objects: ArrayList<Map<String, Any>> = ArrayList.of(256)
    var refCounts: ArrayList<Int> = ArrayList.of()
    var freeList: ArrayList<Int> = ArrayList.of()

    fun alloc(): Int {
        val idx = if (freeList.size > 0) freeList.removeLast() else {
            val i = objects.size
            objects.add(Collections.mutableMapOf())
            i
        }
        refCounts.set(idx, 1)
        return idx
    }

    fun retain(idx: Int): Unit {
        if (idx >= 0 && idx < refCounts.size) {
            refCounts.set(idx, refCounts.get(idx) + 1)
        }
    }

    fun release(idx: Int): Boolean {
        if (idx < 0 || idx >= refCounts.size) return false
        val newCount = refCounts.get(idx) - 1
        refCounts.set(idx, newCount)
        if (newCount <= 0) {
            freeList.add(idx)
            return true  // 已回收
        }
        return false
    }
}
```

**收益**：确定性回收，无 GC 暂停，适合短生命周期脚本。

### 方案 F：FrameManager 用 ArrayList 存储帧

```aurora
class Frame {
    var id: Int = 0
    var funcName: String = ""
    var ip: Int = 0
    var arity: Int = 0
    var locals: ArrayList<String> = ArrayList.of(32)
    var upvalues: ArrayList<Int> = ArrayList.of()
}

class FrameManager {
    var frames: ArrayList<Frame> = ArrayList.of(64)
    var maxCallDepth: Int = 64

    fun allocateFrame(funcName: String, arity: Int): Int {
        if (this.frames.size >= this.maxCallDepth) return -1
        val frame = Frame()
        frame.id = this.frames.size
        frame.funcName = funcName
        frame.arity = arity
        this.frames.add(frame)
        return frame.id
    }

    fun deallocateFrame(frameId: Int): Unit {
        if (frameId >= 0 && frameId < this.frames.size) {
            this.frames.removeLast()  // LIFO 释放
        }
    }

    fun getIp(frameId: Int): Int {
        return this.frames.get(frameId).ip
    }

    fun setIp(frameId: Int, ip: Int): Unit {
        this.frames.get(frameId).ip = ip
    }
}
```

**收益**：O(1) 分配/释放/访问，无字符串重建。

### 方案 G：JitState 用 HashMap

```aurora
class JitState {
    var compiled: HashMap<Int, Boolean> = HashMap.of()
    var skipped: HashMap<Int, String> = HashMap.of()
    var dispatch: HashMap<Int, String> = HashMap.of()
    var callCounts: HashMap<Int, Int> = HashMap.of()
    var threshold: Int = 10000

    fun isCompiled(idx: Int): Boolean {
        return compiled.getOrDefault(idx, false)
    }

    fun callCount(idx: Int): Int {
        return callCounts.getOrDefault(idx, 0)
    }

    fun incCallCount(idx: Int): Int {
        val next = callCount(idx) + 1
        callCounts.put(idx, next)
        return next
    }
}
```

**收益**：O(1) 查找/更新，JIT 热点检测从 O(10000n) 降到 O(10000)。

### 方案 H：实现真正的协程

```aurora
class Coroutine {
    var nextId: Int = 1
    var coroutines: ArrayList<CoroutineInstance> = ArrayList.of()

    fun spawn(body: () -> Any): Int {
        val id = this.nextId
        this.nextId = this.nextId + 1
        val co = CoroutineInstance()
        co.id = id
        co.body = body
        co.status = "waiting"
        co.result = null
        this.coroutines.add(co)
        return id
    }

    fun ask(co_id: Int): Any {
        if (co_id < 0 || co_id >= this.coroutines.size) return null
        val co = this.coroutines.get(co_id)
        if (co.status == "done") return co.result
        // 真正让出执行权（需要 VM 支持 Yield 指令）
        co.status = "running"
        co.result = co.body()
        co.status = "done"
        return co.result
    }
}
```

**注意**：真正的协程让出需要 VM 层支持（`Yield` 指令 + 协程调度器）。当前 VmRunner 不支持。

### 方案 I：实现 JIT 编译流程

```aurora
// 1. 实现 generateClifText：真正将字节码转为 Clif IR
fun generateClifText(code: String, funcIdx: Int, paramCount: Int): String {
    // 需要 JitLower.aura 的完整逻辑
    return JitLower.lowerToClif(code, funcIdx, paramCount)
}

// 2. 实现 registerDispatch：注册到 FFI 分发表
@native fun register_dispatch(idx: Int, token: String): Unit

fun registerDispatch(bridge: VmJitBridge, idx: Int, entry_token: String) {
    register_dispatch(idx, entry_token)
}

// 3. 实现 lookupDispatchToken
@native fun lookup_dispatch(idx: Int): String

fun lookupDispatchToken(idx: Int): String {
    return lookup_dispatch(idx)
}
```

### 方案 J：统一 VM 状态所有权

```
推荐架构：

VmInstance (class, 每 VM 一个实例)
├── interpreter: VmRunner (class)
├── heap: Heap (class)           ← 方案 E 的 ArcHeap
├── gc: GcHeap (class)           ← 方案 D 的 GcHeap
├── frames: FrameManager (class)  ← 方案 F
├── jit: JitState (class)         ← 方案 G
├── jitBridge: VmJitBridge (class)
├── coroutines: CoroutineScheduler (class)  ← 方案 H
├── syncHandles: SyncHandleTable (class)    ← Rust 侧方案 A
└── natives: NativeRegistry (class)         ← Rust 侧已有
```

**原则**：
1. 所有可变状态挂在 `class` 实例上，禁止 `object` 单例可变字段。
2. 所有集合用 `ArrayList`/`HashMap`，禁止字符串编码。
3. 所有句柄（Mutex/Atomic/Thread 等）属于 VM 实例，随 VM 生命周期自动回收。
4. VM 销毁时，所有子组件的引用同时断开，GC 可回收。

---

## 12. 优先级排序

| 优先级 | 问题 | 方案 | 工作量 | 影响 |
|--------|------|------|--------|------|
| P0 🔴 | 全局 `Coroutine` 可变状态 | A | 小 | 安全/正确性 |
| P0 🔴 | 字符串栈/locals/globals O(n²) | B | 中 | 性能（3000×） |
| P0 🔴 | GC 全空壳 | D | 大 | 内存泄漏 |
| P0 🔴 | JIT 编译流程空壳 | I | 中 | 功能缺失 |
| P1 🟡 | FrameManager 字符串帧 | F | 中 | 性能 |
| P1 🟡 | JitState 字符串记录表 | G | 小 | 性能 |
| P1 🟡 | dispatch 30 次 if 检查 | C | 小 | 性能 |
| P1 🟡 | Arc 引用计数不生效 | E | 中 | 内存 |
| P2 🟢 | vrToInt 10× 冗余 | (改 parseInt) | 小 | 性能 |
| P2 🟢 | VmJitBridge FFI 声明无实现 | (补 @native) | 中 | 功能 |
| P3 🔵 | 统一 VM 状态所有权 | J | 大 | 架构 |

---

## 13. 总结

当前 Aura 侧代码的核心问题是**用字符串模拟所有数据结构**，以及**用 `object` 单例承载可变状态**。这导致：

1. **性能灾难**：栈操作 O(n²)、局部变量 O(n)、指令分派 O(m)、JIT 热点检测 O(10000n)。估算 3000× 性能差距。
2. **内存泄漏**：GC 全空壳、ARC 不生效、协程列表不回收、帧不释放。内存单调增长，永不回收。
3. **全局污染**：`core/Coroutine` 的 `nextId` 和 `coroutines` 是进程全局，跨 VM 实例共享。std 并发原语句柄指向 Rust 全局注册表。
4. **功能缺失**：JIT 编译流程是空壳（`generateClifText` 输出无效 IR、`registerDispatch`/`lookupDispatchToken` 空函数）。Coroutine 不真正让出执行权。
5. **唯一正确范式**：`TestRunner.aura` 用 `class` 实例承载状态，并明确注释了原因。这应成为所有组件的模板。

**核心建议**：将所有可变状态从 `object` 迁移到 `class`，将所有字符串模拟的数据结构替换为 `ArrayList`/`HashMap`，实现真正的 GC（方案 D）或 ARC（方案 E），补全 JIT 编译流程（方案 I）。这些改动可以独立进行，建议按 P0→P1→P2 顺序推进。
