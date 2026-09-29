# Aura 内存模型选型结论与 ARC 增强路线

> 系统级内存管理方案评估 — 选型结论与演进规划

---

## 一、背景

Aura 是 **NovaOS 的系统级脚本语言**，核心设计原则：

- **性能优先**：栈分配、零成本抽象、无 GC
- **安全可控**：空安全、类型安全、内存安全（ARC）
- **Kotlin 兼容**：语法 100% 对齐 Kotlin
- **系统脚本化**：既有脚本的开发效率，又有系统级的执行效率

当前已选择 ARC（Automatic Reference Counting），P7 已全链路实现。本文档给出选型结论、各模式现状评估、GC 代码清理建议和增强路线。

---

## 二、选型结论

### 2.1 一句话结论

> **ARC 是正确选择**。它比 GC 更确定（无暂停），比手动更安全（无泄漏），比所有权更友好（零语法侵入）。当前应坚持 ARC，优先补充 Weak 引用和快速路径优化。

### 2.2 为什么不选其他方案

| 方案 | 不选的核心原因 |
|------|-------------|
| **集中手动** | 摧毁脚本语言易用性，与"系统脚本化"目标矛盾 |
| **GC** | 暂停不可预测，违背"无 GC"原则；当前 ArcHeap 的 Map 存储不适合高效标记遍历 |
| **ARC+GC 混合** | 需要重构 ArcHeap 存储结构才能支持 GC 标记遍历，实现成本高 |
| **所有权** | 与 Kotlin 语法不兼容；开发效率代价过高；VM 架构不兼容 |

### 2.3 ARC 满足设计原则

| 设计原则 | ARC |
|---------|-----|
| 性能优先、栈分配 | ✅ ARC 3-5% 开销可接受，逃逸分析可将非逃逸对象栈分配 |
| 无 GC | ✅ 无暂停，回收确定性 |
| Kotlin 100% 兼容 | ✅ 完全透明，不改变语法 |
| 系统脚本化 | ✅ 低延迟确定性回收 |
| 自举编译 | ✅ 已在编译器全链路实现 |

---

## 三、各模式 ARC 现状评估

### 3.1 总体结论

> **三个模式统一使用 ARC，不切换内存模型。** 差异在优化优先级，不在模型选择。

| 模式 | ARC 状态 | 当务之急 |
|------|---------|---------|
| **VM** | ✅ 完整 | 补完 `@weak` 语法（VM 指令层已就绪） |
| **JIT** | 🔴 **被忽略** | 将 ARC no-op 改为 `call` 降级或 VM 代理 |
| **AOT (LLVM)** | ✅ 完整 | 快速路径 + 逃逸分析增强（微服务关键） |
| **AOT (Photon)** | 🟡 **部分** | 将 stub `retain`/`release` 改为 `call Memory_arc*` |

### 3.2 VM — 保持 ARC ✅

- 当前实现最完善，单线程无原子开销
- Weak 引用指令已在 VM 中实现（`Instr::WeakRef`/`WeakGet`），只差语言层 `@weak` 注解和标准库封装
- 建议：补完 `@weak` 语法 + Weak 引用表即可

### 3.3 JIT — 保持 ARC，但必须修复 🔴

- **问题**：`JitLower.aura:417-419` 将所有 ARC 指令（`Retain`/`Release`/`IncRef`/`DecRef`/`DropRef`）编译为空操作
- **建议**：JIT 层将 `Retain`/`Release` 降级为 `call aura_arc_increment/decrement`（与 AOT LLVM 路径一致）；若引导 JIT 不支持 `call` 指令，则通过 VM 层代理（JIT 执行前 VM 执行 retain，执行后 VM 执行 release）

### 3.4 AOT (LLVM) — 保持 ARC，优先优化 🔴→✅

用于**系统微服务**的优化优先级：

| 优先级 | 优化项 | 原因 |
|--------|-------|------|
| **P0** | 快速路径（rc=1 非原子操作） | 微服务多线程共享对象时，`LOCK INC/DEC` 内存屏障开销大 |
| **P0** | 逃逸分析增强 | 大量短生命周期对象（请求上下文、序列化缓冲），逃逸分析可栈分配，完全绕过 ARC |
| **P1** | 批量操作 | 链式赋值场景，批量合并原子操作减少内存屏障次数 |
| **P1** | Weak 引用 | 大量事件监听器/观察者模式，需 Weak 打破循环 |
| **P2** | Arena 分配器集成 | 按请求批量分配/释放，绕过逐个 ARC |

### 3.5 AOT (Photon) — 保持 ARC，先修复 🔴

- **问题**：`emitRetain`/`emitRelease`（`PhotonRuntime.aura:2230-2243`）是 stub——直接返回原对象，无实际引用计数
- **已具备**：`Memory_arcIncrement`/`Memory_arcDecrement` 有完整的 LOCK INC/DEC 实现
- **建议**：将 `emitRetain` 降级为 `call Memory_arcIncrement`，`emitRelease` 降级为 `call Memory_arcDecrement`（与 LLVM AOT 一致）
- **注意**：Photon 后端目标是自举编译器和小型工具，系统微服务应优先使用 LLVM AOT 路径

### 3.6 系统微服务的补充手段

对于极端热路径，**不切换内存模型**，而是提供"逃逸通道"：

1. **C FFI 绕过 ARC**（已有支持）：`native fun syscall_read(fd, buf, len)`
2. **Value 类型 / struct**：小数据用 tagged value 存储在寄存器/栈上，完全不经过 ARC
3. **Arena 分配器**：按请求批量分配，请求结束时整批回收，不走 ARC

---

## 四、GC 代码清理建议

### 4.1 核心结论

> **所有 GC 代码均为死代码，应当删除。** GC 在代码库中完全断开——没有任何代码路径从"触发 GC"到"执行回收"形成闭环。

### 4.2 GC 代码清单与处置

| 代码 | 位置 | 状态 | 处置 |
|------|------|------|------|
| `GcHeap`（Aura 语言版） | `aura/compiler/.../gc/Gc.aura` | 有实现但**无调用者** | **删除** |
| `MarkSweep` | `aura/compiler/.../gc/MarkSweep.aura` | 委托 GcHeap 但无调用者 | **删除** |
| `Incremental` | `aura/compiler/.../gc/Incremental.aura` | 空壳 phantom，无 GC 逻辑 | **删除** |
| `Concurrent` | `aura/compiler/.../gc/Concurrent.aura` | 空壳 phantom，无 GC 逻辑 | **删除** |
| `GcTrigger` | `aura/compiler/.../runtime/GcTrigger.aura` | 空壳 phantom，无调用者 | **删除** |
| `GC` 对象 | `aura/core/aura/lang/native/GC.aura` | `collect()` 是 TODO 空壳 | **删除**（保留同文件的 `ARC` 对象） |
| `gc` 字段 + `gcAlloc()`/`gcCollect()`/`gcThreshold` | `aura/compiler/.../vm/VmInstance.aura` | 创建后从未使用 | **删除** |
| `GC_MARK`/`GC_SWEEP` 指令 | `aura/compiler/.../vm/Opcodes.aura` | 从未发射、从未处理 | **删除** |
| Rust `GcHeap` | `seed/compiler/src/bootstrap/runtime.rs` | seed 二进制不用 | **删除**（保留 `Coroutine` 和 `memory` 模块） |

### 4.3 保留项

- **`ARC` 对象**（`GC.aura` 中的 `ARC` 单例）——有实际的 `retain()`/`release()` 实现，是 ARC 链路的组成部分，应保留（可迁移到独立的 `ARC.aura` 文件）

### 4.4 删除收益

1. **消除歧义**：代码库中只有一个内存模型（ARC），不再有"两个模型共存但只有一个生效"的混淆
2. **消除 bug 源头**：`SsaBuilder` 中 `GC.isEnabled()` 导致单例字段访问降级为 `@field(0, mem)` 的问题不再存在
3. **减少维护负担**：约 600 行死代码从代码库中移除
4. **减少认知负担**：新人看代码不会误以为有两个内存系统

---

## 五、ARC 增强路线

### 5.1 总体规划

```
Phase 0（立即）：清理 + 修复
    ├── GC 死代码删除                              — P0
    ├── JIT ARC no-op 修复                          — P0
    └── Photon stub retain/release 修复             — P0

Phase 1（本月）：正确性修复
    ├── 5.2 Weak 引用（打破循环引用）                 — P0
    └── 5.3 ARC 批量操作优化                         — P1

Phase 2（本季度）：性能优化
    ├── 5.4 快速路径（rc=1 非原子操作）               — P1
    ├── 5.5 冗余 Retain/Release 消除增强              — P1
    └── 5.6 逃逸分析增强                              — P2

Phase 3（长期）：安全网
    ├── 5.7 运行时泄漏检测增强                         — P2
    ├── 5.8 Weak 引用容器                             — P2
    └── 5.9 （可选）极低频率 GC 安全网                  — P3
```

### 5.2 Weak 引用（Phase 1 — P0）

**问题**：没有 Weak 引用，闭包捕获自身、双向关联对象会产生不可回收的循环引用泄漏。

**方案**：新增 `@weak` 修饰符：

```aura
class Node {
    var value: Int
    var next: Node
    @weak var prev: Node   // 弱引用，不计入引用计数
}
```

**实现要点**：
1. 语言层：`@weak` 注解 + `WeakRef<T>` 类型
2. MIR 层：`WeakRetain` / `WeakRelease` 指令
3. 运行时层：弱引用表，对象释放时通知所有弱引用置为 null
4. AOT 层：弱引用读取用 CAS 检查 + 置空

**工作量**：~13 天

---

### 5.3 ARC 批量操作优化（Phase 1 — P1）

**问题**：当前每次赋值/传递都调用原子操作，同一表达式内多次赋值同一引用产生冗余操作。

**方案**：MIR 降级阶段追踪引用计数变化，作用域结束时批量提交净变化。

**工作量**：~8 天

---

### 5.4 快速路径优化（Phase 2 — P1）

**问题**：当前总是走原子操作（`lock inc/dec`），即使对象只有单线程访问也走原子路径。

**方案**：双路径策略——引用计数为 1（独占）时走非原子快速路径，引用计数 > 1（共享）时走原子慢速路径。

**注意**：快速路径需处理 ABA 问题（对象被释放后重新分配，rc 回到 1）。

**工作量**：~11 天

---

### 5.5 冗余 Retain/Release 消除增强（Phase 2 — P1）

**现状**：已有基础消除，可进一步增强：
1. **传递消除**：`a = b; c = a;` 传递 retain，消除中间 retain
2. **内联消除**：内联后函数边界的 retain/release 可能冗余
3. **尾调用消除**：返回引用的调用方立即赋值，可消除返回点的 retain

**工作量**：~10 天

---

### 5.6 逃逸分析增强（Phase 2 — P2）

**问题**：当前逃逸分析精度有限，许多本可栈分配的对象被保守地分配在堆上。

**增强**：
1. 函数级逃逸分析（跨函数边界追踪）
2. 逃逸结果传递到 MIR 降级，非逃逸对象直接栈分配，不插入任何 ARC 指令
3. 闭包逃逸分析（闭包捕获的对象影响逃逸判定）

**工作量**：~12 天

---

### 5.7 运行时泄漏检测增强（Phase 3 — P2）

**现状**：`aura leak-check` 仅做静态分析。

**增强**：运行时追踪未释放对象，程序退出前报告泄漏；弱引用循环检测；泄漏根因定位（分配栈跟踪）。

**工作量**：~12 天

---

### 5.8 Weak 引用容器（Phase 3 — P2）

**方案**：新增标准库容器：

```aura
class WeakRefSet<T> {
    fun add(obj: T): Unit
    fun get(obj: T): T?  // 返回 null 表示已释放
    fun collect(): Int
}

class WeakRefCache<K, V> {
    fun put(key: K, value: V): Unit
    fun get(key: K): V?
    fun collect(): Int
}
```

**工作量**：~8 天

---

### 5.9 极低频率 GC 安全网（Phase 3 — 可选，P3）

**触发条件**：仅在以下情况启用——ARC + Weak 仍无法解决某些复杂循环引用场景、运行时泄漏检测发现大量循环、用户显式启用。

**设计**：默认关闭，仅在内存压力时触发；只检测循环引用，不做全量标记-清除。

**工作量**：~12 天

---

## 六、时间线与预期收益

### 6.1 时间线

```
Week 1         Month 1          Month 2          Month 3+
├── Phase 0    ├── Phase 1      ├── Phase 2      ├── Phase 3
│   GC 清理 +  │   @weak +      │   快速路径 +    │   泄漏检测 +
│   JIT 修复 +  │   批量优化      │   消除增强 +    │   弱引用容器
│   Photon 修复│                │   逃逸分析     │   (+可选GC)
```

### 6.2 预期收益

| 阶段 | 预期收益 | 改动行数 |
|------|---------|---------|
| Phase 0（清理+修复） | 消除死代码 + 修复 JIT/Photon ARC 缺失 | ~-600 行（删除） |
| Phase 1（正确性） | 消除循环引用泄漏（闭包/双向关联场景） | ~800 行 |
| Phase 2（性能） | 性能提升 **15-30%** | ~600 行 |
| Phase 3（安全网） | 运行时泄漏检测 + 弱引用容器 + 可选 GC 兜底 | ~1000 行 |
| **总计** | 内存安全 + 性能优化全覆盖 | **~1800 行净增** |

增强后 ARC 开销预计从 **3-5% 降至 1-2%**。

---

## 七、风险

| 风险 | 缓解 |
|------|------|
| Weak 引用 ABA 问题 | Epoch-based reclamation 或 double-check |
| 逃逸分析与 ARC 交互 | 逃逸分析后执行验证 pass |
| 跨平台原子操作差异 | x86 快速路径可选禁用（`lock` 开销小） |
| 自举兼容性 | 每个增强先 VM 验证，再 AOT 验证，均通过自举测试 |

---

## 附录

### A. 相关文件索引

| 文件 | 说明 |
|------|------|
| `aura/core/aura/lang/native/Runtime.aura` | ARC C ABI |
| `aura/core/aura/lang/native/Memory.aura` | 原子操作声明 |
| `aura/core/aura/lang/native/GC.aura` | ARC 单例 + 待删除的 GC 对象 |
| `aura/compiler/aura/lang/compiler/memory/Arc.aura` | ARC 堆（VM 层） |
| `seed/compiler/src/mir/mir.rs` | MIR 降级 + ARC 插桩 |
| `seed/compiler/src/codegen/arc.rs` | ARC 自动插入与优化 |
| `aura/compiler/aura/lang/compiler/aot/Emit.aura` | AOT ARC 发射 |
| `aura/compiler/aura/lang/compiler/jit/JitLower.aura` | JIT ARC 降级（待修复） |
| `aura/photon/aura/lang/compiler/photon/PhotonRuntime.aura` | Photon ARC 发射（待修复） |
| `book/chapter-04.md` | 性能基准（ARC 3-5%） |
| `docs/语言-基础类型性能分析与优化方案.md` | ARC 优化方案 §3.11 |
| `book/README.md` | 设计原则（"无 GC"、"ARC（无暂停）"） |

### B. GC 待删除文件清单

| 文件 | 行数 | 处置 |
|------|------|------|
| `aura/compiler/aura/lang/compiler/gc/Gc.aura` | ~200 | 删除 |
| `aura/compiler/aura/lang/compiler/gc/MarkSweep.aura` | ~30 | 删除 |
| `aura/compiler/aura/lang/compiler/gc/Incremental.aura` | ~38 | 删除 |
| `aura/compiler/aura/lang/compiler/gc/Concurrent.aura` | ~43 | 删除 |
| `aura/compiler/aura/lang/compiler/runtime/GcTrigger.aura` | ~45 | 删除 |
| `seed/compiler/src/bootstrap/runtime.rs`（GcHeap 部分） | ~140 | 删除 GcHeap，保留 Coroutine/memory |
| `aura/compiler/aura/lang/compiler/vm/VmInstance.aura`（gc 相关字段/方法） | ~15 | 删除 gc 字段、gcAlloc、gcCollect、gcThreshold |
| `aura/compiler/aura/lang/compiler/vm/Opcodes.aura`（GC 指令） | ~4 | 删除 GC_MARK/GC_SWEEP |
| `aura/core/aura/lang/native/GC.aura`（GC 对象部分） | ~50 | 删除 GC 对象，保留 ARC 对象 |
