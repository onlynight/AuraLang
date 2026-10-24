# Photon 编译后端内存优化方案

> **定位**：基于 ARC 的 Photon 后端内存优化实施方案。根因分析见 [photon-mem-analysis.md](photon-mem-analysis.md)。

---

## 一、问题概要

Photon 编译后端完全运行在 Aura VM 中，依赖 ARC 逐对象回收。三个根因导致内存持续增长：

| 根因 | 影响 | 对应优先级 |
|------|------|-----------|
| **阶段释放不完整** — `ssaBuilder = SsaBuilder()` 仅重指向局部变量，旧对象 ARC 不归零则残留 | 阶段间残留 30-60 MB | P0 |
| **字符串拼接 O(n²)** — `traceBuf`/`outputHex` 每次追加整段拷贝 | 累计分配数十 MB + 碎片化 | P1 |
| **VM HashMap 空间放大** — 对象字段 `HashMap<u16, Value>` 桶开销 1.4-2.9× | 100-200 MB 纯桶开销 | P2 |

当前峰值：**>1 GB**（50k 行源码），极端情况 **>20 GB**（死循环 eprintln）。
目标：峰值 **<300 MB**，消除 20 GB 风险。

---

## 二、优化架构

```
┌────────────────────────────────────────────────────┐
│                PhotonPipeline                        │
│                                                     │
│  PhaseArena（阶段分配器）                             │
│  ├── Phase A Arena: SsaBuilder 所有对象              │
│  │   Phase B 开始 → reset() 整块释放                │
│  ├── Phase B Arena: LirProgram 所有对象              │
│  │   Phase C 开始 → reset() 整块释放                │
│  ├── Phase C Arena: MachineDag 所有对象              │
│  │   Phase E 结束 → reset() 整块释放                │
│  └── Shared Arena: BackendResult 等跨阶段对象        │
│      管线结束 → reset() 整块释放                     │
│                                                     │
│  BoundedBuffer（有界缓冲区）                          │
│  ├── traceBuf:    max=3KB, 分段追加                  │
│  ├── probeBuf:    max=3KB, 分段追加                  │
│  └── outputHex:   动态上限, 分段追加                  │
└────────────────────────────────────────────────────┘
```

---

## 三、实现计划

### P0：诊断日志条件化 + 阶段边界清理（0.5 天）

**目标**：消除 20 GB 死循环风险，不改变架构。

#### 3.1.1 阶段边界强制清理

**代码位置**：`PhotonPipeline.aura:296-303, 312-318, 595-614`

```
// 当前（不完整）：
ssaBuilder = SsaBuilder()   // 仅重指向局部变量，旧对象可能残留

// 修改为（完整清理）：
ssaBuilder.clear()          // 清空 varMap, varVersion, loopStack 等字段
ssaBuilder = SsaBuilder()
```

为 `SsaBuilder` 增加 `clear()` 方法：

```
fun clear(): Unit {
    this.program = MirSsaProgram()
    this.varMap = ""
    this.varVersion = ""
    this.loopStack = ""
    this.curParams = ""
    this.curFunc = -1
    this.curBlock = 0
    this.memHead = -1
    this.tmpCount = 0
}
```

同样为 `LirProgram` 增加 `clear()`，在 Phase C 开始前调用。

#### 3.1.2 诊断日志条件化

**代码位置**：`PhotonPipeline.aura:183-194, 218-226`

当前 `traceMark` 在 `traceOn == false` 时已跳过，但 `peProbe` 没有：

```
// 当前（无条件执行）：
private fun peProbe(msg: String): Unit {
    if (this.probePath == "") { return }  // 路径空时跳过，但路径非空时无条件执行
    ...
}
```

修改为：`peProbe` 始终检查 `probePath` 是否为空（已有），并确保 `verboseOn == false` 时 `vprintln` 不分配字符串。

**预期效果**：消除 20 GB 死循环风险。

---

### P1：BoundedBuffer 替换字符串拼接（1-2 天）

**目标**：消除 O(n²) 字符串拼接，峰值 -50%。

#### 3.2.1 实现 BoundedBuffer 类

**新文件**：`aura/photon/aura/lang/compiler/photon/BoundedBuffer.aura`

```
package aura.lang.compiler.photon
import aura.lang.std.String

/// 有界分段缓冲区：分段追加，超限丢弃最旧段。
class BoundedBuffer {
    private var segments: List<String> = arrayListOf<String>()
    private var maxBytes: Long = 0
    private var totalBytes: Long = 0

    fun BoundedBuffer(maxBytes: Long): BoundedBuffer {
        this.maxBytes = maxBytes
        return this
    }

    fun append(data: String): Unit {
        this.segments = arrayListOf<String>()
        // 清空旧段，只保留最后 maxBytes 字节
        var result = ""
        var remaining = this.maxBytes
        var i = this.segments.size() - 1
        while (i >= 0 && remaining > 0) {
            val seg = this.segments.get(i)
            if (seg.length() <= remaining) {
                result = seg + result
                remaining = remaining - seg.length()
            } else {
                result = seg.substring(seg.length() - remaining.toInt()) + result
                remaining = 0
            }
            i = i - 1
        }
        this.segments = arrayListOf<String>(result)
        this.totalBytes = result.length().toLong()
    }

    fun appendSegment(data: String): Unit {
        this.segments.add(data)
        this.totalBytes = this.totalBytes + data.length().toLong()
        // 超限则丢弃最早的段
        while (this.totalBytes > this.maxBytes && this.segments.size() > 1) {
            val removed = this.segments.get(0)
            this.segments.removeAt(0)
            this.totalBytes = this.totalBytes - removed.length().toLong()
        }
    }

    fun toString(): String {
        var result = ""
        var i = 0
        while (i < this.segments.size()) {
            result = result + this.segments.get(i)
            i = i + 1
        }
        return result
    }

    fun length(): Int {
        return this.totalBytes.toInt()
    }
}
```

#### 3.2.2 替换 traceBuf

**代码位置**：`PhotonPipeline.aura:178-194`

```
// 当前（O(n²) 拼接）：
private fun traceMark(msg: String): Unit {
    if (!this.traceOn) { return }
    this.traceStep = this.traceStep + 1
    this.traceBuf = this.traceBuf + "[s" + this.traceStep + "] " + msg + "\n"
    if (this.traceBuf.length > 3000) {
        this.traceBuf = this.traceBuf.substring(this.traceBuf.length - 2500)
    }
    if (this.traceStep % 50 == 0) {
        FileUtils.writeText(this.outDir + "/photon_trace.log", this.traceBuf)
    }
}

// 修改为（分段追加）：
private var traceBuf: BoundedBuffer = BoundedBuffer(3000)

private fun traceMark(msg: String): Unit {
    if (!this.traceOn) { return }
    this.traceStep = this.traceStep + 1
    this.traceBuf.appendSegment("[s" + this.traceStep + "] " + msg + "\n")
    if (this.traceStep % 50 == 0) {
        FileUtils.writeText(this.outDir + "/photon_trace.log", this.traceBuf.toString())
    }
}
```

#### 3.2.3 替换 outputHex / relocRecords

**代码位置**：`X86Emitter.aura`

```
// 当前（O(n²) 拼接）：
this.outputHex = this.outputHex + hex

// 修改为（分段追加）：
private var outputBuf: BoundedBuffer = BoundedBuffer(Long.MAX_VALUE)
outputBuf.appendSegment(hex)

// 最终输出时一次性拼接
val machineCodeHex: String = outputBuf.toString()
```

#### 3.2.4 替换 peProbe 缓冲区

**代码位置**：`PhotonPipeline.aura:218-226`

```
// 修改为：
private var probeBuf: BoundedBuffer = BoundedBuffer(3000)

private fun peProbe(msg: String): Unit {
    if (this.probePath == "") { return }
    this.probeStep = this.probeStep + 1
    this.probeBuf.appendSegment("[s" + this.probeStep + "] " + msg + "\n")
    FileUtils.writeText(this.probePath, this.probeBuf.toString())
}
```

**预期效果**：峰值内存从 ~200 MB 降至 ~100 MB。

---

### P2：PhaseArena 实现 + SsaBuilder 迁移（3-5 天）

**目标**：阶段隔离 + 批量回收，峰值 -60%。

#### 3.3.1 实现 PhaseArena 类

**新文件**：`aura/photon/aura/lang/compiler/photon/PhaseArena.aura`

```
package aura.lang.compiler.photon
import aura.lang.std.String

/// 阶段分配器：bump pointer 分配，reset() 整块释放。
class PhaseArena {
    private var chunks: List<String> = arrayListOf<String>()
    private var currentOffset: Long = 0
    private var currentChunk: String = ""
    private var chunkSize: Long = 65536  // 64KB 初始块

    fun PhaseArena(name: String): PhaseArena {
        this.name = name
        return this
    }

    private var name: String = ""

    /// 分配 size 字节的对齐内存。
    fun alloc(size: Long, align: Int): Long {
        var alignedOffset = alignTo(this.currentOffset, align)
        if (alignedOffset + size > this.currentChunk.length().toLong()) {
            this.grow(alignedOffset + size)
        }
        this.currentOffset = alignedOffset + size
        return alignedOffset
    }

    /// 整块释放。
    fun reset(): Unit {
        this.currentOffset = 0
        this.currentChunk = ""
        this.chunks = arrayListOf<String>()
    }

    private fun grow(required: Long): Unit {
        var newChunkSize = this.chunkSize
        while (newChunkSize < required) {
            newChunkSize = newChunkSize * 2
        }
        this.currentChunk = this.currentChunk + ""  // 创建新块
        this.chunkSize = newChunkSize
    }

    private fun alignTo(offset: Long, align: Int): Long {
        if (align <= 1) return offset
        val mask = (align.toLong() - 1).toInt()
        return (offset + mask.toLong()) & (~mask.toLong())
    }

    fun stats(): String {
        return "arena=" + this.name +
            " chunks=" + this.chunks.size() +
            " current=" + this.currentOffset
    }
}
```

#### 3.3.2 迁移 SsaBuilder

**代码位置**：`SsaBuilder.aura`

当前 SsaBuilder 使用 VM 堆上的 `List<...>` 和扁平字符串。迁移到 PhaseArena 需要：

1. 将 `program: MirSsaProgram` 替换为 PhaseArena 分配的字节数组
2. 将 `varMap`/`varVersion`/`loopStack` 扁平字符串替换为 PhaseArena 分配的字节数组
3. 节点 ID 使用 Arena 偏移量而非 List 索引

**关键改动**：

```
// 当前：
private var program: MirSsaProgram = MirSsaProgram()
private var varMap: String = ""

// 迁移后：
private var arena: PhaseArena = PhaseArena("ssa")
private var programOffset: Long = 0   // 在 Arena 中的偏移
private var varMapOffset: Long = 0
```

**注意**：此迁移涉及 SsaBuilder 的所有数据访问模式，需要逐步替换。建议先从最小改动开始——仅将大列表迁移到 Arena，扁平字符串保留 VM 堆。

#### 3.3.3 管线集成

**代码位置**：`PhotonPipeline.aura`

```
fun compileHir(hir: Hir, outDir: String, moduleName: String): BackendResult {
    this.outDir = outDir
    this.moduleName = moduleName

    // 创建阶段分配器
    val phaseAArena = PhaseArena("ssa")
    val phaseBArena = PhaseArena("lir")
    val phaseCArena = PhaseArena("dag")
    val phaseEArena = PhaseArena("emit")

    // Phase A: HIR → SSA MIR
    var ssaBuilder = SsaBuilder()
    ssaBuilder.setArena(phaseAArena)
    ssaBuilder = SsaBuilderUtils.build(hir)

    // Phase B: SSA → LIR
    val lowering = Lowering()
    var lir = lowering.lower(ssaBuilder)
    lir.setArena(phaseBArena)

    // Phase A 结束 → 释放
    phaseAArena.reset()
    ssaBuilder.clear()

    // Phase C: LIR → DAG
    val selector = InstructionSelectorUtils.emptySelector()
    var dag = selector.select(lir)
    dag.setArena(phaseCArena)

    // Phase B 结束 → 释放
    phaseBArena.reset()
    lir.clear()

    // ... Phase D, E ...

    // 管线结束 → 释放全部
    phaseCArena.reset()
    phaseEArena.reset()

    return result
}
```

**预期效果**：峰值内存从 ~100 MB 降至 ~50-70 MB。

---

### P3：MachineDag 迁移到 PhaseArena（5-10 天）

**目标**：DAG 节点批量回收，峰值 -80%。

#### 3.4.1 DagNode 结构化存储

当前 `MachineDag` 使用 `List<DagNode>`，每个节点是一个 VM 堆对象。迁移后：

```
class MachineDag {
    private var arena: PhaseArena = PhaseArena("dag")
    private var nodeCount: Int = 0

    // DagNode 结构化布局（字节数组）：
    //   offset 0:  kind (4 bytes, string index)
    //   offset 4:  op   (4 bytes, string index)
    //   offset 8:  type (4 bytes)
    //   offset 12: reg  (4 bytes, string index)
    //   offset 16: args (4 bytes, string offset)
    //   offset 20: uses (4 bytes, string offset)
    //   offset 24: chain (4 bytes)
    //   offset 28: aux  (4 bytes, string offset)
    // 总大小: 32 bytes / node

    fun nodeOf(id: Int): DagNode {
        val base = id * 32
        val kind = this.readString(base + 0)
        val op = this.readString(base + 4)
        // ... 逐字段读取
        return DagNode()
    }
}
```

**预期效果**：峰值内存从 ~50 MB 降至 ~30 MB。

---

### P4：Rust 原生管线重构（2-4 周）

**目标**：消除 VM HashMap 空间放大，峰值 -90%。

将 SSA/LIR/DAG/RegAlloc 管线从 Aura VM 迁移到 Rust 原生：

```
// Rust 实现
use std::collections::HashMap;

#[derive(Clone)]
struct SsaProgram {
    functions: Vec<Function>,
    values: Vec<Value>,
    blocks: Vec<Block>,
}

// Vec<NamedStruct> 代替 HashMap<u16, Value>
// 空间利用率: 100% vs HashMap 的 40-50%
```

**预期效果**：峰值内存从 ~30 MB 降至 ~15-20 MB。

---

## 四、效果预估

| 阶段 | 峰值内存 | 节省 | 投入 |
|------|---------|------|------|
| 当前 | **>1 GB** | — | — |
| P0：日志条件化 + 边界清理 | ~200-300 MB | -70-80% | 0.5 天 |
| P1：BoundedBuffer | ~100 MB | -50% | 1-2 天 |
| P2：PhaseArena + SsaBuilder | ~50-70 MB | -40-50% | 3-5 天 |
| P3：PhaseArena + MachineDag | ~30 MB | -50% | 5-10 天 |
| P4：Rust 原生管线 | ~15-20 MB | -50% | 2-4 周 |

---

## 五、验证方法

### 5.1 内存测量

```bash
# Windows: 监控 photon.exe 工作集
wmic process where "name='photon.exe'" get WorkingSetSize

# WSL/Linux: 使用 /usr/bin/time
/usr/bin/time -v aura build target.aura 2>&1 | grep "Maximum resident"
```

### 5.2 验证步骤

1. 应用 P0 修复 → 编译前后对比内存
2. 应用 P1 修复 → 编译前后对比内存
3. 确认编译器输出正确性（产物 exe 行为一致）
4. 测量时间影响（Arena 分配可能比 VM 堆稍慢）

### 5.3 回归测试

```bash
# 全量测试
cargo test -p compiler
cargo test -p cli

# Photon 差分测试
./scripts/photon-diff-test.sh
```

---

## 六、风险控制

| 风险 | 缓解措施 |
|------|---------|
| Arena 分配导致碎片化 | 每个阶段独立 Arena，阶段结束即释放 |
| 迁移期间双路径不一致 | 保留 VM 堆路径作为后备，通过环境变量切换 |
| SsaBuilder 重构引入 bug | 逐字段迁移，每迁移一个字段就跑回归测试 |
| 字符串索引读取错误 | Arena 内字符串使用固定偏移量，增加 bounds check |

---

## 七、总结

| 指标 | 当前 | P0+P1 | P0+P1+P2 | 全部完成 |
|------|------|-------|----------|---------|
| 50k 行源码峰值 | **>1 GB** | ~100 MB | ~50-70 MB | ~15-20 MB |
| 20 GB 死循环风险 | 存在 | 消除 | 消除 | 消除 |
| 碎片化 | 严重 | 中等 | 轻微 | 无 |
| 编译速度 | 基准 | 轻微下降 | 轻微下降 | 显著加速 |
| 投入 | — | 1.5-2.5 天 | 4.5-7.5 天 | 4-6 周 |

**核心方案**：PhaseArena（阶段批量分配/批量释放）+ BoundedBuffer（有界分段缓冲）替代 VM 堆 ARC 逐对象回收，可将峰值内存降低 **90%**，同时消除 20 GB 死循环风险。
