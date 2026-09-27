# Aura VM 纯 Aura 化：技术方案与达成路径

> **文档编号**：VM-PA-00（v3）
> **日期**：2026-09-25
> **状态**：技术方案（决策已定，待评审）
> **目标**：将目前由 Rust 实现的虚拟机（`rust/compiler/src/vm/`）全部改用 Aura 自实现（`aura/compiler/aura/lang/compiler/vm/`）
> **结论**：**可行**——核实发现 `bootstrap/` 是孤立死代码（全仓库 0 生产引用），因此「新 VM 不引用 bootstrap」这一隔离要求**今天已天然满足**，隔离成本近乎为零。建议分 5 个阶段推进，总工期 **14–22 周（3.5–5.5 个月）**，JIT/Photon 作为独立任务并行跟踪。

---

## 〇、决策记录（已定，本方案据此展开）

| # | 决策 | 内容 | 影响 |
|---|------|------|------|
| D1 | **`bootstrap/` 保留但不引用** | `rust/compiler/src/bootstrap/`（2,759 行 / 10 文件）**保留在库中不删除**，但**新的 Aura VM 实现不得引用它**。Rust 侧 VM 与 bootstrap 保持现状，不做清理 | 见 §1.1 核实：该层当前已 0 生产引用，隔离要求天然满足；见 §2.5 的隔离守卫机制 |
| D2 | **纯 Aura 实现 + Photon 后端** | 下沉部分**仅保留无法用 Aura 实现的部分**，不再使用 Rust | VM 指令集压到最小（~30 条），其余全部 Aura 代码 |
| D3 | **JIT 走纯 Aura + Photon** | 不纳入本方案本地范围，用其他任务跟踪 | 本方案 P0–P4 产出**纯解释执行**的 VM，JIT 是独立并行任务 |
| D4 | **Rust 当前仅为种子** | 主要还是 Aura 自举实现 VM | 迁移后 Rust 编译器是**一次性冻结种子**，完成自举即可彻底移除 |

**与 v1 方案的关键差异**：
- v1 建议"薄 VM + 厚标准库"并把 `bootstrap/` 列为常驻引导层边界 → **v2 确立隔离约束：新 VM 不引用 bootstrap，但该层代码保留不删**
- v1 含 P4「JIT/AOT 接入」阶段（3–5 周）→ **v2 移除，改为独立任务**
- v1 工期 18–27 周 → **v2 缩减为 14–22 周**

---

## 一、事实基线：现状核查结论

### 1.1 三套 VM 并存，其中中间那套是孤立死代码（保留但不得引用）

| # | 实现 | 位置 | 规模 | 指令数 | 生产使用 |
|---|------|------|------|--------|---------|
| A | **Rust 主 VM** | `rust/compiler/src/vm/` | **15,172 行 / 26 文件** | **106** | ✅ CLI `main.rs:1769/2445/2979` 三处调用 |
| B | **Bootstrap 迷你 VM** | `rust/compiler/src/bootstrap/` | **2,759 行 / 10 文件** | **15–17** | ❌ **完全孤立** |
| C | **Aura VM** | `aura/compiler/aura/lang/compiler/vm/` | **4,419 行 / 13 文件** | **~65** | ⚠️ CLI 部分命令 + 少量测试 |

**B 的孤立性经 grep 全面核实**：

| 引用源 | 对 `bootstrap` 的引用 |
|--------|----------------------|
| `rust/cli/src/main.rs`（实际 CLI 入口） | **0 处** |
| `rust/cli/src/` 其他文件 | **0 处** |
| `rust/loom/src/` | **0 处**（仅 `ffi/aot.rs:12`、`stdlib/mod.rs:20` 两处**注释**提及） |
| `rust/compiler/src/` 其他模块 | **0 处** |
| `rust/compiler/src/lib.rs:3` | `pub mod bootstrap;`（**唯一挂载点**） |
| `rust/compiler/tests/bootstrap_test.rs` | 20+ 处（**唯一使用者，测试文件**） |
| `bootstrap/` 内部自引用 | 13 处 |

**关键推论**：`bootstrap/` 从未接入任何生产路径。`docs/pure_aura_jit/README.md:86-88` 将其描述为"最小引导层保留边界"，这是**规划意图，不是代码事实**。

**按 D1 决策：该层保留不删除，但新 VM 实现不得引用它。** 由于当前已是 0 生产引用，隔离约束**今天已天然满足**，本方案的工作不是"清理"，而是**加一道守卫防止将来被接回**（见 §2.5）。

**另一处重要纠正**：`bootstrap/jit_ffi.rs`（328 行）定义了 `jit_compile`/`jit_load`/`jit_call`，正是 Aura 侧 `VmJitBridge.aura` 声明的 `@native` 目标。但 grep 核实主 VM 的 JIT **完全不走这份实现**：

- 主 VM JIT 在 `vm/jit.rs:388/408/1456` 调用 `jit_compile_cranelift`（定义于 `vm/jit.rs` 内部，直接调 Cranelift）
- `bootstrap::jit_ffi::{jit_compile,jit_load,jit_call}` 的唯一调用者是 `bootstrap_test.rs:217/622`

即：文档声称"JIT FFI 边界 0% 待开发"在**结论上歪打正着**（确实未接入生产），但**理由完全错误**（不是"未实现"，而是"实现了但从未接线"）。**该文件按 D1 保留在库中，新 VM 与 JIT 任务均不得使用它。**

### 1.2 代码量与功能量对比

| 模块 | Rust VM（行） | Aura VM（行） | 差距性质 |
|------|--------------|--------------|---------|
| 解释执行主循环 | 2,346（`interp.rs`） | 1,897（`Vm.aura`） | 架构不同（见 1.4） |
| VM 核心/指令定义 | 1,757（`mod.rs`） | 102（`Opcodes.aura`，**死代码**） | 缺失 |
| 原生函数注册表 | 1,334（`native.rs`，~93 个） | 0 | **完全缺失** |
| JIT | 2,260 | 240（`VmJitBridge.aura`，**未接线**） | D3 决策：移出本方案 |
| 调试器 | 1,234（`debugger.rs`） | 0 | **完全缺失** |
| AOT 运行时 + mmap | 1,297 | 0 | **完全缺失**（随 Photon 任务） |
| 并发运行时 | 2,267（7 文件） | ~250（`Vm.aura` 内散落的原始内存自旋锁） | 部分 |
| 堆/值模型 | 814（`heap.rs`+`value.rs`） | ~400（`Vm.aura` 内，**有缺陷**） | 部分 |
| FFI | 585（3 文件） | ~100（**桩**） | 缺失 |
| 序列化/模块 | 539 | 0（Aura 侧在 `serialize/` 下） | 部分 |
| 其他（event/ipc/coroutine/thread_pool/actor_process） | 1,170 | 0 | **完全缺失** |

**Rust VM 按规模排序（前 10）**：`interp.rs` 2346 / `mod.rs` 1757 / `jit.rs` 1382 / `native.rs` 1334 / `debugger.rs` 1234 / `aot_runtime.rs` 1034 / `concurrent_native.rs` 950 / `jit_opt.rs` 752 / `event_notifier.rs` 549 / `heap.rs` 448。

### 1.3 Rust VM 功能规格（迁移目标）

| 维度 | 规格 |
|------|------|
| 指令变体 | **106** 个（`codegen/opcode.rs:30-274`） |
| 原生函数 | **~93 个**独立函数 / ~112 注册条目（`vm/native.rs:50-340`） |
| 代码行数 | **15,172** 行 / 26 文件 |
| 值类型 | 10 变体：`Int(i64)` / `Float(f64)` / `Bool` / `Str(Rc<str>)` / `Null` / `Ref(usize)` / `Weak(usize)` / `Ptr(i64)` / `List(Vec<Value>)` / `Map(HashMap<Value,Value>)`（`vm/value.rs:14-37`） |
| 堆数据类型 | 7 变体：`Object{type_tag,fields,vtable}` / `Array` / `List` / `Map` / `Closure{func_name,param_count,locals,captures,func_idx}` / `Enum` / `FnRef`（`vm/heap.rs:19-54`） |
| 堆策略 | **纯 ARC（无 GC）** |
| 异常机制 | Handler 栈 + 帧截断 + 类型过滤 + 自动字符串→Exception 包装 |
| 并发原语 | 6 种（Mutex/RwLock/Atomic/Condvar/Barrier/Semaphore）+ EventNotifier 零轮询 |
| FFI | C 回调蹦床（最多 8 参数，thread_local + 全局栈双层派发） |

### 1.4 架构层面的根本差异（最关键的判断）

两侧**不是同一设计的两种实现，而是两种不同的架构范式**。

| 维度 | Rust VM | Aura VM |
|------|---------|---------|
| 字节码表示 | 预解码 `Vec<Instr>` 枚举，操作数加载期解析为**索引** | `String` 文本，每行一条指令 `OPCODE arg1 arg2` |
| 指令分派 | Rust `match` 枚举判别式（编译器生成跳转表） | **60+ 条 `else if (opcode == "...")` 字符串线性比较**（`Vm.aura:293-719`） |
| 值表示 | tagged union 枚举，基础类型内联 | `Any` + `Long` 裸指针（`Memory.alloc`） |
| 操作数 | `u16`/`i32` 索引，加载期完成跳转重定位 | 参数字符串，执行期 `VmOps.toI32()` 逐个解析 |
| 对象字段 | `HashMap<u16, Value>`，**按实例**存储（FNV 哈希键） | **全局表** `globals["field:"+name]`，**跨实例共享** |
| 分派成本 | O(1) | O(N) 字符串比较 × 每指令 N 条分支 |

**Aura VM 的架构代价已在代码注释中被实测记录**。`Vm.aura:56-57`：

```
执行期不再做 `substring` / 参数字符串切分：AOT 运行时没有 GC，
逐条解析会把临时字符串累积到 GB 级。
```

`Vm.aura:91-103` 存在两个**双格式适配开关**，说明两套字节码约定不一致：

```
aucArgBase: .auc 的形参槽从 1 开始（槽 0 是函数指针占位）
aucMode:    LIST_LEN 在 Aura 文本字节码里 peek，在 Rust .auc 里 pop
```

`Vm.aura:93-95` 记录了一个由槽位错位引起的真实 bug：`fact(3)` 恒返回 1。

**结论**：即使功能补齐，字符串分派 + 文本字节码在性能与正确性上都无法与 Rust VM 等价。**架构重构是前置必做项，不是优化项。**

### 1.5 `.auc` 二进制格式兼容性

Aura 侧 `serialize/AucLoader.aura` 能读取 Rust 编译器产出的真实 `.auc`（魔数 `"AURA"`，版本 ≤ 7），但有两处决定性问题：

**(1) 故意跳过关键区段**。`AucLoader.aura:251-253`：

```
// 说明：其后还有依赖/签名/vtable/类表/段表等区段。
// VM 执行只需要 `bytecode` / `constPool` / `funcTable` / `exports` / `imports`，
// 故**不再继续解析**后续区段。
```

被丢弃：依赖表、签名表、vtable、**类定义表**、AOT 段表。直接导致**无法支持类继承 / 虚方法分派 / AOT 嵌入机器码**。

**(2) 二进制被重新物化为文本字符串**。`AucLoader.readBytecode()`（`AucLoader.aura:335-422`）逐字节解码 Rust OpCode，经 `opcodeName(op)` 把 110 种数字操作码翻译成文本助记符，把 i32 跳转偏移重扫译成全局行号，写入 `mod.bytecode`（巨大拼接字符串）。

即：Aura VM **不直接执行 Rust 二进制字节码**，而是"二进制 → 文本 → 解释文本"。这是翻译层，不是执行器。

**指令集编号也是两套独立的**（`Opcodes.aura` 从未被分派器引用）：

| 指令 | `Opcodes.aura` | Rust `OpCode::byte()` |
|------|---------------|----------------------|
| ADD | 0x14 (20) | 3 |
| LOAD_LOCAL | 0x0A (10) | 1 (LoadVar) |
| CALL | 0x32 (50) | 26 |
| CALL_RUST / CALL_AURA / GC_MARK / GC_SWEEP | 存在 | **不存在** |

### 1.6 Aura VM 完成度逐项判定

| 能力 | 状态 | 证据 |
|------|------|------|
| 文本字节码解释循环 | ✅ 85% | 分派器 90+ 助记符 |
| Rust `.auc` 加载 | ⚠️ 50% | 能加载但丢弃 AOT/vtable/类表，翻译为文本 |
| 数值/字符串/集合 | ✅ 80% | 真实现 |
| **对象/类/方法分派** | 🔴 **15%** | **字段存全局表，跨实例共享** |
| **闭包/upvalue** | 🔴 **10%** | **不捕获任何 upvalue** |
| **协程** | 🔴 **5%** | **纯桩，不保存执行点** |
| 异常处理 | ⚠️ 40% | 骨架在，catch 类型过滤被忽略 |
| ARC/GC | ⚠️ 25% | ARC 仅对 Long 指针；**无 GC** |
| JIT 桥接 | 🔴 15% | `VmJitBridge.aura` 完整但**从未接线**（D3：移出本方案） |
| 尾调用 | 🔴 0% | `TailCall.aura` 是死代码 |
| 泛型/单例/vtable | 🔴 0% | 完全缺失 |
| **整体** | **~35%** | |

### 1.7 五个致命缺陷（按严重度排序）

#### 缺陷 1：对象字段跨实例共享 —— 使面向对象完全不可用

`Vm.aura:1193-1234`：

```aura
private fun getField(fieldName: String): Unit {
    val obj: Any = this.stack.pop()
    // ... 内建成员特判 ...
    // 简化：对象字段按名称哈希存储，此处使用全局表
    val fieldKey: String = "field:" + fieldName
    val val = this.globals.get(fieldKey)
    this.stack.push(if (val == null) 0 else val)
}

private fun setField(fieldName: String): Unit {
    val val: Any = this.stack.pop()
    val objPtr: Long = this.stack.pop()   // ← objPtr 被取出但从未使用
    val fieldKey: String = "field:" + fieldName
    this.globals.put(fieldKey, val)
}
```

字段不落在对象指针 `objPtr` 指向的堆内存里，而落在**全局变量表**里。后果：

```aura
class Dog { var name: String }
val d1 = Dog("Rex");  val d2 = Dog("Fido")
println(d1.name)   // 输出 "Fido" —— 而非 "Rex"
```

`newObject()`（`Vm.aura:1179-1186`）分配 16 字节头，类型标签恒为 0（`// 类型标签（0 = 通用对象）`）。`CALL_METHOD` / `CALL_CTOR`（`1291-1301`）是桩，忽略对象、忽略参数、无 vtable 分派。

#### 缺陷 2：闭包不捕获 upvalue

`Vm.aura:1317-1336`：`loadClosure()` 解析 `funcName|varNames` 后**丢弃 varNames**；`makeClosure()` 只记函数名；`callClosure()` 以 `argc=0` 调用。独立 `Closures.aura`（282 行）有完整 upvalue API，**从未被 `Vm.aura` 引用**。

后果：**lambda / 回调 / 函数式编程不可用**。

#### 缺陷 3：协程是纯桩

`newCoroutine()`（`1512-1519`）只把 funcIdx 塞进 HashMap；`resumeCoroutine()`（`1521-1528`）只做栈数据重排，不恢复执行点（注释自认 `// 简化：协程恢复在当前 VM 内继续执行`）；`YIELD`（`1504-1510`）设 `running=false` 但**无恢复路径**。`VmInstance.aura:76` 的独立 `coroutines` 字段从未使用。

#### 缺陷 4：无 GC，堆对象泄漏

`Opcodes.aura:57-58` 定义了 `GC_MARK`/`GC_SWEEP`，但**分派器无对应指令**。`VmInstance.aura` 的 `GcHeap` 从未使用。ARC 只对 64 位指针生效（`typeName(v)=="Long"`），装箱值不计数；`weakRef()` 原样透传；`boxAlloc()` 对字符串 `write64` 会截断。`VmCollections` 创建的列表/映射**头部、map 头部、所有元素指针都从未释放**。

#### 缺陷 5：JIT 桥接完整实现但完全未接线

`VmJitBridge.aura`（344 行）本身完整（热点阈值 10000、派发决策、Clif IR → FFI `jit_compile` → `jit_load` → 分发表、去优化回退）。但 `grep "vmCallHook\|VmJitBridge" Vm.aura` 返回 **0**。

> **按 D3 决策**：此桥接层随 JIT 任务一并移交，不在本方案范围内。本方案产出的 VM 为**纯解释执行**。

### 1.8 测试与验证现状

- `tests/phase5_vm_tests.aura` 中至少 4 个用例（`testVmRunnerStack`/`testVmRunnerLocal`/`testVmRunnerGlobal`/`testVmRunnerArith`）调用**已删除的旧 API**（`push`/`pop`/`peek`/`setLocal`/`getLocal`/`arithAdd`），**当前代码库下无法编译**。
- `examples/` 目录 `grep "VmRunner\|loadAucAndRun"` 返回 **0** —— **没有任何示例实际调用 VM**。
- **无端到端证据证明 Rust 编译器产出的 `.auc` 曾被 Aura VM 成功执行过。**
- **三个死代码模块**：`Opcodes.aura`（`grep "Opcodes\." Vm.aura` → 0）、`TailCall.aura`（`grep "TailCall" Vm.aura` → 0）、`Closures.aura`（`grep "Closures" Vm.aura` → 0）。

### 1.9 历史时间线

```
2026-09-19  docs/remove_rust/00：「Vm.aura 是 return null 占位桩，不能移除 Rust」
            docs/remove_rust/01（同日）：「P0 完成，Vm.aura ~900 行、100+ 操作码」
            —— 同一天两份文档结论互相矛盾；实际 1,897 行 / ~65 分支 / 35%，两份都不对
2026-09-21  bootstrap/ 全部 10 文件最后修改日期（此后从未被生产路径引用）
2026-09-22  提交 436c16b「实现aura自举虚拟机并运行」：Vm.aura +1637 行、AucLoader +495、
            VmCollections +339、VmOps +649、VmRunner 从 919 行重写精简
2026-09-23  提交 4f91c84「彻底抛弃rust依赖」：删除根 Cargo.toml / cli/Cargo.toml /
            aot/runtime/*.ll / aura/tests/concurrent/*
2026-09-23  提交 036ab65「把误删的工具代码添加回来」—— 回滚
2026-09-24/25  集中于 photon 后端（HAT 端到端）
```

**文档现状**：`docs/` 下 24 份相关文档中 **12 份结论已过期**，存在 **9 处互相矛盾**（含同目录同日两份文档结论完全相反）。权威依据仅 6 份：`docs/photon/implementation-deviation-analysis.md`、`aura/compiler/README.md`、`docs/pure_aura/03-差距分析.md`、`docs/pure_aura/03-自举验证报告.md`、`docs/pure_aura_jit/README.md`、`docs/pure_aura_jit/02-技术方案.md`。

---

## 二、架构设计（据四项决策展开）

### 2.1 目标 VM 形态：极简指令集 + 全 Aura 标准库

据 D2「仅无法用 Aura 实现的才下沉」，目标指令集压到最小。**判定标准**：该语义能否用「栈 + 局部变量 + 对象字段 + 函数调用」组合表达？能 → Aura 代码；不能 → VM 指令。

**必须保留为 VM 指令的（~30 条，无法用 Aura 表达）**：

| 类别 | 指令 | 为何无法下沉 |
|------|------|-------------|
| 值与常量 | `ConstI` `ConstF` `ConstS` `ConstBool` `ConstNull` | 字面量装载是 VM 基本能力 |
| 栈与局部变量 | `LoadLocal` `StoreLocal` `Pop` `Dup` `Swap` | 栈操作是 VM 基本能力 |
| 算术 | `Add` `Sub` `Mul` `Div` `Rem` `Neg` `Not` | 需 VM 内联类型分发 |
| 比较 | `Eq` `Ne` `Lt` `Gt` `Le` `Ge` | 需 VM 内联类型分发 |
| 位运算 | `BitAnd` `BitOr` `BitXor` `Shl` `Shr` | 需 VM 内联类型分发 |
| 控制流 | `Jump` `JumpIfTrue` `JumpIfFalse` | 控制流转移无法下沉 |
| 调用 | `Call` `Return` `ReturnTail` `CallNative` | 调用约定是 VM 基本能力 |
| 对象 | `NewObject` `NewArray` `GetField` `SetField` `GetIndex` `SetIndex` | 对象布局是 VM 基本能力 |
| 类型 | `InstanceOf` `CheckCast` | 需 VM 读取类型标签 |
| 引用计数 | `IncRef` `DecRef` | **正确性关键**：GC 扫描器需感知引用，必须 VM 级 |
| 异常 | `PushHandler` `PopHandler` `Throw` | 帧截断需 VM 级栈操作 |
| 终止 | `Halt` | VM 终止语义 |

**下沉为纯 Aura 代码的（~76 条原 Rust 指令 + 全部原生函数）**：

| 原 Rust 指令 | 数量 | 下沉形态 |
|-------------|------|---------|
| 集合元素操作 `ListPush/Pop/Len` `MapSet/Get/Len` | 6 | Aura `ArrayList`/`HashMap` 方法 |
| 集合构造 `NewList` `NewMap` | 2 | Aura 构造器（VM 仅保留 `NewArray` 供内部数组用） |
| 并发 `Mutex*`(4) `Atomic*`(5) `RwLock*`(5) `Channel*`(3) `Condvar*`(4) | 21 | Aura 标准库语义层 + `CallNative` 调 syscall 原子原语 |
| 线程 `ThreadSpawn/Join/Sleep/Id/Parallelism` | 5 | Aura `Thread` 类 + `CallNative` |
| ARC 扩展 `Retain` `Release` `DropRef` `WeakRef` `WeakGet` `BoxAlloc` `DeferBegin` `DeferEnd` | 8 | Aura `Arc`/`Weak`/`Box` 包装类 |
| FFI 助手 `CString` `ReadCStr` `PtrIsNull` `PtrToInt` `IntToPtr` `MakeCallback` `CallC` | 7 | Aura `CString` 类 + `CallNative` |
| 枚举/函数引用 `EnumConstruct` `EnumTag` `MakeFnRef` | 3 | Aura `Enum` 类型 + 对象字段 |
| 闭包 `MakeClosure` `CallClosure` | 2 | Aura 闭包对象（`NewObject` + 字段 + `Call`） |
| 跨模块 `CallExport` `CallExternal` `CallAot` | 3 | Aura 模块注册表 + `CallNative` |
| 协程 `Yield` `NewCoroutine` `ResumeCoroutine` | 3 | **见下方争议项** |
| 原生函数注册表 ~93 个 | 93 | Aura `std` 包 + `CallNative` 转发到 syscall 层 |

**争议项（需 P0 评估后定夺）**：

1. **协程（3 条）**：真协程需保存/恢复 VM 执行点（PC + 调用栈 + 操作数栈），这是 VM 内部状态，**Aura 代码无法自访问**。建议**保留为 VM 指令**，或采用「显式状态机 + 对象字段」的协作式实现（无 VM 支持，但无法跨函数挂起）。
2. **闭包（2 条）**：可用 `NewObject` + 字段 + `Call` 组合实现——闭包对象含「函数索引字段 + 捕获值字段」，调用时 Aura 代码先构造实参数组再 `Call`。技术上可行但**每次闭包调用多一层对象查找开销**。建议**下沉**，接受性能代价（JIT 任务是独立并行线，可后续覆盖热点）。

### 2.2 分派架构重构（前置必做）

当前字符串 `else if` 链是**性能与正确性的双重瓶颈**：每条指令 60+ 次字符串比较（O(N) 分派）、参数执行期反复解析、字节码是巨大拼接字符串（AOT 运行时无 GC 时累积到 GB 级，`Vm.aura:56-57` 自认）。

**目标架构**（对齐 `interp.rs` 的预解码模式）：

```
加载期（一次性完成，执行期零解析）：
  字节码（二进制 .auc 或文本）
    → opcode 数组   Array<Int>            数值编码，不再存字符串
    + 操作数表      Array<Array<Int>>     索引已在加载期解析
    + 跳转重定位    i32 字节偏移 → 指令索引（加载期完成）
  文本模式的 String 字节码也在此处一次性转换

执行期：
  val op = ops[ip]; val args = argTable[ip]; ip += 1
  分派：when (op) { const OP_X -> ...; const OP_Y -> ... }   ← 数值常量匹配
  参数全部为 Int/Long，无字符串解析
```

**前置技术依赖**：`Memory.alloc` 连续数组 + 定宽元素布局。`VmStack.aura` 已用原生列表 + 显式栈指针（`Vm.aura:70`），该模式可行。

**工作量**：等价于重写 `Vm.aura` 主循环与全部指令实现，约 2,000–3,000 行 Aura。

### 2.3 JIT 与 AOT（按 D3 移出本方案）

**本方案不实现 JIT/AOT 加速能力**，产出的是**纯解释执行 VM**。

移交内容（独立任务跟踪）：

| 移交项 | 当前状态 | 目标路线 |
|--------|---------|---------|
| JIT 派发桥接 | `VmJitBridge.aura`（344 行）完整实现但**未接线** | 纯 Aura + Photon |
| Clif IR 生成 | `compiler/jit/JitLower.aura` 等（`pure_aura_jit` 侧声称已完成） | 替换为 Photon IR |
| JIT FFI 边界 | `bootstrap/jit_ffi.rs`（328 行）**从未接入生产**，按 D1 保留但不得使用 | Photon 进程内编译 |
| AOT 运行时 | Rust `aot_runtime.rs`（1,034 行）+ `mmap_util.rs`（263 行） | Photon + PhotonNativeWriter |
| JIT 后端 | Cranelift crate | **Photon 自研后端替换** |

**本方案的契约要求**：VM 的 `Call`/`CallNative` 分派路径必须预留**可插拔的加速派发钩子**（但不实现），接口签名保持稳定，供 JIT 任务对接。

**Photon 成熟度现状**（据 `docs/photon/implementation-deviation-analysis.md`，2026-09-22，最权威）：端到端真实管线**未打通**，仅 hello world 冒烟路径。团队内部认知不一致（同日 `implementation-completeness-report.md` 称"98% 完成"，偏差分析称"六大致命偏差"）。**因此本方案不依赖 Photon 进度**，两条线完全解耦。

### 2.4 Rust 定位（按 D4：仅种子）

| 组件 | 迁移后状态 |
|------|-----------|
| VM 解释器 | ✅ 纯 Aura |
| 标准库运行时语义 | ✅ 纯 Aura |
| 编译器前端/后端、CLI、LSP、调试器、loom | ✅ 纯 Aura（已大体完成） |
| **`rust/compiler/src/bootstrap/`**（2,759 行） | 🟡 **保留但不引用**（已核实为孤立死代码，见 §1.1；新 VM 与 JIT 任务均不得使用，见 §2.5） |
| **Rust 编译器** | 🟡 **一次性冻结种子**——编译 Aura 编译器为原生 exe，完成自举后彻底移除 |
| OS 系统调用（mmap/malloc/NT_CreateFile/线程原语） | 🟡 **保留**——不可避免的物理边界 |
| LLVM / Cranelift | 🟡 **目标态由 Photon 替换**（独立任务） |

**迁移后残留 Rust**：`rust/` 下的种子编译器（含 `bootstrap/`，保留但不引用）——用于编译 Aura 编译器自身，自举闭环建立后可整体移除。**不再有任何常驻的 Rust VM 运行时被新 VM 引用**。

### 2.5 bootstrap 隔离守卫（按 D1 新增）

D1 要求"保留但不引用"，因此本方案的必要工作不是删除，而是**建立机制防止将来被接回**。当前 0 生产引用是事实状态，但事实状态会漂移。

**守卫措施（纳入 P0.1）**：

1. **CI 静态检查**：新增检查脚本，断言以下约束，任一违反即失败：
   - `aura/compiler/` 全目录内 `grep -r "bootstrap"` **0 命中**（Aura 侧代码不得引用 bootstrap 符号）
   - `rust/compiler/src/vm/` 内 `grep -r "bootstrap"` **0 命中**（Rust 主 VM 不得引用 bootstrap）
   - `rust/cli/`、`rust/loom/src/` 内 `grep -r "bootstrap"` 仅允许**注释命中**
2. **依赖方向声明**：在 ADR 中明确依赖方向单向性——`bootstrap/` 是**叶子模块**，允许被 `tests/` 引用，**禁止被 `src/vm/`、`src/codegen/`、CLI、loom 生产代码引用**。
3. **注释标记**：在 `rust/compiler/src/bootstrap/mod.rs` 头部加显著标记，说明"此层为遗留孤立代码，保留但不引用，新 VM 与 JIT 不得接入"。

**为何仍需守卫**：`bootstrap/` 中的 `jit_core.rs`/`jit_ffi.rs` 与 JIT 需求高度重叠，独立 JIT/Photon 任务在排期压力下**最容易的选择就是把现成的 328 行 `jit_ffi.rs` 接回来**。守卫必须在 JIT 任务启动前就位。

---

## 三、分阶段达成路径

### 阶段 P0：地基、隔离守卫与架构重构（2–3 周）

> **目标**：确立架构隔离与架构约束、消除阻断性设计缺陷。**P0 完成前不应开始任何功能补全。**

| 任务 | 内容 | 交付物 |
|------|------|--------|
| **P0.1 bootstrap 隔离守卫** | **不删除代码**；建立 §2.5 的三项守卫：CI 静态检查脚本（断言 4 条 grep 约束）+ ADR 依赖方向声明 + `bootstrap/mod.rs` 头部标记 | CI 检查脚本 + ADR + 标记注释 |
| P0.2 文档治理 | 12 份过期文档移入 `docs/archive/` 并加 `DEPRECATED` 头注；补齐 6 份权威文档日期戳 | PR：`docs/archive/` |
| P0.3 架构决策落定 | 把 §二 的设计写成 ADR（`docs/adr/ADR-001-极简VM指令集.md` 等），含 §2.1 争议项的评估结论 | 3–4 份 ADR |
| **P0.4 分派器重构** | 字符串 if/else 链 → 数值索引分派表；字节码 `String` → `Array<Int>` opcode + `Array<Array<Int>>` 操作数表；加载期完成跳转重定位；`Opcodes.aura` 改为真实生效的唯一编号源（或并入删除） | `Vm.aura` 主循环重写（~2,000–3,000 行） |
| **P0.5 对象模型修正** | 字段从全局表移入对象堆布局（`objPtr + 16` 起按字段槽存储）；实现类定义表与 vtable 解析的加载端；`newObject`/`getField`/`setField`/`callMethod`/`callCtor` 全部重写 | 堆布局规范 + 5 个方法重写 |
| P0.6 死代码清理 | 明确 `Opcodes.aura` / `TailCall.aura` / `Closures.aura` 三者去留：接线或删除，不留「有实现但无调用方」的模块 | 三者去留决议 |
| P0.7 测试重建 | 修复 `phase5_vm_tests.aura` 中 4 个失效用例；新增**多实例字段隔离回归测试** | 测试全部编译通过 |
| P0.8 加速钩子预留 | 在 `Call`/`CallNative` 分派路径预留可插拔加速派发钩子（仅接口，不实现） | 接口签名稳定 |

**P0 验收标准**：
- ✅ `bootstrap/` 隔离守卫生效：新增 CI 检查脚本并在当前代码库上**通过**（证明 0 生产引用是可持续的）
- ✅ `git grep "bootstrap" -- aura/compiler/` 与 `-- rust/compiler/src/vm/` 均 **0 命中**
- ✅ `Vm.aura` 分派器无字符串 `else if` 链，无执行期字符串解析
- ✅ 以下程序在 Aura VM 下正确运行：
  ```aura
  class Dog { var name: String }
  fun main() {
      val d1 = Dog("Rex");  val d2 = Dog("Fido")
      println(d1.name)   // 必须输出 "Rex"，不是 "Fido"
  }
  ```
- ✅ `phase5_vm_tests.aura` 全部用例编译并通过
- ✅ `examples/` 下新增至少 1 个实际调用 `VmRunner` 的示例

**P0 工作量**：~3,000–4,000 行 Aura 改动（bootstrap 保留不删，仅加守卫脚本与标记），2–3 周（1–2 人）

---

### 阶段 P1：正确性内核（4–6 周）

> **目标**：让 Aura VM 的**语言语义正确**，这是能否承载 Aura 编译器自编译产物的前提。

| 任务 | 内容 | 对齐目标 | 估计行数 |
|------|------|---------|---------|
| P1.1 值模型 | 对齐 Rust `Value` 10 变体（`Int/Float/Bool/Str/Null/Ref/Weak/Ptr/List/Map`）；明确装箱约定 | `vm/value.rs` | ~400 |
| P1.2 类/继承/vtable 执行端 | 在 P0.5 的加载端基础上实现虚方法分派、`InstanceOf`/`CheckCast`、对象单例 | `vm/heap.rs` + `mod.rs` | ~1,500 |
| P1.3 闭包/upvalue | 按 §2.1 决议实现：若下沉为 Aura 闭包对象，则在 VM 层实现「闭包对象 + 函数索引字段」的 `Call` 支持；若保留指令则接线 `Closures.aura` | `heap.rs` 的 `Closure` 变体 | ~600 |
| P1.4 异常 | catch 类型过滤器生效；异常类型/消息结构；多级过滤（内层优先）；自动字符串→Exception 包装 | `interp.rs` 的 `Handler` 栈 | ~500 |
| P1.5 内存 | ARC 扩展到所有引用类型（对象/集合/闭包/字符串）；`WeakRef` 真弱引用；`BoxAlloc` 类型感知；**集合/映射头部与元素释放** | `vm/heap.rs` | ~800 |
| P1.6 尾调用 | 新增 `ReturnTail` 指令（Rust VM 无此指令，本项目改进项）；接线 `TailCall.aura` | — | ~300 |
| P1.7 协程 | 按 §2.1 决议：保留 VM 指令则实现真状态机（保存/恢复 PC + 调用栈 + 操作数栈）；下沉则实现显式状态机对象 | `coroutine.rs` | ~400 |

**P1 验收标准**：
- ✅ `tests/classes/`（20 个文件，含 `object_hierarchy_test.aura` 200 行、`test_value_class*.aura`）全部通过
- ✅ `tests/HashMap/`（5 文件 1,544 行）+ `tests/ArrayList/` + `tests/LinkedList/` + `tests/Deque/` + `tests/HashSet/` 全部通过
- ✅ `tests/language-test/01-lexer.aura` … `16-script-mode.aura`（16 文件 2,500+ 行）全部通过
- ✅ 闭包捕获测试：`val f = { val x = i }; ...` 能正确捕获并在闭包内读取
- ✅ ARC 泄漏检测：长循环创建/销毁对象后活跃对象计数归零（对齐 `heap.rs:94-96` 的 `active_count`）

**P1 工作量**：~4,500 行 Aura，4–6 周（1–2 人）

---

### 阶段 P2：指令集对齐与字节码完整化（3–4 周）

> **目标**：Aura VM 能直接执行 Rust 编译器产出的完整 `.auc`，与 Rust VM 输出一致。

| 任务 | 内容 | 对齐目标 |
|------|------|---------|
| P2.1 指令集补齐 | 按 §2.1 的极简指令集（~30 条）完成全部实现；补齐当前缺失的 `LoadConst`/`NewArray`/`GetIndex`/`Halt`/`PushHandler` 等 | `opcode.rs` 106 条中选中子集 |
| P2.2 `.auc` 加载器完整化 | 解析**依赖表 / 签名表 / vtable / 类表 / AOT 段表 / 源码索引段**（当前全部跳过，`AucLoader.aura:251-253`）；**取消「二进制→文本」翻译**，直接保留数值字节码 | `serialize.rs`（309 行） |
| P2.3 模块系统 | `ModuleRegistry` 等价实现；`CALL_EXPORT`/`CALL_EXTERNAL` 跨模块调用；修复当前 `"cross-module linking requires loaded module registry"` 报错 | `multi_module.rs`（230）+ `abi.rs`（190） |
| P2.4 **差分测试框架** | 建立 Rust VM vs Aura VM 的**输出差分测试**：同一 `.auc` 由两侧执行，比对 stdout / 退出码 / 异常 | 新建设施 |

**P2 验收标准**：
- ✅ Rust 编译器产出的 `.auc`（含类表/vtable）由 Aura VM **直接执行**（无文本翻译中间层）
- ✅ 差分测试套件（覆盖 `tests/` 下 300+ 个 `.aura` 编译产物）中，Aura VM 与 Rust VM 输出**逐字节一致**
- ✅ 跨模块调用测试：多 `.auc` 模块互相调用成功

**P2 工作量**：~2,500–3,500 行 Aura，3–4 周

---

### 阶段 P3：运行时服务（4–6 周）

> **目标**：补齐 Rust VM 的运行时服务能力，按 D2 全部下沉为 Aura 代码，仅 syscall 原语保留在 native 层。这是工作量最大、风险最高的阶段。

| 任务 | 内容 | Rust VM 对应 | 风险 |
|------|------|-------------|------|
| P3.1 原生函数下沉 | ~93 个原生函数由 Aura `std` 包接管（`println`/`abs`/`sqrt`/`pow`/`toInt`/`toFloat`/`toStr`/`clock`/`strlen`/`CString`/`CStr`/`ptrToInt`/`intToPtr`/`makeCallback`/`equals`/`hashCode`/`typeOf`/`aura_cast` 等）；三套命名空间（短名/`aura.lang.std.*`/`aura.ffi.*`）统一；仅 syscall 级原语经 `CallNative` 转发 | `native.rs` 1,334 行 | 🟢 低 |
| P3.2 并发同步 | 按 §2.1 全部下沉为 Aura 语义层：`Mutex`/`RwLock`/`Atomic`/`Channel`/`Condvar`，底层经 `CallNative` 调 syscall 原子原语（`Memory`/`Cpu.atomicAdd`/已有自旋锁基元） | `concurrent_native.rs` 950 + `actor.rs` 353 + `channel.rs` 186 + `channel_tcp.rs` 157 + `event_notifier.rs` 549 + `thread_pool.rs` 127 + `ipc.rs` 165 | 🔴 高 |
| P3.3 Actor | Actor 运行时与消息队列（纯 Aura） | `actor_process.rs` 194 | 🟡 中 |
| P3.4 FFI | `CString`/`ReadCStr` Aura 化；C 回调蹦床（最多 8 参数，thread_local + 全局栈双层派发）；动态库加载 | `ffi.rs` 357 + `ffi_cache.rs` 130 + `dynamic_ffi.rs` 98 | 🔴 高 |
| P3.5 调试器 | 断点/单步/寄存器读取/栈回溯 | `debugger.rs` 1,234 行 | 🟡 中 |

**P3 验收标准**：
- ✅ `tests/concurrent/`（7 文件 1,634 行：`atomic_ops`/`mutex_shared`/`rwlock_condvar`/`barrier_semaphore`/`future_chain`/`thread_basics`/`integration`）全部通过
- ✅ `tests/language-test/08-concurrency.aura`、`09-ffi.aura`、`10-memory.aura` 通过
- ✅ `tests/self_bootstrap/gc_test.aura`（302 行）、`memory_test.aura`（306 行）通过
- ✅ FFI demo：`examples/ext_ffi_demo` 在 Aura VM 下运行成功
- ✅ Actor/Channel 测试通过（跨线程消息传递正确）

**P3 工作量**：~5,000–7,000 行 Aura，4–6 周（2 人并行）

---

### 阶段 P4：自举闭环与 Rust VM 退役（2–3 周）

> **目标**：完成迁移闭环，退役 Rust VM。

| 任务 | 内容 |
|------|------|
| P4.1 自举闭环 | Aura VM 执行 Aura 编译器自编译产物（`Main.aura` 自举）；Stage-1/2/3 全通 |
| P4.2 全量差分回归 | Rust VM vs Aura VM 对 `tests/` 全量 + `examples/` 全量做差分 |
| P4.3 **删除 Rust VM** | 删除 `rust/compiler/src/vm/`（15,172 行 / 26 文件） |
| P4.4 CLI 切换 | `rust/cli/src/main.rs` 中 3 处 `Vm::new`（`1769`/`2445`/`2979`）改为 Aura VM 路径；Rust CLI 退化为纯种子 |

**P4 验收标准**：
- ✅ 自举链 Stage-1/2/3 全部成功
- ✅ 差分回归 100% 一致
- ✅ `git grep "compiler::vm\|use.*vm::"` 无生产代码命中
- ✅ 删除 `rust/compiler/src/vm/` 后全部测试仍通过

**P4 工作量**：~1,500 行改动 + 15,172 行删除，2–3 周

---

## 四、不可纯 Aura 化的边界

按 D2「仅无法用 Aura 实现的才下沉」，边界收敛到极小：

| 组件 | 为何不可 Aura 化 | 保留形态 |
|------|----------------|---------|
| **OS 系统调用**（mmap/malloc/NT_CreateFile/线程原语/epoll） | 必须与内核二进制接口交互，语言层无法替代 | `aura_syscalls.c`（43,074 字节）+ `aura/lang/native/arch/*/Syscalls.aura` |
| **Rust 种子编译器** | 自举的第一推动力——Aura 编译器自身需被某物编译 | 迁移后**仅用于编译 Aura 编译器**，自举闭环建立后可整体删除 |
| LLVM / Cranelift | 由 Photon 替换（**独立任务**，不在本方案范围） | 当前仍在用，Photon 成熟后移除 |
| **`rust/compiler/src/bootstrap/`**（2,759 行） | 已核实为孤立死代码（§1.1），**不属于"不可 Aura 化"，而是"已不需要但按 D1 保留"** | 保留在库中不删除；新 VM 与 JIT 任务均不得引用；P0.1 建立隔离守卫（§2.5） |

**对比 v1 方案**：v1 把 `bootstrap/` 列为常驻引导层边界；**v2 将其从"功能边界"降级为"遗留保留代码"**——它不构成新 VM 的能力来源，只被 P0.1 的隔离守卫约束。迁移后常驻 Rust 代码从「bootstrap + JIT FFI（功能边界）」收敛为「种子编译器（含孤立保留的 bootstrap）」。

**结论**：本方案完成后，Aura 拥有**自己实现自己的 VM**，Rust 从运行时依赖退化为构建时一次性工具。

---

## 五、风险清单与缓解措施

| # | 风险 | 等级 | 依据 | 缓解措施 |
|---|------|------|------|---------|
| R1 | **文档持续误导**，新工作基于过期结论返工 | 🔴 高 | 24 份文档 12 份过期、9 处矛盾；同日两份文档结论相反 | P0.2 强制先做文档治理；建立「文档必须有日期戳 + 代码行号引用」规范 |
| R2 | **对象模型修正牵连全部下游** | 🔴 高 | 当前字段存全局表（`Vm.aura:1222-1234`），修复需重设堆布局，影响 P1.2/P2/P3 全部 | P0.5 必须在 P1 前完成；先用类内定长字段槽表（简单）而非 FNV 哈希（灵活但复杂） |
| R3 | **分派器重构是重写而非修补** | 🔴 高 | 字符串→数值分派需重写 `Vm.aura` 主循环与全部指令实现（~3,000 行） | 保留现有文本字节码作为 P0 输入兼容层，新分派器逐步接管，旧路径标记 deprecated |
| R4 | **极薄指令集导致标准库路径性能不可接受** | 🔴 高 | ~76 条原 Rust 指令下沉为 Aura 代码，每次集合/同步操作多一层解释开销；本方案**无 JIT**（D3 独立任务） | P0.3 决策时建立微基准；设定可接受门槛（如 Rust VM 的 30–50%）而非要求等价；`List`/`Map` 内部数组布局由 VM 的 `NewArray` 建立，仅元素读写在 Aura 层；JIT 任务是独立并行线，成熟后覆盖热点 |
| R5 | **并发原语纯 Aura 化失败** | 🔴 高 | 子代理风险预判标注「并发同步原语/AtomicBool 不可纯 Aura 实现」 | P3.2 分层：语义层 Aura，**原子操作层保留 syscall 原语**（CAS 等不可用 Aura 表达）；不追求把 CAS 写成纯 Aura |
| R6 | **`bootstrap/` 隔离漂移**——遗留代码被后来的任务（尤其 JIT/Photon）接回生产路径 | 🟡 中 | 已核实当前 0 生产引用，但 D1 是"保留"而非"删除"，删除才是物理隔离；`jit_ffi.rs`（328 行）与 JIT 需求高度重叠，是最易被接回的部分 | P0.1 的 CI 静态检查（§2.5）在 JIT 任务启动前就位；ADR 声明单向依赖方向；`bootstrap/mod.rs` 头部标记 |
| R7 | **`phase5_vm_tests.aura` 已无法编译**，测试基线缺失 | 🟡 中 | 4 个用例调用已删除 API | P0.7 先修复测试再动 VM |
| R8 | **无 `.auc` 端到端执行证据**，P2 差分框架可能暴露大量隐藏缺陷 | 🔴 高 | 无任何测试/示例证明 Rust `.auc` 被 Aura VM 执行过 | P2.4 差分框架提前到 P1 末尾原型验证；采用「逐函数对齐」而非「整体一次性切换」 |
| R9 | **JIT 缺位导致长期性能不达标** | 🟡 中 | 本方案产出纯解释 VM；Aura 编译器比 Rust 编译器慢 4–5 倍（`pure_aura/07-性能对比报告.md`，报告本身已过期但趋势可信） | 明确告知：本方案**不解决性能**，仅解决「VM 归属」；性能由 Photon/JIT 独立任务承担；P0.8 预留的加速钩子即为对接点 |
| R10 | **协程/闭包下沉判定失误** | 🟡 中 | §2.1 标记为争议项，协程可能无法下沉 | P0.3 评估时以「能否跨函数挂起」为硬判据，宁可保留 VM 指令也不牺牲正确性 |
| R11 | **三个死代码模块（Opcodes/TailCall/Closures）**造成重复实现混乱 | 🟡 中 | 均有实现但均未被 `Vm.aura` 引用 | P0.6 明确去留：接线或删除，不留「有实现但无调用方」模块 |
| R12 | **自举的「第一推动力」悖论** | 🟢 低 | 需某物编译 Aura 编译器 | 已由 Rust 种子编译器解决，D4 已明确其定位，非新增风险 |
| R13 | **Photon 与本方案进度不同步导致契约错配** | 🟡 中 | Photon 成熟度团队内部认知不一致（同日两份文档 98% vs 六大致命偏差） | P0.8 加速钩子接口与 Photon 解耦（仅定义 VM 侧契约）；两条线独立验收，不互相阻塞 |

---

## 六、验收标准总表

| 阶段 | 完成定义（DoD） | 阻塞后续阶段？ |
|------|---------------|--------------|
| P0 | bootstrap 隔离守卫生效且 CI 检查通过；分派器无字符串比较；多实例字段隔离测试通过；3–4 条 ADR 落定；12 份过期文档归档 | 🔴 是 |
| P1 | `tests/classes/` + `tests/HashMap/` + `tests/language-test/` 全部通过；ARC 泄漏检测归零 | 🔴 是 |
| P2 | `.auc` 直接执行（无文本翻译）；差分测试与 Rust VM 输出一致 | 🔴 是 |
| P3 | `tests/concurrent/` + FFI demo + Actor/Channel 全部通过 | 🔴 是 |
| P4 | 自举 Stage-1/2/3 全通；删除 `rust/compiler/src/vm/`（15,172 行）后全量测试通过 | — |

**跨方案验收（由独立 JIT/Photon 任务负责，本方案不阻塞）**：热点 JIT 生效、AOT 段可执行、Photon 替换 LLVM/Cranelift。

---

## 七、工作量汇总

| 阶段 | 工期 | Aura 代码量 | 删除量 | 人力 |
|------|------|------------|--------|------|
| P0 地基/隔离守卫/架构重构 | 2–3 周 | 3,000–4,000 行改动 | —（bootstrap 保留不删） | 1–2 人 |
| P1 正确性内核 | 4–6 周 | ~4,500 行 | — | 1–2 人 |
| P2 指令集与字节码 | 3–4 周 | 2,500–3,500 行 | — | 1–2 人 |
| P3 运行时服务 | 4–6 周 | 5,000–7,000 行 | — | 2 人 |
| P4 自举与退役 | 2–3 周 | ~1,500 行改动 | 15,172 行（Rust VM） | 1–2 人 |
| **合计** | **14–22 周（3.5–5.5 个月）** | **~16,500–20,500 行** | **~15,200 行** | 1–2 人主力 |

**迁移后残留 Rust**：
- ✅ **删除**：`rust/compiler/src/vm/`（15,172 行 / 26 文件）
- 🟡 **保留但不引用**：`rust/compiler/src/bootstrap/`（2,759 行 / 10 文件）+ `bootstrap_test.rs` —— 遗留孤立代码，按 D1 保留，受 P0.1 隔离守卫约束
- 🟡 **保留（临时）**：`rust/` 其余部分——种子编译器，自举闭环建立后可整体移除
- 🟡 **保留（物理边界）**：`aura/runtime/cffi/aura_syscalls.c`

**与 v1 方案对比**：工期 18–27 周 → **14–22 周**（-4 至 -5 周）；残留常驻 Rust 从「bootstrap + JIT FFI（功能边界）」收敛为「种子编译器 + 孤立保留的 bootstrap（非功能）」。

---

## 附录 A：核查证据索引

| 结论 | 证据位置 |
|------|---------|
| 三套 VM 并存 | `rust/compiler/src/vm/mod.rs:18-45`（26 模块）/ `rust/compiler/src/bootstrap/mod.rs` / `aura/compiler/aura/lang/compiler/vm/` |
| Rust VM 15,172 行 | 26 文件行数汇总，`interp.rs` 2346 最大 |
| Aura VM 4,419 行 | 13 文件行数汇总，`Vm.aura` 1897 最大 |
| **bootstrap 完全孤立** | `grep -r "bootstrap" rust/` 命中 43 处，其中 CLI 0、loom 0（仅 2 处注释）、compiler/src 非 bootstrap 模块 0、唯一使用者 `tests/bootstrap_test.rs` |
| bootstrap 唯一挂载点 | `rust/compiler/src/lib.rs:3`（`pub mod bootstrap;`） |
| bootstrap 规模 | 10 文件 / 2,759 行（`vm_core.rs` 756 + `aot_core.rs` 665 + `jit_ffi.rs` 328 + `jit_core.rs` 268 + `runtime.rs` 203 + `memory.rs` 176 + `type_core.rs` 119 + `mod.rs` 76 + `value_check.rs` 71 + `any_core.rs` 97） |
| bootstrap 内嵌迷你 VM | `bootstrap/vm_core.rs:28-29`（Value 6 变体）、`:127-153`（15–17 指令） |
| **`jit_ffi.rs` 从未接入生产** | `bootstrap/jit_ffi.rs:69/158/297` 定义 `jit_compile/jit_load/jit_call`；主 VM JIT 走 `vm/jit.rs:388/408/1456` 的 `jit_compile_cranelift`（不同函数）；唯一调用者 `tests/bootstrap_test.rs:217/622` |
| 字符串分派 | `Vm.aura:293-719`（60+ 条 `else if (opcode == "...")`） |
| GB 级内存自认 | `Vm.aura:56-57` |
| 双格式适配 | `Vm.aura:91-103`（`aucArgBase`/`aucMode`） |
| 槽位错位真实 bug | `Vm.aura:93-95`（`fact(3)` 恒返回 1） |
| 跳过类表/vtable/AOT 段 | `AucLoader.aura:251-253` |
| 对象字段存全局表 | `Vm.aura:1222-1234` |
| 闭包丢 upvalue | `Vm.aura:1317-1336` |
| 协程纯桩 | `Vm.aura:1512-1528` |
| 无 GC | `grep "GC_MARK\|GC_SWEEP" Vm.aura` → 0 |
| JIT 未接线 | `grep "vmCallHook\|VmJitBridge" Vm.aura` → 0 |
| 三个死代码模块 | `grep "Opcodes\." Vm.aura` → 0；`grep "TailCall" Vm.aura` → 0；`grep "Closures" Vm.aura` → 0 |
| 测试已无法编译 | `tests/phase5_vm_tests.aura:334-407`（旧 API） |
| 无示例调用 VM | `grep "VmRunner\|loadAucAndRun" examples/` → 0 |
| Rust Value 10 变体 | `rust/compiler/src/vm/value.rs:14-37` |
| Rust HeapData 7 变体 | `rust/compiler/src/vm/heap.rs:19-54` |
| Rust 106 指令 | `rust/compiler/src/codegen/opcode.rs:30-274` |
| Rust ~93 原生函数 | `rust/compiler/src/vm/native.rs:50-340` |
| Rust CLI 3 处 VM 调用 | `rust/cli/src/main.rs:1769`/`2445`/`2979` |
| 时间线 | git log `436c16b`（09-22 实现 VM）/ `4f91c84`（09-23 删 Rust）/ `036ab65`（09-23 回滚） |

## 附录 B：相关文档可信度

**可作为权威依据（6 份）**：`docs/photon/implementation-deviation-analysis.md`（2026-09-22）、`aura/compiler/README.md`、`docs/pure_aura/03-差距分析.md`、`docs/pure_aura/03-自举验证报告.md`、`docs/pure_aura_jit/README.md`、`docs/pure_aura_jit/02-技术方案.md`

**结论已过期（12 份）**：`docs/remove_rust/00`、`docs/remove_rust/01`、`docs/pure_aura/01-现状分析`、`docs/pure_aura/05`、`docs/pure_aura/06-S4验证报告`、`docs/pure_aura/07-性能对比报告`、`docs/pure_aura/完全Aura化技术方案-v3.0`、`docs/pure_aura_jit/03-分阶段开发计划`、`docs/pure_aura_jit/04-测试与验收矩阵`、`docs/photon/photon-e2e-status`、`docs/photon/photon-compiler-bootstrap-analysis`、`docs/compiler/编译器性能分析与优化建议`

**最需要注意的自相矛盾**：`docs/remove_rust/` 目录下**同一天（09-19）**的两份文档，一份说"Vm.aura 是 `return null` 占位"，另一份说"~900 行、100+ 操作码、完整"。实际为 1,897 行 / ~65 条分派分支 / 35% 完成度——**两份都不对**。

## 附录 C：移交独立任务的清单（D3）

以下内容**不在本方案范围**，应由独立的 JIT/Photon 任务跟踪：

1. **JIT 派发桥接**：`VmJitBridge.aura`（344 行，已完整实现但未接线）→ 纯 Aura + Photon
2. **JIT IR 生成**：`compiler/jit/`（`JitLower.aura`/`JitState.aura`/`JitDispatch.aura`/`JitOpt.aura`/`JitAbi.aura`/`DispatchTable.aura`）→ 替换为 Photon IR
3. **JIT FFI 边界**：`bootstrap/jit_ffi.rs`（328 行）→ 按 D1 **保留在库中但不得被引用**，由 Photon 进程内编译取代；P0.1 的隔离守卫（§2.5）负责防止它在 JIT 任务中被接回生产路径
4. **AOT 运行时**：`vm/aot_runtime.rs`（1,034 行）+ `vm/mmap_util.rs`（263 行）→ Photon + `PhotonNativeWriter`
5. **JIT 后端替换**：Cranelift crate → Photon 自研后端
6. **AOT 编译器后端**：LLVM llc/clang → Photon

**本方案对其的唯一约束**：P0.8 在 `Call`/`CallNative` 分派路径预留**稳定的可插拔加速派发钩子接口**（仅接口签名，不实现），供上述任务对接。
