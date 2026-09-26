# 07 · Photon JIT 自举设计方案

> **定位**：使用 Aura 自举 + Photon 编译后端重新实现 JIT 编译器，完全替换 Cranelift 依赖
> **核心思路**：Photon 后端已有完整的机器码生成管线（`compileEncodeOnly`）和 JIT 基础设施（`JitBackend`），但当前被 `vmMode=true` 锁死在模拟模式。本方案通过 Aura 自举打通原生运行时，让 JIT 基础设施在自举后真正可用，从而用 Photon 替换 Cranelift，消除最后的 Rust JIT 依赖。
> **配套文档**：`02-技术方案.md`（旧 Cranelift JIT 方案）、`docs/photon/photon-self-contained-design-v3.md`（Photon 后端设计）
> **文档日期**：2026-09-25

---

## 一、背景与动机

### 1.1 当前 JIT 架构的问题

当前 Aura JIT 采用 **Cranelift（Rust crate）** 作为机器码后端，通过 FFI 边界连接纯 Aura 侧：

```text
当前架构（Cranelift JIT）：
┌─ 纯 Aura 侧（已完成 100%） ──────────────────────────────┐
│ JitState → JitOpt → JitLower → JitDispatch → VmJitBridge │
│ Clif IR 文本生成（.clif）                                  │
└───────────────────────────┬──────────────────────────────┘
                            │ FFI 边界（jit_ffi.rs）
                            ▼
┌─ Rust 侧（保留） ────────────────────────────────────────┐
│ jit_compile(clif_text) → Cranelift JITBuilder → blob     │
│ jit_load(blob) → mmap(RW) → mprotect(RX)                 │
│ jit_call(entry_token) → dispatch_table + call_indirect   │
│ 依赖：cranelift 0.116 (Rust crate)                        │
└───────────────────────────────────────────────────────────┘
```

**核心问题**：
1. **Cranelift 是 Rust crate**——JIT 必须进程内编译（1–5ms），无法像 AOT 那样走外部子进程，因此必须保留 Rust FFI 边界
2. **`jit_ffi.rs::jit_compile` 是占位实现**——当前忽略 `clif_text` 参数，只编译空函数，需要引入 `cranelift::codegen::parse::parse_program` 才能解析 Clif 文本
3. **语言种类多**：Rust + Aura + LLVM + Cranelift 四者共存，违反"减少语言种类"的纯 Aura 化目标

### 1.2 Photon 后端的优势

Photon 后端（28 文件，~13,244 行）已经具备 JIT 所需的全部基础设施：

| 组件 | 文件 | 能力 | 当前状态 |
|------|------|------|----------|
| 机器码生成管线 | `PhotonPipeline.aura` | `compileEncodeOnly`/`compileJit` 返回裸机器码 hex | ✅ 可产出 hex |
| W^X 内存管理 | `JitBackend.aura` | `allocateExecMemory`/`setExecutable`/`writeCode` | ✅ 真实实现（vmMode=true 模拟） |
| 分派表 | `JitBackend.aura` | `allocateDispatchSlot`/`registerFunction`/`lookupFunction` | ✅ 真实实现 |
| 存根生成 | `JitBackend.aura` | dispatch/call/return/deopt 四种存根 | ✅ 产出真实 x86_64 hex |
| 原生执行 | `JitBackend.aura` | `executeNative` 通过 `JitExec.callI64` 调用 | ✅ 真实实现（需 enableNativeExec） |
| Syscall 发射 | `SyscallEmitter.aura` | 直接生成 syscall 指令（Nt*/Linux） | ✅ 完整实现 |
| X86 编码器 | `x86_64/X86Encoder.aura` | x86_64 指令编码 | ✅ 完整实现 |

**关键洞察**：Photon 后端**完全自包含**——不依赖 LLVM、不依赖外部工具链、不依赖 Cranelift。所有机器码生成都在 Aura 内部完成。这意味着 JIT 可以**完全在纯 Aura 侧运行**，无需任何 Rust FFI 边界。

### 1.3 设计目标

1. **完全消除 Cranelift 依赖**——用 Photon 后端替换 Cranelift，JIT 机器码生成完全在 Aura 内部完成
2. **消除 FFI 边界**——不需要 `jit_ffi.rs` 的 `jit_compile`/`jit_load`/`jit_call`，JIT 全程纯 Aura
3. **自举可行**——JIT 基础设施（纯 Aura）由自举编译，在原生运行时可用
4. **最小 native 依赖**——仅保留 `Memory`/`JitExec` extern interface（由 Photon 降低为 syscall 指令）
5. **向后兼容**——保留现有 JitState/JitDispatch/JitAbi 的字节码级 JIT 逻辑

---

## 二、现状分析

### 2.1 当前 JIT 路径（Cranelift）

```text
字节码（.auc）
  │
  ▼
JitCore.aura ──── 预解码：常量池内联、分支绝对→相对、白名单检查
  │                 输出：JitUnit（行表 code + consts + funcs）
  ▼
JitOpt.aura ───── 7 个优化 pass（常量折叠/死码消除/跳转线程化/强度削弱/
  │                 指令调度/函数内联/循环展开）
  ▼
JitLower.aura ─── 字节码 → Cranelift 文本 IR（.clif）
  │                 纯字符串拼接，逐指令发射 SSA 值名
  ▼
VmJitBridge.aura ─ @native jit_compile(clif_text) → blob_b64
  │                 @native jit_load(blob_b64) → entry_token
  │                 @native jit_call(entry_token, args) → result
  ▼
jit_ffi.rs ────── 🔴 占位实现（忽略 clif_text，编译空函数）
  │                 Cranelift JITBuilder → mmap(RW) → mprotect(RX)
  ▼
机器码执行
```

**问题**：
- `JitLower` 生成 .clif 文本 → 需要 Cranelift 解析（Rust）
- `jit_ffi.rs::jit_compile` 未接通 Cranelift 解析（只编译空函数）
- 整个 FFI 边界（5 个 @native 函数）必须保留 Rust

### 2.2 Photon 后端现有能力

```text
Photon 管线（AOT 路径，已可运行）：
  HIR → SSA MIR → LIR → MachineDag → RegAlloc → Peephole
    → X86Emitter → PhotonObjectWriter(COFF) → lld-link → exe

Photon 管线（JIT 路径，可产出机器码 hex）：
  HIR → SSA MIR → LIR → MachineDag → RegAlloc → Peephole
    → X86Emitter → machineCodeHex（裸机器码，无 COFF/link）
```

**JitBackend.aura 现有能力**（697 行，真实实现）：
- W^X 内存：`allocateExecMemory`（mmap）→ `writeCode`（写入 hex）→ `setExecutable`（mprotect RX）
- 分派表：`allocateDispatchSlot`/`registerFunction`/`lookupFunction`/`deoptimize`
- 存根生成：`buildDispatchStub`（RIP 间接跳转）/`buildCallStub`/`buildReturnStub`/`buildDeoptTrampoline`
- 管线桥接：`compileMachineCode` 调用 `PhotonPipeline.compileEncodeOnly`
- 原生执行：`executeNative` 通过 `JitExec.callI64` 调用机器码

**致命约束**：`vmMode` 默认 `true`，所有内存操作被模拟。必须调用 `enableNativeExec()` 才能启用真实 JIT，但此路径依赖 `Memory`/`JitExec` extern——仅 AOT/自举原生运行时可用。

### 2.3 自举链现状

```text
Stage-1: Rust 编译器 → aura-compiler-c2.exe（原生载体）
Stage-2: aura-compiler-c2.exe 自我编译 → aura-compiler-native2.exe
         规模：51 模块 / 57834 HIR 节点 / ~45 MB / 11s
         后端：Aura 侧 Aot.aura → LLVM IR 文本 → llc → clang → exe
Stage-3: aura-compiler-native2.exe 编译用户程序（含 @native FFI）
```

**关键**：Stage-1/2/3 全走 AOT（LLVM）路径，不涉及 JIT。JIT 是运行时优化。

### 2.4 差距分析

| 项目 | 当前状态 | 目标状态 |
|------|----------|----------|
| JIT 机器码后端 | Cranelift（Rust，FFI） | Photon（Aura，自包含） |
| FFI 边界 | 5 个 @native 函数 | 0 个（JIT 全程 Aura） |
| .clif 文本 IR | 需要 Cranelift 解析 | 不需要（Photon 直接用 HIR） |
| W^X 内存 | Rust 侧 `jit_ffi.rs::mmap_alloc` | Aura 侧 `JitBackend.allocateExecMemory` |
| 分派表 | Rust 侧全局 HashMap | Aura 侧 `JitBackend.dispatchTable` |
| 原生执行 | Rust 侧 `transmute` 调用 | Aura 侧 `JitExec.callI64` |
| 自举集成 | JIT 不参与自举 | JIT 基础设施随自举编译 |
| vmMode 锁死 | 默认 true（模拟） | 自举后可 enableNativeExec |

---

## 三、总体架构

### 3.1 新架构概览

```text
┌─ 纯 Aura 侧（全部可自举，无 Rust 依赖） ──────────────────────────────┐
│                                                                       │
│  ┌─ 前端 ──────────────────────────────────────────────────────┐    │
│  │ Lexer → Parser → AST → Sema → HIR → MIR → Bytecode          │    │
│  │                                          │                   │    │
│  │                          ┌───────────────┘                   │    │
│  │                          ▼                                   │    │
│  │              ┌──────────────────┐                            │    │
│  │              │  HirCache.aura   │ ← 新增：编译期缓存 HIR     │    │
│  │              │  (funcIdx→HIR)   │    供运行时 JIT 使用        │    │
│  │              └────────┬─────────┘                            │    │
│  └───────────────────────┼──────────────────────────────────────┘    │
│                          │                                           │
│  ┌─ VM 解释器 ───────────┼────────────────────────────────────┐     │
│  │ Vm.aura / VmRunner    │                                    │     │
│  │ 执行字节码            │                                    │     │
│  │                        │                                    │     │
│  │ 热点检测 ──────────────┼──────────────────────────┐        │     │
│  └───────────────────────┼──────────────────────────┼────────┘     │
│                          │                          │               │
│  ┌─ JIT 前端 ────────────┼──────────────────────────┼────────┐     │
│  │ JitState.aura         │                          │        │     │
│  │ 热点阈值/白名单/编译顺序                      │        │     │
│  │ JitDispatch.aura      │                          │        │     │
│  │ NATIVE/SKIP/DEFER 派发决策                    │        │     │
│  │ JitAbi.aura           │                          │        │     │
│  │ JitValue ABI (tag+payload)                    │        │     │
│  └───────────────────────┼──────────────────────────┼────────┘     │
│                          │                          │               │
│  ┌─ JIT 后端（Photon）──┼──────────────────────────┼────────┐     │
│  │                       │  ┌─────────────────┐     │        │     │
│  │ HirCache.get(idx) ───┼─→│ PhotonPipeline   │     │        │     │
│  │                       │  │ .compileJit(HIR) │     │        │     │
│  │                       │  │ HIR→SSA→LIR→DAG  │     │        │     │
│  │                       │  │ →RegAlloc→Encode │     │        │     │
│  │                       │  │ → machineCodeHex │     │        │     │
│  │                       │  └────────┬────────┘     │        │     │
│  │                       │           │              │        │     │
│  │                       │  ┌────────▼────────┐     │        │     │
│  │                       │  │  JitBackend      │     │        │     │
│  │                       │  │ .emitFunction(hex)│     │        │     │
│  │                       │  │ mmap(RW)→write→mprotect(RX)     │     │
│  │                       │  │ → entry point     │     │        │     │
│  │                       │  │ .registerFunction │     │        │     │
│  │                       │  └────────┬────────┘     │        │     │
│  │                       │           │              │        │     │
│  │                       │  ┌────────▼────────┐     │        │     │
│  │                       │  │ JitExec.callI64 │     │        │     │
│  │                       │  │ (entry, args)    │     │        │     │
│  │                       │  │ → result         │     │        │     │
│  │                       │  └─────────────────┘     │        │     │
│  └───────────────────────┼──────────────────────────┼────────┘     │
│                          │                          │               │
│  ┌─ Native 原语（@native，由 Photon 降低为 syscall） ─────┐       │
│  │ Memory.alloc / Memory.free / Memory.mprotect          │       │
│  │ Memory.write / Memory.write32 / Memory.write64        │       │
│  │ JitExec.call0 / JitExec.callI64                       │       │
│  └───────────────────────────────────────────────────────┘       │
└───────────────────────────────────────────────────────────────────┘
```

### 3.2 与旧架构的对比

| 维度 | 旧架构（Cranelift） | 新架构（Photon） |
|------|---------------------|------------------|
| 机器码后端 | Cranelift（Rust crate） | Photon（纯 Aura） |
| IR 形式 | Clif 文本 IR（.clif） | HIR（原生数据结构） |
| FFI 边界 | 5 个 @native 函数 | 0 个（JIT 全程 Aura） |
| Rust 依赖 | cranelift 0.116 | 无 |
| W^X 内存 | Rust `mmap_alloc` | Aura `JitBackend.allocateExecMemory` |
| 分派表 | Rust 全局 HashMap | Aura `JitBackend.dispatchTable` |
| 原生调用 | Rust `transmute` | Aura `JitExec.callI64` |
| 可自举 | 否（FFI 边界不可自举） | 是（全部 Aura） |
| 语言种类 | Rust + Aura + LLVM + Cranelift | Aura + LLVM（AOT 路径） |

### 3.3 自举链集成

```text
新自举链（Stage-1/2/3 + JIT 支持）：

Stage-1: Rust 编译器 → aura-compiler-c2.exe
         包含：完整前端 + AOT 后端 + Photon 后端 + JIT 基础设施
         JIT 状态：vmMode=true（种子 VM 下模拟）

Stage-2: aura-compiler-c2.exe 自我编译 → aura-compiler-native2.exe
         后端：AOT（LLVM）或 Photon（均可）
         JIT 状态：vmMode=false（原生运行时，enableNativeExec 已调用）
         ✅ JIT 真正可用！

Stage-3: aura-compiler-native2.exe 编译用户程序
         用户程序运行时可使用 JIT
         ✅ 用户程序享受 JIT 加速

Stage-4（可选）: aura-compiler-native2.exe 用 Photon 后端编译自身
         → aura-compiler-native3.exe（纯 Photon 自举）
         → 验证自举一致性
```

---

## 四、详细设计

### 4.1 新增组件

#### 4.1.1 HirCache.aura — HIR 缓存

**目的**：在编译期缓存每个函数的 HIR，供运行时 JIT 使用。

**背景**：Photon 后端以 HIR 为输入，但 JIT 在运行时触发——此时 HIR 已被 MIR 消费。HirCache 在 HIR 生成后、MIR 降低前保存 HIR 副本。

```aura
// aura/compiler/aura/lang/compiler/jit/HirCache.aura

import aura.lang.compiler.hir.Hir
import aura.lang.collection.HashMap

/// HIR 缓存：编译期存储，运行时 JIT 读取
class HirCache {

    /// funcIdx → HIR 节点数据（字符串表示）
    var cache: HashMap<Int, String> = HashMap<Int, String>()

    /// 编译期：注册函数的 HIR
    fun put(funcIdx: Int, hirData: String): Unit {
        this.cache.put(funcIdx, hirData)
    }

    /// 运行时 JIT：获取函数的 HIR
    fun get(funcIdx: Int): String {
        return this.cache.get(funcIdx)
    }

    /// 函数是否已缓存
    fun has(funcIdx: Int): Boolean {
        return this.cache.has(funcIdx)
    }

    /// 缓存的函数数量
    fun size(): Int {
        return this.cache.size
    }

    /// 清空缓存
    fun clear(): Unit {
        this.cache.clear()
    }
}

/// 全局 HIR 缓存实例
object GlobalHirCache {
    val cache: HirCache = HirCache()
}
```

**数据格式**：HIR 需要序列化为字符串以便存储。可以使用现有的 PHIR 序列化器（`PhirSerializer.aura`）或自定义 HIR 序列化格式。

**集成点**：
- 在 `Aot.aura` 或 `Codegen.aura` 中，HIR 生成后立即缓存
- 在 JIT 编译触发时，从缓存读取 HIR，交给 `PhotonPipeline.compileJit`

#### 4.1.2 JitBridge.aura — VM-JIT 桥接（替换 VmJitBridge.aura）

**目的**：替代现有的 `VmJitBridge.aura`，将 VM 派发路径连接到 Photon JIT 后端。

**与旧 VmJitBridge 的区别**：
- 旧：调用 @native FFI（`jit_compile`/`jit_load`/`jit_call`）
- 新：直接调用 Aura 侧的 `PhotonPipeline.compileJit` + `JitBackend.emitFunction` + `JitExec.callI64`

```aura
// aura/compiler/aura/lang/compiler/jit/JitBridge.aura

import aura.lang.compiler.jit.JitState
import aura.lang.compiler.jit.JitDispatch
import aura.lang.compiler.jit.JitAbi
import aura.lang.compiler.jit.HirCache
import aura.lang.compiler.backend.photon.PhotonPipeline
import aura.lang.compiler.backend.photon.JitBackend
import aura.lang.native.JitExec

/// JIT 桥接：连接 VM 派发与 Photon JIT 后端
class JitBridge {

    /// JIT 状态（热点检测、编译顺序）
    var state: JitState = JitState()

    /// JIT 后端（W^X 内存、分派表）
    var backend: JitBackend = JitBackend()

    /// 热点阈值
    var threshold: Int = 10000

    /// 是否启用 JIT
    var enabled: Boolean = false

    /// 原生运行时模式（enableNativeExec 后为 true）
    var nativeMode: Boolean = false

    init() {
        this.state = JitState()
        this.backend = JitBackend()
        this.threshold = 10000
        this.enabled = false
        this.nativeMode = false
    }

    /// 启用原生执行（仅在 AOT/自举原生运行时调用）
    fun enableNative(): Unit {
        this.backend.enableNativeExec()
        this.nativeMode = true
        this.enabled = true
    }

    /// VM 函数调用钩子：检查是否 JIT 编译
    fun callHook(idx: Int, args: String): String {
        if (!this.enabled) {
            return ""  // JIT 未启用，回退解释器
        }

        // 1. 派发决策
        val decision: String = JitDispatch.jitDecide(this.state, idx)

        if (decision == JitDecide.NATIVE) {
            // 已编译，走原生码
            return this.nativeCall(idx, args)
        } else if (decision == JitDecide.SKIP) {
            // 已跳过，回退解释器
            return ""
        } else {
            // DEFER：检查是否达阈值
            this.state.incrementCallCount(idx)
            if (this.state.shouldCompile(idx) || this.state.canForce(idx)) {
                val ok: Boolean = this.tryCompile(idx)
                if (ok) {
                    return this.nativeCall(idx, args)
                } else {
                    this.state.skip(idx, "Photon 编译失败")
                    return ""
                }
            }
            return ""  // 未达阈值
        }
    }

    /// 尝试用 Photon 编译函数
    fun tryCompile(idx: Int): Boolean {
        if (!this.nativeMode) {
            return false
        }

        // 1. 从 HIR 缓存获取 HIR
        val hirData: String = GlobalHirCache.cache.get(idx)
        if (hirData == "") {
            this.state.skip(idx, "无缓存 HIR")
            return false
        }

        // 2. 反序列化 HIR
        val hir: Hir = Hir.deserialize(hirData)

        // 3. 用 Photon 编译为机器码 hex
        val pipeline: PhotonPipeline = PhotonPipeline()
        val result: BackendResult = pipeline.compileJit(hir, this.moduleName())

        if (!result.success) {
            this.state.skip(idx, "Photon 编译失败: " + result.errorMessage)
            return false
        }

        // 4. 将机器码写入 W^X 内存
        val funcName: String = this.funcName(idx)
        val entry: Int = this.backend.emitFunction(funcName, result.machineCodeHex)

        if (entry == 0) {
            this.state.skip(idx, "内存分配失败")
            return false
        }

        // 5. 注册到分派表
        this.backend.registerFunction(funcName, entry)
        this.state.insert(idx)  // 标记为已编译
        this.state.setDispatchEntry(idx, entry)

        return true
    }

    /// 调用已编译的原生函数
    fun nativeCall(idx: Int, args: String): String {
        if (!this.nativeMode) {
            return ""
        }

        // 1. 获取入口地址
        val funcName: String = this.funcName(idx)
        val entry: Int = this.backend.lookupFunction(funcName)

        if (entry == 0) {
            return ""
        }

        // 2. 通过 JitExec 调用
        // 注意：需要转换 args 格式为 JitValue ABI
        val argsValue: Long = this.encodeArgs(args)
        val result: Long = JitExec.callI64(entry as Long, argsValue)

        // 3. 转换结果为字符串
        return this.decodeResult(result as Int)
    }

    // ── 辅助方法 ──

    fun moduleName(): String { return "" }
    fun funcName(idx: Int): String { return "func" + idx }
    fun encodeArgs(args: String): Long { return 0 }
    fun decodeResult(result: Int): String { return "" }
}
```

#### 4.1.3 JitRelocation.aura — JIT 重定位处理

**目的**：处理 Photon 生成的机器码中的重定位记录，在写入 W^X 内存后修正绝对地址和相对跳转。

**背景**：Photon 的 X86Emitter 在生成机器码时会记录重定位（函数调用目标、数据引用等）。在 AOT 模式下由链接器修正；在 JIT 模式下需要在运行时手动修正。

```aura
// aura/compiler/aura/lang/compiler/jit/JitRelocation.aura

import aura.lang.native.Memory

/// 重定位条目
class Relocation {
    var offset: Int = 0      // 在机器码中的偏移
    var target: Int = 0      // 目标符号名（如 "func_42"）
    var type: Int = 0        // 0=RIP32, 1=ABS64, 2=PCREL32

    init(offset: Int, target: Int, relocType: Int) {
        this.offset = offset
        this.target = target
        this.type = relocType
    }
}

/// JIT 重定位处理器
class JitRelocation {

    /// 应用重定位：修正机器码中的地址
    fun applyRelocations(base: Int, codeSize: Int, relocs: List<Relocation>, dispatchTable: JitBackend): Unit {
        var i: Int = 0
        while (i < relocs.size) {
            val reloc: Relocation = relocs.get(i)

            if (reloc.type == 0) {
                // RIP32：修正相对跳转（call/jmp 的偏移）
                val absAddr: Int = this.resolveTarget(reloc.target, dispatchTable)
                if (absAddr != 0) {
                    val nextInsnAddr: Int = base + reloc.offset + 4
                    val relOffset: Int = absAddr - nextInsnAddr
                    Memory.write32((base + reloc.offset) as Long, relOffset as Int)
                }
            } else if (reloc.type == 1) {
                // ABS64：修正绝对地址（mov rax, imm64）
                val absAddr: Int = this.resolveTarget(reloc.target, dispatchTable)
                if (absAddr != 0) {
                    Memory.write64((base + reloc.offset) as Long, absAddr as Long)
                }
            }

            i = i + 1
        }
    }

    /// 解析目标符号名，返回绝对地址
    fun resolveTarget(target: Int, dispatchTable: JitBackend): Int {
        // 从分派表查找函数入口
        val funcName: String = "func" + target
        return dispatchTable.lookupFunction(funcName)
    }
}
```

#### 4.1.4 JitDriver.aura — JIT 统一驱动

**目的**：提供 JIT 的统一入口，封装编译、装载、调用的完整流程。

```aura
// aura/compiler/aura/lang/compiler/jit/JitDriver.aura

import aura.lang.compiler.jit.HirCache
import aura.lang.compiler.jit.JitBridge
import aura.lang.compiler.jit.JitRelocation
import aura.lang.compiler.backend.photon.JitBackend

/// JIT 驱动：统一入口
object JitDriver {

    /// 全局 JIT 桥接实例
    val bridge: JitBridge = JitBridge()

    /// 初始化 JIT（编译期调用）
    fun initNative(): Unit {
        bridge.enableNative()
    }

    /// 注册函数 HIR（编译期调用）
    fun registerHir(funcIdx: Int, hirData: String): Unit {
        GlobalHirCache.cache.put(funcIdx, hirData)
    }

    /// VM 调用钩子（运行时调用）
    fun callHook(idx: Int, args: String): String {
        return bridge.callHook(idx, args)
    }

    /// 查询 JIT 状态
    fun isNativeMode(): Boolean {
        return bridge.nativeMode
    }

    /// 查询已编译函数数
    fun compiledCount(): Int {
        return bridge.state.compiledCount()
    }
}
```

### 4.2 保留组件（不需要修改）

以下组件已完美实现，在新架构中保持不变：

| 组件 | 文件 | 行数 | 职责 | 原因 |
|------|------|------|------|------|
| JitState | `jit/JitState.aura` | 384 | 热点检测、白名单、编译顺序 | 字节码级 JIT 逻辑，与后端无关 |
| JitDispatch | `jit/JitDispatch.aura` | 392 | NATIVE/SKIP/DEFER 派发决策 | 字节码级派发逻辑，与后端无关 |
| JitAbi | `jit/JitAbi.aura` | 283 | JitValue ABI（13 个标签） | 共享调用约定，VM/JIT/AOT 通用 |
| JitUtil | `jit/JitUtil.aura` | 336 | 公共字符串工具 | 工具函数，与后端无关 |
| DispatchTable | `jit/DispatchTable.aura` | 123 | 分发表纯数据结构 | 纯数据，与后端无关 |

### 4.3 替换/移除组件

| 组件 | 当前状态 | 处置 | 替代 |
|------|----------|------|------|
| JitLower.aura | 字节码→.clif 文本 | **移除** | Photon 直接消费 HIR，不需要 .clif |
| JitOpt.aura | 7 个字节码优化 pass | **保留但降级** | Photon 有 SSA 级优化，字节码优化作为预优化 |
| JitCore.aura | 预解码 + 基线编译 | **保留但降级** | 预解码仍有用（白名单检查），编译由 Photon 负责 |
| JitRuntime.aura | W^X 描述 | **移除** | JitBackend 已实现 W^X |
| VmJitBridge.aura | VM-JIT 桥接（FFI） | **替换** | JitBridge.aura（纯 Aura） |
| jit_ffi.rs | FFI 边界（Rust） | **移除** | 不需要 FFI 边界 |
| cranelift feature | Cargo.toml | **移除** | 不需要 Cranelift |

### 4.4 Photon 后端需要的修改

#### 4.4.1 JitBackend.aura — 增强 W^X 内存管理

**当前问题**：
1. `writeCode` 逐字节写入（`Memory.write`），性能极差
2. 存根生成的重定位占位符（`00 00 00 00`）未修正
3. `emitFunction` 不处理重定位

**需要的修改**：
1. 增加批量写入方法（`writeCodeBlock`），使用 `Memory.copy` 代替逐字节写入
2. `emitFunction` 接受重定位列表参数，写入后修正地址
3. 增加 `flushExecutable()` 方法，确保所有写入的机器码已变为 RX 权限
4. 增加 `codeBuffer` 概念：先在 RW 缓冲区构建机器码，然后一次性写入 RX 区域

```text
修改后的 emitFunction 流程：
  1. 分配 RW 内存（allocateExecMemory）
  2. 写入机器码 hex → 字节（writeCodeBlock）
  3. 应用重定位（applyRelocations）
  4. 切换为 RX 权限（setExecutable）
  5. 返回入口地址
```

#### 4.4.2 PhotonPipeline.aura — 增强 compileEncodeOnly

**当前问题**：`compileEncodeOnly` 只返回 `machineCodeHex`，不返回重定位记录。

**需要的修改**：
1. `BackendResult` 增加 `relocations` 字段（重定位列表）
2. `compileEncodeOnly` 收集 X86Emitter 的重定位记录
3. 增加 `relocationCount` 字段

```text
BackendResult 增加字段：
  var relocations: List<Relocation> = []   // 重定位列表
  var relocationCount: Int = 0             // 重定位数量
  var entryOffset: Int = 0                 // 入口点偏移
  var dataSectionHex: String = ""          // 数据段 hex（.rdata/.data）
```

#### 4.4.3 X86Emitter.aura — 导出重定位记录

**当前状态**：X86Emitter 内部有重定位记录，但不对外暴露。

**需要的修改**：
1. 增加 `getRelocations()` 方法，返回重定位列表
2. 增加 `getEntryOffset()` 方法，返回入口点偏移
3. 增加 `getDataSectionHex()` 方法，返回数据段 hex

#### 4.4.4 PhotonPipeline.aura — 增加 JIT 专用优化级别

**当前问题**：JIT 编译需要快速（1–5ms），应使用较低优化级别。

**需要的修改**：
1. `compileJit` 接受 `optLevel` 参数（默认 0 或 1）
2. optLevel=0：跳过部分优化 pass（如 Peephole），换取编译速度
3. optLevel=1：完整优化（当前默认）

---

## 五、数据流

### 5.1 编译期数据流（Stage-2 自举）

```text
源码 (.aura)
  │
  ▼
Lexer → Parser → AST → Sema → HIR
  │
  ├──→ HirCache.put(funcIdx, HIR)     ← 缓存 HIR（新增）
  │
  ▼
MIR → Codegen → 字节码 (.auc)
  │
  ▼
AOT 后端 (Aot.aura) → LLVM IR → llc → clang → exe
  或
  Photon 后端 (PhotonPipeline) → COFF → lld-link → exe
```

### 5.2 运行期数据流（JIT 触发）

```text
VM 执行字节码
  │
  ▼ 函数调用
JitDriver.callHook(idx, args)
  │
  ├──→ JitDispatch.jitDecide(state, idx)
  │       │
  │       ├── "native" → JitExec.callI64(entry, args) → result
  │       ├── "skip"   → 回退解释器
  │       └── "defer"  → 检查阈值
  │                          │
  │                          ▼ 达阈值
  │                    tryCompile(idx)
  │                          │
  │                          ▼
  │                    GlobalHirCache.get(idx) → HIR
  │                          │
  │                          ▼
  │                    PhotonPipeline.compileJit(HIR)
  │                          │
  │                          ▼
  │                    machineCodeHex + relocations
  │                          │
  │                          ▼
  │                    JitBackend.emitFunction(hex, relocs)
  │                          │
  │                          ▼
  │                    W^X 内存（mmap → write → mprotect）
  │                          │
  │                          ▼
  │                    entry point + dispatch table
  │                          │
  │                          ▼
  │                    JitExec.callI64(entry, args) → result
  │
  └──→ 返回结果
```

### 5.3 完整自举 + JIT 数据流

```text
┌─ Stage-1（Rust 编译器） ──────────────────────────────────────┐
│ Main.aura → Rust 编译器 → aura-compiler-c2.exe                │
│ 包含：前端 + AOT + Photon + JIT 基础设施                      │
│ JIT 状态：vmMode=true（种子 VM，模拟）                         │
└───────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─ Stage-2（自举编译） ────────────────────────────────────────┐
│ aura-compiler-c2.exe 编译自身                                 │
│                                                              │
│ 编译期：                                                      │
│   HIR → HirCache.put（缓存）→ MIR → 字节码 → AOT/Photon     │
│                                                              │
│ 运行时（自举编译过程中）：                                     │
│   VM 执行字节码 → JitDriver.callHook                          │
│     → Photon.compileJit(HIR) → JitBackend.emitFunction       │
│     → JitExec.callI64 → 原生码执行                            │
│                                                              │
│ 产物：aura-compiler-native2.exe                               │
│ JIT 状态：vmMode=false（原生运行时，JIT 可用）                 │
└───────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─ Stage-3（编译用户程序） ─────────────────────────────────────┐
│ aura-compiler-native2.exe 编译用户程序                        │
│                                                              │
│ 编译期：同 Stage-2（HIR 缓存）                                │
│ 运行时：用户程序可使用 JIT                                    │
│                                                              │
│ 产物：user.exe（含 JIT 基础设施）                             │
│ JIT 状态：vmMode=false（JIT 可用）                             │
└───────────────────────────────────────────────────────────────┘
```

---

## 六、FFI 边界分析

### 6.1 新架构的 FFI 边界

新架构中，JIT 路径**完全不需要 Rust FFI 边界**。所有 JIT 操作都是纯 Aura 代码，仅依赖以下 native 原语：

| 原语 | 接口 | 用途 | Photon 降低方式 |
|------|------|------|-----------------|
| `Memory.alloc(n)` | `extern interface Memory` | 分配可执行内存 | syscall: mmap/VirtualAlloc |
| `Memory.free(addr)` | `extern interface Memory` | 释放内存 | syscall: munmap/VirtualFree |
| `Memory.mprotect(addr, len, prot)` | `extern interface Memory` | 切换内存权限 | syscall: mprotect/VirtualProtect |
| `Memory.write(addr, v)` | `extern interface Memory` | 写字节 | load/store 指令 |
| `Memory.write32(addr, v)` | `extern interface Memory` | 写 32 位 | load/store 指令 |
| `Memory.write64(addr, v)` | `extern interface Memory` | 写 64 位 | load/store 指令 |
| `Memory.copy(dst, src, n)` | `extern interface Memory` | 内存复制 | 循环 store |
| `JitExec.call0(entry)` | `extern interface JitExec` | 调用无参函数 | inttoptr + call |
| `JitExec.callI64(entry, arg0)` | `extern interface JitExec` | 调用单参函数 | inttoptr + call |

**关键**：这些 native 原语**已经在 AOT 发射器中实现**（`Emit.aura` 行 7132-8207），由 AOT 后端降低为 `inttoptr` + `call` 或 syscall 指令。不需要新增任何 Rust FFI 代码。

### 6.2 与旧 FFI 边界的对比

| 旧 FFI 函数 | 新实现方式 | 节省 |
|-------------|-----------|------|
| `jit_compile(clif_text)` | `PhotonPipeline.compileJit(HIR)` | 消除 Cranelift 依赖 |
| `jit_load(blob_b64)` | `JitBackend.emitFunction(hex, relocs)` | 消除 mmap_alloc Rust 代码 |
| `jit_call(entry_token, args)` | `JitExec.callI64(entry, args)` | 消除 transmute Rust 代码 |
| `register_dispatch(idx, token)` | `JitBackend.registerFunction(name, entry)` | 消除 Rust HashMap |
| `lookup_dispatch(idx)` | `JitBackend.lookupFunction(name)` | 消除 Rust HashMap |

**总计节省**：5 个 @native 函数 + ~1000 行 Rust 代码（`jit_ffi.rs` 394 行 + `cranelift_backend` ~1000 行）

---

## 七、HIR 缓存机制

### 7.1 设计考量

**为什么需要 HIR 缓存？**

Photon 后端以 HIR 为输入，但 JIT 在运行时触发。此时 HIR 已被 MIR 消费（HIR → MIR → Bytecode）。如果 JIT 要在运行时访问 HIR，必须在编译期缓存。

**替代方案对比**：

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **HIR 缓存** | 编译期保存 HIR，运行时读取 | 简单、直接、完整类型信息 | 内存占用（每函数一份 HIR 副本） |
| 字节码→HIR 反演 | 从字节码重建 HIR | 无需额外存储 | 复杂、信息丢失（类型信息在 MIR 阶段丢失） |
| 字节码→LIR 直连 | 跳过 HIR，直接字节码→LIR | 无缓存开销 | 需重写整个 Lowering，绕过 Photon |
| HIR→PHIR 文本 | 序列化 HIR 为 PHIR 文本 | 复用现有序列化器 | PHIR 解析器慢（~50ms/函数） |

**结论**：HIR 缓存方案最简单、最可靠、信息最完整。

### 7.2 HIR 序列化格式

HIR 需要序列化为字符串以便在 `HirCache` 中存储。有两个选择：

**选择 A：使用 PHIR 序列化器**（`PhirSerializer.aura`，445 行）
- 优点：已有实现，可复用
- 缺点：PHIR 是文本格式，解析慢（~50ms/函数），序列化/反序列化开销大

**选择 B：自定义紧凑序列化**
- 优点：紧凑、快速
- 缺点：需要新实现

**建议**：初期使用选择 A（PHIR），验证功能后优化为选择 B。

### 7.3 内存开销分析

以 Stage-2 自举为例（51 模块，57834 HIR 节点）：
- 每个 HIR 节点约 100-200 字节（PHIR 文本表示）
- 总 HIR 大小：57834 × 150 ≈ 8.7 MB
- 可接受（编译器本身 ~45 MB）

---

## 八、重定位处理

### 8.1 为什么 JIT 需要重定位

Photon 的 X86Emitter 生成机器码时，函数调用和数据引用使用占位符（如 `00 00 00 00`），需要链接器修正为实际地址。

在 AOT 模式下：
```text
X86Emitter → 机器码 + 重定位记录
  → PhotonObjectWriter → COFF 目标文件（含重定位表）
  → lld-link → 修正重定位 → 可执行文件
```

在 JIT 模式下：
```text
X86Emitter → 机器码 + 重定位记录
  → JitBackend.emitFunction → W^X 内存（mmap）
  → JitRelocation.applyRelocations → 修正重定位
  → setExecutable → 可执行
```

### 8.2 重定位类型

| 类型 | 编码 | 场景 | 修正方式 |
|------|------|------|----------|
| RIP32 | `0x00` | `call rel32` / `jmp rel32` | `offset = target - (base + insn_end)` |
| ABS64 | `0x01` | `mov rax, imm64` | `write64(offset, target_addr)` |
| PCREL32 | `0x02` | `lea rax, [rip + disp32]` | `disp32 = target - (base + insn_end)` |
| DATA_REF | `0x03` | 字符串常量、GOT 条目 | `write64(offset, data_addr)` |

### 8.3 多函数链接

JIT 编译的函数之间可能互相调用（如递归、互递归）。处理方式：

```text
函数 A 调用函数 B：
  1. 编译函数 B → entry_B
  2. 注册 entry_B 到分派表
  3. 编译函数 A → 生成 call 指令（带重定位）
  4. 修正重定位：target = entry_B

或者（间接调用）：
  1. 生成 call stub：jmp [dispatchTable + slot * 8]
  2. 修正 dispatchTable[slot] = entry_B
```

**建议**：使用直接调用（ABS64 重定位），因为 JIT 编译时已知所有函数的入口地址。

### 8.4 数据段处理

JIT 代码可能引用字符串常量、常量池等数据。处理方式：

```text
1. Photon 生成数据段（.rdata）
2. JitBackend 分配 RW 数据内存
3. 写入数据
4. 修正代码中的数据引用重定位
5. 数据段可保持 RW（不需要 RX）
```

---

## 九、W^X 内存管理

### 9.1 当前实现分析

`JitBackend.aura` 的 W^X 实现：

```text
allocateExecMemory(size)
  → Memory.alloc(alignedSize)          // mmap/VirtualAlloc (RW)
  → 返回 base 地址

writeCode(base, offset, hex)
  → 逐字节 Memory.write                // 🔴 性能极差（每字节一次 syscall）

setExecutable(base, size)
  → Memory.mprotect(base, size, RX)   // mprotect/VirtualProtect

freeExecMemory(base, size)
  → Memory.free(base)                  // munmap/VirtualFree
```

**问题**：`writeCode` 逐字节写入，每个字节一次 `Memory.write` 调用。对于一个 100 字节的函数，需要 100 次内存写入调用。

### 9.2 优化方案

**方案 A：批量写入**（推荐）
```aura
// 将 hex 字符串转换为字节缓冲区，然后一次写入
fun writeCodeBlock(base: Int, offset: Int, bytes: Byte[]): Unit {
    val count: Int = bytes.length
    var i: Int = 0
    while (i < count) {
        Memory.write((base + offset + i) as Long, bytes[i])
        i = i + 1
    }
}
```

**方案 B：使用 Memory.copy**（更优）
```aura
// 将 hex 解码到堆缓冲区，然后 Memory.copy 一次复制
fun writeCodeBlock(base: Int, offset: Int, hex: String): Unit {
    val buf: Long = Memory.alloc(hex.length / 2)
    // 解码 hex 到 buf
    var i: Int = 0
    while (i < hex.length) {
        val b: Byte = hexToByte(hex, i)
        Memory.write(buf + (i / 2), b)
        i = i + 2
    }
    // 一次复制到目标地址
    Memory.copy((base + offset) as Long, buf, hex.length / 2)
    Memory.free(buf)
}
```

**方案 C：双缓冲**（最优）
```text
1. 在 RW 缓冲区构建完整机器码（含重定位修正）
2. 一次性 mprotect 为 RX
3. 不需要二次写入
```

**建议**：初期使用方案 A（简单），优化阶段升级为方案 C。

### 9.3 内存生命周期

```text
函数生命周期：
  1. 首次 JIT 编译 → allocateExecMemory → writeCode → setExecutable
  2. 多次调用 → 直接 executeNative（无额外分配）
  3. 去优化 → deoptimize → 分派表条目置零（内存保持 RX，标记为不可用）
  4. 程序退出 → freeExecMemory → munmap/VirtualFree
```

**注意**：JIT 内存不需要频繁释放（函数通常终身有效），所以 `freeExecMemory` 仅在程序退出时调用。

---

## 十、自举集成

### 10.1 自举链修改

当前自举链（Stage-1/2/3）全走 AOT（LLVM）路径。新架构需要修改：

**Stage-2 修改**：
- 编译期：在 HIR 生成后缓存到 `GlobalHirCache`
- 运行时：JIT 基础设施在原生运行时可用（`enableNativeExec` 已调用）
- 自举编译过程中，VM 执行热点函数时自动触发 JIT

**Stage-3 修改**：
- 用户程序编译时同样缓存 HIR
- 用户程序运行时 JIT 可用

### 10.2 初始化流程

```text
程序启动
  │
  ▼
Runtime 初始化
  │
  ├──→ Memory 分配器初始化
  ├──→ GC 初始化
  ├──→ VM 初始化
  │
  ├──→ JitDriver.initNative()          ← 新增
  │     → JitBackend.enableNativeExec()
  │     → JitBridge.enabled = true
  │
  ▼
VM 开始执行
  │
  ▼ 函数调用
JitDriver.callHook(idx, args)
  │
  ├──→ 已编译 → JitExec.callI64 → 原生码
  └──→ 未编译 → 检查阈值 → 可能触发编译
```

### 10.3 vmMode 过渡策略

**当前问题**：`JitBackend.vmMode` 默认 `true`，种子 VM 下无法关闭（`Memory` 不可解析）。

**解决方案**：
1. 编译期：`JitBackend` 保持 `vmMode=true`（种子 VM 安全）
2. 原生运行时启动时：调用 `enableNativeExec()` 关闭 `vmMode`
3. 此后 JIT 真正可用

```text
种子 VM（vmMode=true）：
  JitBackend.allocateExecMemory → 返回模拟地址
  JitBackend.writeCode → 直接 return
  JitBackend.executeNative → 返回 0

原生运行时（vmMode=false）：
  JitBackend.allocateExecMemory → Memory.alloc（真实 mmap）
  JitBackend.writeCode → Memory.write（真实写入）
  JitBackend.executeNative → JitExec.callI64（真实调用）
```

### 10.4 自举验证

自举完成后，需要验证 JIT 自举产物的正确性：

```text
验证步骤：
  1. 用 AOT 编译编译器 → aura-compiler-aot.exe
  2. 用 Photon 编译编译器 → aura-compiler-photon.exe
  3. 两个产物编译相同用户程序 → 比较结果
  4. JIT 编译的函数执行结果 → 与 VM 解释结果比较
  5. JIT 编译延迟 → 测量（目标 < 5ms/函数）
```

---

## 十一、分阶段实施计划

### Phase 1：基础设施验证（1 周）

**目标**：验证 Photon 后端能在原生运行时生成可执行机器码。

| 任务 | 文件 | 工作量 | 依赖 |
|------|------|--------|------|
| 1.1 修改 `compileEncodeOnly` 返回重定位 | `PhotonPipeline.aura` | 1 天 | 无 |
| 1.2 增加 `getRelocations`/`getEntryOffset` | `X86Emitter.aura` | 1 天 | 无 |
| 1.3 增强 `JitBackend.emitFunction` 支持重定位 | `JitBackend.aura` | 1 天 | 1.1 |
| 1.4 优化 `writeCode` 为批量写入 | `JitBackend.aura` | 1 天 | 无 |
| 1.5 原生运行时测试程序 | `tests/photon/jit_native_test.aura` | 2 天 | 1.1-1.4 |

**验收**：原生运行时下 `JitBackend.emitFunction` 返回非零入口地址，`executeNative` 返回正确结果。

### Phase 2：HIR 缓存（1 周）

**目标**：实现 HIR 缓存机制，编译期缓存 HIR 供 JIT 使用。

| 任务 | 文件 | 工作量 | 依赖 |
|------|------|--------|------|
| 2.1 实现 `HirCache.aura` | `jit/HirCache.aura`（新） | 1 天 | 无 |
| 2.2 HIR 序列化（PHIR 格式） | `jit/HirCache.aura` | 2 天 | 2.1 |
| 2.3 集成到编译管线 | `Codegen.aura` / `Aot.aura` | 1 天 | 2.1-2.2 |
| 2.4 HIR 缓存测试 | `tests/jit/hir_cache_test.aura`（新） | 2 天 | 2.1-2.3 |

**验收**：编译程序后 `GlobalHirCache.cache.size > 0`，`cache.get(funcIdx)` 返回非空字符串。

### Phase 3：JIT 桥接（1.5 周）

**目标**：实现 VM-JIT 桥接，连接 VM 派发到 Photon JIT。

| 任务 | 文件 | 工作量 | 依赖 |
|------|------|--------|------|
| 3.1 实现 `JitRelocation.aura` | `jit/JitRelocation.aura`（新） | 2 天 | Phase 1 |
| 3.2 实现 `JitBridge.aura` | `jit/JitBridge.aura`（新） | 3 天 | Phase 2 |
| 3.3 实现 `JitDriver.aura` | `jit/JitDriver.aura`（新） | 1 天 | 3.1-3.2 |
| 3.4 集成到 VM 调用路径 | `Vm.aura` / `VmRunner.aura` | 1 天 | 3.1-3.3 |
| 3.5 JIT 桥接测试 | `tests/jit/jit_bridge_test.aura`（新） | 3 天 | 3.1-3.4 |

**验收**：VM 调用热点函数时自动触发 JIT 编译，JIT 编译后的函数执行结果与 VM 解释一致。

### Phase 4：移除旧路径（1 周）

**目标**：移除 Cranelift 相关代码，清理旧 FFI 边界。

| 任务 | 文件 | 工作量 | 依赖 |
|------|------|--------|------|
| 4.1 移除 `JitLower.aura` | `jit/JitLower.aura` | 0.5 天 | Phase 3 |
| 4.2 移除 `JitRuntime.aura` | `jit/JitRuntime.aura` | 0.5 天 | Phase 3 |
| 4.3 替换 `VmJitBridge.aura` | `vm/VmJitBridge.aura` | 1 天 | Phase 3 |
| 4.4 移除 `jit_ffi.rs` | `bootstrap/jit_ffi.rs` | 0.5 天 | Phase 3 |
| 4.5 移除 Cranelift 依赖 | `compiler/Cargo.toml` | 0.5 天 | Phase 3 |
| 4.6 更新文档 | `docs/pure_aura_jit/` | 1 天 | Phase 3-4 |
| 4.7 回归测试 | 全部测试 | 2 天 | Phase 3-4 |

**验收**：删除 Cranelift 依赖后，所有测试通过，JIT 功能正常。

### Phase 5：自举验证（1.5 周）

**目标**：验证自举链中 JIT 的正确性和性能。

| 任务 | 工作量 | 依赖 |
|------|--------|------|
| 5.1 Stage-2 自举（含 JIT） | 2 天 | Phase 4 |
| 5.2 Stage-2 自举验证 | 2 天 | 5.1 |
| 5.3 Stage-3 用户程序编译（含 JIT） | 2 天 | 5.1 |
| 5.4 性能基准测试 | 2 天 | 5.1-5.3 |
| 5.5 稳定性测试（长时间运行） | 2 天 | 5.1-5.3 |

**验收**：
- Stage-2 自举成功（JIT 在自举过程中被使用）
- JIT 编译延迟 < 5ms/函数
- JIT 执行性能 ≥ VM 解释的 10x
- 无内存泄漏、无崩溃

### Phase 6：优化与完善（2 周）

**目标**：优化 JIT 性能和功能完整性。

| 任务 | 工作量 | 依赖 |
|------|--------|------|
| 6.1 编译速度优化（optLevel=0 快速编译） | 3 天 | Phase 5 |
| 6.2 W^X 双缓冲优化 | 2 天 | Phase 5 |
| 6.3 去优化（deopt）完整实现 | 3 天 | Phase 5 |
| 6.4 类型反馈/投机内联（可选） | 5 天 | Phase 5 |
| 6.5 完整测试套件 | 3 天 | Phase 5 |

### 总工作量估算

| 阶段 | 工期 | 人力 |
|------|------|------|
| Phase 1：基础设施验证 | 1 周 | 1 人 |
| Phase 2：HIR 缓存 | 1 周 | 1 人 |
| Phase 3：JIT 桥接 | 1.5 周 | 1 人 |
| Phase 4：移除旧路径 | 1 周 | 1 人 |
| Phase 5：自举验证 | 1.5 周 | 1 人 |
| Phase 6：优化与完善 | 2 周 | 1 人 |
| **总计** | **8 周** | **1 人** |

---

## 十二、测试策略

### 12.1 测试层级

```text
Layer 1: 单元测试（每个组件独立测试）
  - JitBackend 内存管理测试
  - JitRelocation 重定位测试
  - HirCache 缓存测试
  - JitBridge 桥接测试

Layer 2: 集成测试（组件协作测试）
  - HIR → Photon → 机器码 → W^X → 执行
  - VM → JitDriver → JIT 编译 → 执行
  - 热点检测 → 编译 → 调用

Layer 3: 系统测试（端到端）
  - Stage-2 自举（含 JIT）
  - 用户程序编译 + JIT 执行
  - 性能基准

Layer 4: 回归测试
  - 与旧 Cranelift JIT 结果对比
  - 与 VM 解释结果对比
  - 与 AOT 编译结果对比
```

### 12.2 关键测试用例

#### JIT 基础设施测试（Phase 1）
```text
1. W^X 内存分配 → 写入 → 执行
2. 重定位修正（RIP32、ABS64）
3. 多函数分派表
4. 去优化（分派表条目置零）
```

#### HIR 缓存测试（Phase 2）
```text
1. 编译期缓存 HIR → 缓存 size > 0
2. 运行时读取 HIR → 反序列化成功
3. 多函数缓存 → 各函数独立
4. 缓存完整性 → HIR 节点数一致
```

#### JIT 桥接测试（Phase 3）
```text
1. VM 调用未编译函数 → 回退解释器
2. VM 调用达阈值函数 → 触发 JIT 编译
3. JIT 编译后调用 → 走原生码
4. JIT 结果与 VM 结果一致
5. 递归函数 JIT → 互递归支持
```

#### 自举验证测试（Phase 5）
```text
1. Stage-2 自举成功
2. 自举产物功能一致
3. JIT 在自举过程中被使用（编译延迟统计）
4. 用户程序 JIT 加速效果
```

#### 性能基准测试（Phase 5-6）
```text
| 测试用例 | 目标 |
|----------|------|
| sum(60000) JIT vs VM | JIT 快 ≥ 50x |
| fib(25) JIT vs VM | JIT 快 ≥ 30x |
| JIT 编译延迟 | < 5ms/函数 |
| JIT 编译延迟 vs Cranelift | 差异 < 2x |
```

---

## 十三、风险与缓解

### 13.1 技术风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Photon 生成的机器码有 bug | 中 | 高 | 对比 AOT 后端结果；分阶段测试 |
| 重定位修正不正确 | 中 | 高 | 单元测试覆盖所有重定位类型 |
| HIR 序列化/反序列化错误 | 低 | 高 | 使用现有 PHIR 序列化器（已验证） |
| JIT 编译延迟过长 | 中 | 中 | optLevel=0 快速编译；跳过部分优化 |
| 内存泄漏（W^X 内存未释放） | 中 | 中 | 程序退出时统一释放；调试工具 |
| `vmMode` 过渡失败 | 低 | 高 | 种子 VM 下不触发 JIT；原生运行时才启用 |
| 自举不一致（JIT 产物不同） | 低 | 中 | JIT 是运行时优化，不影响编译产物 |
| `Memory.copy` 在 JIT 路径不可用 | 低 | 中 | 回退到逐字节写入（方案 A） |

### 13.2 架构风险

| 风险 | 描述 | 缓解措施 |
|------|------|----------|
| Photon 编译速度不如 Cranelift | Photon 是纯 Aura 实现，可能比 Cranelift（Rust）慢 | JIT 编译在后台线程进行；optLevel=0 快速路径 |
| HIR 缓存增加内存占用 | 每函数一份 HIR 副本 | 可接受（~100KB/函数 vs 编译器 45MB） |
| 自举链复杂化 | 新增 HIR 缓存 + JIT 桥接增加自举复杂度 | 分阶段验证；每阶段独立可测试 |
| 回退到旧路径 | 如果新方案失败 | 旧 Cranelift 路径代码保留在 git 历史中 |

### 13.3 进度风险

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|----------|
| Photon 后端 bug 修复耗时 | 中 | 中 | Phase 1 先验证基础设施，尽早暴露问题 |
| 自举验证发现问题 | 中 | 高 | Phase 5 提前开始，不等到最后 |
| 性能不达标 | 低 | 中 | Phase 6 专门优化；可接受一定性能损失 |

---

## 十四、与旧方案的对比总结

### 14.1 架构对比

```text
旧方案（Cranelift JIT）：                    新方案（Photon JIT）：

字节码 → JitLower → .clif 文本              字节码 → (VM 执行)
  → @native jit_compile(clif)                  │
    → Cranelift (Rust)                         │ 热点检测
      → blob_b64                               │
  → @native jit_load(blob)                     │
    → mmap/mprotect (Rust)                     ▼
  → @native jit_call(entry)                HIR 缓存 → Photon.compileJit(HIR)
    → transmute call (Rust)                    → machineCodeHex + relocations
                                               → JitBackend.emitFunction(hex)
                                               → W^X 内存 (Aura)
                                               → JitExec.callI64 (Aura)

依赖：Rust + Aura + Cranelift + LLVM         依赖：Aura + LLVM (AOT) + Photon
FFI 边界：5 个 @native 函数                   FFI 边界：0 个
```

### 14.2 量化对比

| 指标 | 旧方案 | 新方案 | 变化 |
|------|--------|--------|------|
| FFI 函数数量 | 5 个 | 0 个 | -100% |
| Rust JIT 代码 | ~1400 行 | 0 行 | -100% |
| Cranelift 依赖 | cranelift 0.116 | 无 | 消除 |
| 语言种类 | Rust + Aura + LLVM + Cranelift | Aura + LLVM + Photon | -1 |
| JIT 编译 IR | Clif 文本 IR | HIR（原生） | 更简洁 |
| 可自举 | 否 | 是 | ✅ |
| W^X 内存 | Rust `mmap_alloc` | Aura `JitBackend` | ✅ |
| 分派表 | Rust HashMap | Aura `JitBackend` | ✅ |
| 原生调用 | Rust `transmute` | Aura `JitExec` | ✅ |
| JIT 编译延迟 | ~1-5ms | 估计 5-20ms | ⚠️ 稍慢（可优化） |

### 14.3 收益总结

1. **完全消除 Cranelift 依赖**——JIT 机器码生成完全在 Aura 内部
2. **消除 FFI 边界**——不需要 `jit_ffi.rs`，不需要 @native 函数
3. **自举可行**——JIT 基础设施全部纯 Aura，可被自举编译
4. **代码简化**——移除 ~1400 行 Rust 代码
5. **语言统一**——从 4 种语言（Rust+Aura+LLVM+Cranelift）减少到 3 种（Aura+LLVM+Photon）
6. **安全改进**——W^X 内存管理在 Aura 侧，可审计

### 14.4 代价

1. **JIT 编译延迟可能增加**——Photon 是纯 Aura 实现，可能比 Cranelift（Rust）慢 2-5x
2. **开发工作量**——8 周（vs 旧方案 P2 阶段 4 周）
3. **内存占用增加**——HIR 缓存（~100KB/函数）

---

## 十五、决策记录

### D1：用 Photon 替换 Cranelift

- **决策**：用 Photon 后端替换 Cranelift 作为 JIT 机器码后端
- **原因**：Photon 自包含、纯 Aura、可自举；Cranelift 是 Rust crate，必须保留 FFI 边界
- **替代方案**：继续用 Cranelift + FFI 边界
- **影响**：消除 5 个 @native 函数 + ~1400 行 Rust 代码
- **风险**：Photon 编译速度可能不如 Cranelift

### D2：HIR 缓存而非字节码反演

- **决策**：编译期缓存 HIR，运行时 JIT 读取缓存
- **原因**：简单、直接、信息完整；字节码反演复杂且信息丢失
- **替代方案**：字节码→HIR 反演、字节码→LIR 直连
- **影响**：内存占用增加 ~100KB/函数
- **风险**：HIR 序列化/反序列化可能出错

### D3：分阶段实施

- **决策**：6 个阶段，每阶段独立可测试
- **原因**：降低风险，每阶段可验证、可回退
- **替代方案**：一次性大改
- **影响**：总工期 8 周
- **风险**：阶段间依赖可能导致进度延迟

### D4：保留字节码级 JIT 前端

- **决策**：保留 JitState/JitDispatch/JitAbi（字节码级），替换 JitLower/JitRuntime（IR 发射/W^X）
- **原因**：JitState 的热点检测、白名单、编译顺序逻辑与后端无关；JitLower 生成 .clif，需要替换为 Photon 路径
- **替代方案**：全部重写
- **影响**：减少 ~1500 行需要重写的代码
- **风险**：JitState 的字节码级白名单可能与 Photon 的 HIR 级优化不兼容

### D5：JitBackend 增强而非重写

- **决策**：增强现有 `JitBackend.aura`（加批量写入、重定位支持），不重写
- **原因**：JitBackend 已有完整的 W^X、分派表、存根生成逻辑，只需小幅增强
- **替代方案**：重写 JitBackend
- **影响**：减少工作量
- **风险**：JitBackend 的设计假设可能需要调整

---

## 十六、附录

### A. 文件变更清单

#### 新增文件
| 文件 | 用途 | 阶段 |
|------|------|------|
| `aura/.../jit/HirCache.aura` | HIR 缓存 | Phase 2 |
| `aura/.../jit/JitBridge.aura` | VM-JIT 桥接（替换 VmJitBridge） | Phase 3 |
| `aura/.../jit/JitRelocation.aura` | 重定位处理 | Phase 3 |
| `aura/.../jit/JitDriver.aura` | JIT 统一驱动 | Phase 3 |
| `tests/jit/hir_cache_test.aura` | HIR 缓存测试 | Phase 2 |
| `tests/jit/jit_bridge_test.aura` | JIT 桥接测试 | Phase 3 |
| `tests/jit/jit_relocation_test.aura` | 重定位测试 | Phase 3 |
| `tests/photon/jit_native_test.aura` | 原生 JIT 测试 | Phase 1 |

#### 修改文件
| 文件 | 修改内容 | 阶段 |
|------|----------|------|
| `PhotonPipeline.aura` | `compileEncodeOnly` 返回重定位 | Phase 1 |
| `X86Emitter.aura` | 增加 `getRelocations`/`getEntryOffset` | Phase 1 |
| `JitBackend.aura` | 增强 `emitFunction`（重定位）、批量写入 | Phase 1 |
| `Codegen.aura` / `Aot.aura` | 集成 HIR 缓存 | Phase 2 |
| `Vm.aura` / `VmRunner.aura` | 集成 JitDriver.callHook | Phase 3 |

#### 移除文件
| 文件 | 原因 | 阶段 |
|------|------|------|
| `jit/JitLower.aura` | 生成 .clif，被 Photon 替换 | Phase 4 |
| `jit/JitRuntime.aura` | W^X 描述，被 JitBackend 替换 | Phase 4 |
| `vm/VmJitBridge.aura` | FFI 桥接，被 JitBridge 替换 | Phase 4 |
| `bootstrap/jit_ffi.rs` | FFI 边界，不再需要 | Phase 4 |

#### Cargo.toml 修改
| 文件 | 修改内容 | 阶段 |
|------|----------|------|
| `compiler/Cargo.toml` | 移除 `cranelift` 依赖，移除 `jit` feature | Phase 4 |

### B. 关键接口定义

#### HirCache
```aura
class HirCache {
    fun put(funcIdx: Int, hirData: String): Unit
    fun get(funcIdx: Int): String
    fun has(funcIdx: Int): Boolean
    fun size(): Int
    fun clear(): Unit
}
```

#### JitBridge
```aura
class JitBridge {
    fun enableNative(): Unit
    fun callHook(idx: Int, args: String): String
    fun tryCompile(idx: Int): Boolean
    fun nativeCall(idx: Int, args: String): String
    var enabled: Boolean
    var nativeMode: Boolean
    var threshold: Int
}
```

#### JitDriver
```aura
object JitDriver {
    fun initNative(): Unit
    fun registerHir(funcIdx: Int, hirData: String): Unit
    fun callHook(idx: Int, args: String): String
    fun isNativeMode(): Boolean
    fun compiledCount(): Int
}
```

#### JitRelocation
```aura
class Relocation {
    var offset: Int
    var target: Int
    var type: Int
}

class JitRelocation {
    fun applyRelocations(base: Int, codeSize: Int, relocs: List<Relocation>, dt: JitBackend): Unit
    fun resolveTarget(target: Int, dt: JitBackend): Int
}
```

### C. 术语表

| 术语 | 说明 |
|------|------|
| JIT | Just-In-Time 编译，运行时编译 |
| AOT | Ahead-Of-Time 编译，编译期编译 |
| HIR | High-level Intermediate Representation，高层中间表示 |
| MIR | Mid-level Intermediate Representation，中层中间表示 |
| LIR | Low-level Intermediate Representation，底层中间表示 |
| SSA | Static Single Assignment，静态单赋值 |
| PHIR | Photon IR，Photon 中间表示 |
| HAT | HIR-Advanced Text IR，SSA 文本 IR |
| Clif | Cranelift IR，Cranelift 中间表示 |
| COFF | Common Object File Format，目标文件格式 |
| W^X | Write XOR Execute，写时不可执行安全策略 |
| mmap | 内存映射，分配虚拟内存 |
| mprotect | 修改内存页保护权限 |
| RIP | Instruction Pointer，指令指针寄存器 |
| 重定位 | Relocation，修正机器码中的地址引用 |
| 去优化 | Deoptimization，从 JIT 码回退到解释器 |
| 自举 | Bootstrap，用自身编译自身 |
