# Aura VM 功能规格清单

> 分析时间：2026-06-12 | 分析范围：`rust/compiler/src/vm/` 全部 26 个子模块
> 用途：作为「用纯 Aura 重写 VM」的目标规范，可直接用作验收清单。

---

## 1. 指令集完整枚举（Instr Enum）

**文件**：`rust/compiler/src/vm/mod.rs` 行 196–388
**总变体数**：**106**

### 按语义分组

| 分组 | 变体 | 操作数 | 数量 |
|------|------|--------|------|
| **常量/变量** | `LoadConst(u16)`, `LoadVar(u16)`, `StoreVar(u16)` | 各1 | 3 |
| **算术** | `Add`, `Sub`, `Mul`, `Div`, `Rem`, `Neg`, `Not` | 0 | 7 |
| **逻辑/位运算** | `And`, `Or`, `BitAnd`, `BitOr`, `BitXor`, `Shl`, `Shr` | 0 | 7 |
| **比较** | `Eq`, `Ne`, `Lt`, `Gt`, `Le`, `Ge` | 0 | 6 |
| **控制流** | `Jump(usize)`, `JumpIfTrue(usize)`, `JumpIfFalse(usize)` | 各1 | 3 |
| **函数调用** | `Call(u16)`, `CallNative(u16)`, `CallNativeArgs(u16,u16)`, `Return`, `ReturnUnit` | 1~2 | 5 |
| **对象/数组** | `NewObject(u16)`, `NewArray`, `GetField(u16)`, `SetField(u16)`, `GetIndex`, `SetIndex` | 0~1 | 6 |
| **引用计数(ARC)** | `IncRef`, `DecRef`, `Retain`, `Release`, `DropRef` | 0 | 5 |
| **方法/接口** | `CallMethod(u16)`, `CallCtor(u16)` | 各1 | 2 |
| **类型检查** | `InstanceOf(u16)`, `CheckCast(u16)` | 各1 | 2 |
| **集合** | `NewList`, `NewMap`, `ListPush`, `ListPop`, `ListLen`, `MapSet`, `MapGet`, `MapLen` | 0 | 8 |
| **协程** | `Yield`, `NewCoroutine(u16)`, `ResumeCoroutine` | 0~1 | 3 |
| **弱引用** | `WeakRef`, `WeakGet` | 0 | 2 |
| **显式堆分配** | `BoxAlloc` | 0 | 1 |
| **defer** | `DeferBegin`, `DeferEnd` | 0 | 2 |
| **终止** | `Halt` | 0 | 1 |
| **FFI(C)** | `CallC(u16)`, `CString`, `ReadCStr`, `PtrIsNull`, `PtrToInt`, `IntToPtr`, `MakeCallback(u16)` | 0~1 | 7 |
| **闭包** | `MakeClosure(u16)`, `CallClosure` | 0~1 | 2 |
| **枚举** | `EnumConstruct(u16)`, `EnumTag` | 0~1 | 2 |
| **函数引用** | `MakeFnRef(u16)` | 1 | 1 |
| **跨模块** | `CallExport(u16)`, `CallExternal(u16,u16)` | 1~2 | 2 |
| **AOT** | `CallAot(u16)` | 1 | 1 |
| **异常处理** | `PushHandler(usize,u16,u16)`, `PopHandler` | 0~3 | 2 |
| **并发(线程)** | `ThreadSpawn(u16)`, `ThreadJoin`, `ThreadSleep`, `ThreadId`, `ThreadParallelism` | 0~1 | 5 |
| **并发(Mutex)** | `MutexNew`, `MutexLock`, `MutexUnlock`, `MutexTryLock` | 0 | 4 |
| **并发(Atomic)** | `AtomicNew`, `AtomicLoad`, `AtomicStore`, `AtomicAdd`, `AtomicCas` | 0 | 5 |
| **并发(RwLock)** | `RwLockNew`, `RwLockReadLock`, `RwLockWriteLock`, `RwLockReadUnlock`, `RwLockWriteUnlock` | 0 | 5 |
| **并发(Channel)** | `ChannelNew`, `ChannelSend`, `ChannelRecv` | 0 | 3 |
| **并发(Condvar)** | `CondvarNew`, `CondvarWait`, `CondvarSignal`, `CondvarBroadcast` | 0 | 4 |

**小计**: 106 个变体（精确计数，行196-388）

---

## 2. 值表示（Value）与堆（Heap）

### 2.1 Value 枚举
**文件**：`rust/compiler/src/vm/value.rs` 行 1–140
**结构**：`#[derive(Clone, Debug)] enum Value`

| 变体 | 类型 | 说明 |
|------|------|------|
| `Int(i64)` | 内联 | 64位有符号整数（对应 Int/Long） |
| `Float(f64)` | 内联 | 64位浮点 |
| `Bool(bool)` | 内联 | 布尔 |
| `Str(Rc<str>)` | 引用计数 | UTF-8 字符串，`Rc` 共享 |
| `Null` | 单元 | 空值（同时充当 Unit/Nothing） |
| `Ref(usize)` | 堆句柄 | 堆对象/数组索引 |
| `Weak(usize)` | 弱引用句柄 | 不增加引用计数 |
| `Ptr(i64)` | 原始指针 | C ABI 互操作，0=nullptr |
| `List(Vec<Value>)` | 堆外 | 列表（内联值语义） |
| `Map(HashMap<Value,Value>)` | 堆外 | 映射（内联值语义） |

**关键方法**：`is_truthy()`, `is_null_ptr()`, `as_int()`, `as_float()`, `as_bool()`, `as_ptr()`, `as_string()`, `type_name()`

**实现**: `impl PartialEq for Value`（跨类型数值比较、指针/Null 等价）
**实现**: `impl Eq + Hash`（支持 Map 键）
**实现**: `impl fmt::Display`
**实现**: `impl serde::Serialize + Deserialize`（Phase 3 IPC 支持）

**复杂度**：低（纯数据结构定义）

### 2.2 Heap 堆管理器
**文件**：`rust/compiler/src/vm/heap.rs` 行 1–498

**堆对象类型（HeapData enum）**：
| 变体 | 说明 |
|------|------|
| `Object { type_tag, fields, vtable }` | 类实例：字段名 FNV 哈希 → 值 |
| `Array(Vec<Value>)` | 定长数组 |
| `List(Vec<Value>)` | 动态列表（堆表示） |
| `Map(HashMap<Value,Value>)` | 哈希映射 |
| `Closure { func_name, param_count, locals, captures, func_idx }` | 闭包 |
| `Enum(u16)` | 枚举变体 |
| `FnRef(usize)` | 函数引用 |

**引用计数**：ARC（Allocation-based Reference Counting）
- `HeapSlot { rc: usize, data: Option<HeapData>, drop_cb: Option<fn(Value)> }`
- `alloc()` → 新分配（含空闲链表复用）
- `inc_ref()` / `dec_ref()` → 引用计数 ±1，归零回收
- `drop_ref()` → 显式释放 + 触发 drop 回调
- **无 GC**：纯 ARC，无垃圾回收扫描

**泄漏检测**：`leak_report()` → 统计总分配/活跃/泄漏对象数

**复杂度**：低（简单槽式分配器）

---

## 3. 解释执行循环

**文件**：`rust/compiler/src/vm/interp.rs` 行 1–2468
**主循环**：`Vm::run()` → `Vm::step()` → `Vm::exec_instr()`

### 3.1 主循环结构（行72-150）
```
run() {
    push_frame(entry)
    while !frames.is_empty() && !halt {
        step()  // 对栈顶帧执行一条指令
    }
    return result
}
```

### 3.2 栈帧管理
**Frame 结构**（mod.rs 行883-916）：
```rust
pub struct Frame {
    pub func: usize,         // 所属函数索引
    pub ip: usize,           // 当前指令索引
    pub locals: Vec<Value>,  // 局部变量槽（含参数）
    pub stack: Vec<Value>,   // 操作数栈
    pub coroutine_id: usize, // 协程 ID（0=主线程）
}
```
- **参数布局**：`locals[0]` = 函数指针占位，`locals[1..=param_count]` = 参数
- **调用**：`push_frame()` 检查 `max_call_depth`（默认4096），创建新帧
- **返回**：`pop_frame()` 弹栈，返回值压入调用者栈；空栈时设 `result` + `halt`

### 3.3 尾调用优化
**无显式 TCO**。注释提到"直接线程码"，但实际是 Rust `match` 分派，非 `computed goto`。函数调用总是 push/pop 帧，不优化尾调用。

### 3.4 异常传播（Handler 栈）
**文件**：mod.rs 行855-880, interp.rs 行514-526 + 1200-1355

**Handler 结构**：
```rust
pub struct Handler {
    pub frame_index: usize,  // 注册帧索引
    pub ip: usize,           // 处理器入口指令索引
    pub stack_len: usize,    // 注册时栈高度
    pub slot: u16,           // 异常值写入槽位（u16::MAX=不写入）
    pub catch_type: u16,     // catch 类型过滤器（u16::MAX=catch-all）
}
```

**异常传播流程**（`raise()` 行1200-1243）：
1. 从 handler 栈顶弹出处理器
2. 类型过滤：`catch_type != u16::MAX` 时检查 `is_instance_of`
3. 计算槽位值（字符串→Exception 对象包装、Exception→message 提取）
4. 帧栈截断到处理器帧，栈截断到注册高度
5. 异常值写入槽位或压栈
6. 跳转到处理器入口
7. 无处理器 → 未捕获异常，返回 `VmError::Runtime`

**复杂度**：中（多层栈截断 + 类型过滤 + 自动包装逻辑）

### 3.5 原生函数调用派发链（do_call_native，行1444-1638）
1. 特殊拦截：`__throw` → `raise()`；`__new_exception` → 创建异常对象；`Process.exit` → 请求退出
2. 可变集合工厂拦截：`mutableListOf/arrayListOf` → 堆对象
3. 原位集合写入拦截：`set/listSet/hashMapPut` → 原地修改
4. Phase D：查 `stdlib_func_map` → Aura 编译函数优先（支持 self 注入）
5. 对象原生拦截：`typeOf/aura_isOfType/aura_cast/aura_cast_safety`
6. Rust native 注册表查找
7. C FFI 静态链接（`static_call_c_with_lib`）
8. 未链接兜底：`warn_unlinked_once` + 返回 `Int(0)`

**复杂度**：高（多级派发链，每级有状态转换）

---

## 4. 原生/内置函数

**文件**：`rust/compiler/src/vm/native.rs` 行 1–1454
**注册表**：`NativeRegistry`（HashMap<String, NativeFn> + DynamicLoader）
**NativeFn 签名**：`fn(&[Value]) -> Value`

### 4.1 原生函数清单（按类别）

#### 核心 I/O（3个）
| 函数名 | 说明 |
|--------|------|
| `println` | 打印并换行 |
| `print` | 打印不换行 |
| `puts` | C 风格输出 |

#### 数学（4个）
| 函数名 | 说明 |
|--------|------|
| `abs` | 绝对值 |
| `sqrt` | 平方根 |
| `pow` | 幂运算 |
| `clock` | 当前时间戳 |

#### 类型转换（4个）
| 函数名 | 说明 |
|--------|------|
| `toInt` | 转整数 |
| `toFloat` | 转浮点 |
| `toStr` | 转字符串 |
| `toString` | `toStr` 别名 |

#### FFI/C 互操作（8个）
| 函数名 | 说明 |
|--------|------|
| `CString` / `CStr` | 字符串→C指针 |
| `ptrIsNull` | 指针空检查 |
| `ptrToInt` | 指针→整数 |
| `intToPtr` | 整数→指针 |
| `makeCallback` | 创建 C 回调蹦床 |
| `Builtin.cstr` / `cstr` | C 字符串（带缓存） |

#### 反射/类型（4个）
| 函数名 | 说明 |
|--------|------|
| `equals` | 值相等 |
| `hashCode` | 哈希码 |
| `typeOf` | 运行时类型名 |
| `aura_isOfType` | 类型检查 |

#### 类型转换（2个）
| 函数名 | 说明 |
|--------|------|
| `aura_cast` | 严格转换（失败抛异常） |
| `aura_cast_safety` | 安全转换（as? 失败返回 Null） |

#### 迭代器（2个）
| 函数名 | 说明 |
|--------|------|
| `__size` | 集合长度 |
| `__get` | 按下标取值 |

#### 辅助（2个）
| 函数名 | 说明 |
|--------|------|
| `strlen` | 字符串长度 |
| `fnIndex` | 按函数名解析索引 |

#### 内存原语（Memory，12个）
| 函数名 | 说明 |
|--------|------|
| `Memory.alloc` | 分配内存 |
| `Memory.free` | 释放内存 |
| `Memory.read` / `read16` / `read32` / `read64` | 按宽度读 |
| `Memory.write` / `write16` / `write32` / `write64` | 按宽度写 |
| `Memory.copy` | 内存复制 |
| `Memory.set` | 内存填充 |

#### CPU 原语（4个）
| 函数名 | 说明 |
|--------|------|
| `Cpu.atomicAdd` | 原子加法 |
| `Cpu.memFence` | 内存屏障 |
| `Cpu.rdtsc` | 时间戳计数器（占位） |
| `Cpu.cpuid` | CPU 信息（占位） |

#### 并发（Concurrent）原生函数（20个）
| 函数名 | 说明 |
|--------|------|
| `Coroutine.spawn` | 创建协程 |
| `Coroutine.ask` | 协程请求 |
| `Actor.send` | 向 Actor 发消息 |
| `Actor.reply` | 回复 Actor |
| `Actor.spawnActor` | 创建 Actor |
| `Actor.supervise` | 建立监督关系 |
| `Actor.actorAlive` | 检查 Actor 存活 |
| `Channel.newChannel` | 创建 Channel |
| `Channel.channelSend` | 发送 |
| `Channel.channelRecv` | 接收（阻塞） |
| `Channel.channelTryRecv` | 接收（非阻塞） |
| `Channel.select` | 多路复用 |
| `Channel.selectTimeout` | 带超时 select |
| `Actor.spawnActorProcess` | 跨进程 Actor |
| `Actor.sendProcessActor` | 跨进程发送 |
| `Actor.recvProcessActor` | 跨进程接收 |
| `Actor.processActorAlive` | 跨进程存活检查 |
| `Actor.killProcessActor` | 跨进程终止 |
| `Channel.newTcpChannel` | TCP Channel |
| `Channel.tcpChannelSend` | TCP Channel 发送 |

#### 线程桥（ThreadOps，5个）
| 函数名 | 说明 |
|--------|------|
| `ThreadOps.create` | 创建 OS 线程 |
| `ThreadOps.join` | 等待线程 |
| `ThreadOps.sleepMs` | 线程休眠 |
| `ThreadOps.currentId` | 当前线程 ID |
| `ThreadOps.cores` | 可用核心数 |

#### 并发同步原语（concurrent_native.rs 中实现的注册函数）
- `Thread.spawn` / `Thread.join` / `Thread.sleep` / `Thread.id` / `Thread.parallelism` / `Thread.availableCores`
- `Mutex.new` / `Mutex.lock` / `Mutex.unlock` / `Mutex.tryLock`
- `Atomic.new` / `Atomic.load` / `Atomic.store` / `Atomic.add` / `Atomic.cas`
- `RwLock.new` / `RwLock.readLock` / `RwLock.writeLock` / `RwLock.readUnlock` / `RwLock.writeUnlock`
- `Condvar.new` / `Condvar.wait` / `Condvar.signal` / `Condvar.broadcast`
- `Barrier.new` / `Barrier.wait`
- `Semaphore.new` / `Semaphore.acquire` / `Semaphore.tryAcquire` / `Semaphore.release`

### 4.2 完整注册清单
| 类别 | 独立函数数（去别名） | 含别名/全名总数 |
|------|---------------------|----------------|
| 核心 prelude | 16 | 32（含 aura.lang.std.* 别名） |
| FFI 别名 | 6 | 6（aura.ffi.*） |
| 并发 | 20 | 23（含 Coroutine.spawnActor 别名） |
| Memory/Cpu | 16 | 16 |
| Thread 桥 | 5 | 5 |
| 并发同步原语 | ~30 | ~30 |
| **合计** | **~93** | **~112** |

**复杂度**：中（单个函数实现简单，但类别繁多）

---

## 5. 并发运行时

### 5.1 Actor 运行时
**文件**：`actor.rs` 行 1–393
**结构**：`ActorRuntime { actors: Vec<Option<Actor>>, pending_requests, response_queue, response_notifier }`
**Actor 实例**：`Actor { id, name, mailbox: VecDeque<Value>, state: HashMap<String,Value>, alive, parent, children, death_strategy, death_reason }`
**能力**：
- `spawn(name)` → 创建 Actor，返回 ID（从1开始）
- `send(id, msg)` → 非阻塞发送
- `ask(id, msg)` / `ask_with_timeout(id, msg, timeout)` → 精确阻塞请求（EventNotifier 零轮询）
- `reply(request_id, response)` → 回复请求
- `supervise(parent, child)` → 建立监督关系
- `is_alive(id)` → 存活检查
- 死亡策略：`Restart` / `Escalate` / `Terminate`
**依赖 OS 原语**：`EventNotifier`（epoll/select/eventfd）

### 5.2 跨进程 Actor
**文件**：`actor_process.rs` 行 1–200
**能力**：子进程 Actor（`spawn`/`send`/`recv`/`is_alive`/`kill`）
**依赖**：`std::process::Command`、TCP socket

### 5.3 Channel 运行时
**文件**：`channel.rs` 行 1–206
**结构**：`ChannelRuntime { channels: Vec<Option<Channel>>, next_id }`
**Channel 实例**：`Channel { id, bound, buffer: VecDeque<Value>, notifier: Arc<dyn EventNotifier> }`
**能力**：
- `new_channel(bound)` → 有界/无界
- `send(id, val)` → 非阻塞（满时返回 false）
- `recv(id)` → 精确阻塞（EventNotifier.wait）
- `try_recv(id)` → 非阻塞
- `recv_timeout(id, timeout)` → 带超时阻塞
- `len(id)` / `is_empty(id)`
**依赖 OS 原语**：`EventNotifier`（epoll/select/eventfd）

### 5.4 TCP Channel
**文件**：`channel_tcp.rs` 行 1–170
**能力**：跨进程 Channel 的 TCP 实现（`TcpChannelServer::new(port)` / `new_any()`）

### 5.5 并发同步原语（concurrent_native.rs，1077行）
| 原语 | 实现方式 | 注册表 |
|------|---------|--------|
| `RawMutex` | `AtomicBool + Condvar` | `RAW_MUTEX_REGISTRY` |
| `RawRwLock` | `AtomicI64 ×2 + Condvar` | `RWLOCK_REGISTRY` |
| `AtomicI64` | `std::sync::atomic` | `ATOMIC_REGISTRY` |
| `RawCondvar` | `Condvar + Mutex<()>` | `CONDVAR_REGISTRY` |
| `RawBarrier` | `AtomicI64 + Condvar` | `BARRIER_REGISTRY` |
| `RawSemaphore` | `Mutex<i64> + Condvar` | `SEMAPHORE_REGISTRY` |
| `ThreadEntry` | `JoinHandle + mpsc::Receiver` | `THREAD_REGISTRY` |

**依赖 OS 原语**：`std::sync`（Mutex/Condvar/atomic）、`std::thread`、`std::sync::mpsc`

### 5.6 协程调度器
**文件**：`coroutine.rs` 行 1–153
**结构**：`CoroutineScheduler { coroutines: Vec<Option<Coroutine>>, ready_queue, next_id }`
**能力**：`spawn` / `save_frames`（挂起） / `restore_frames`（恢复） / `mark_done` / `take_done_value`
**依赖**：无 OS 原语（纯 Rust 数据结构，帧栈快照）

### 5.7 事件通知器
**文件**：`event_notifier.rs`（约 500 行）
**能力**：跨平台事件等待（epoll/select/eventfd），消除轮询空转

### 5.8 IPC
**文件**：`ipc.rs`（约 150 行）
**能力**：跨进程 IPC 通信基础

### 5.9 线程池
**文件**：`thread_pool.rs`（约 120 行）
**能力**：固定大小线程池

**复杂度**：高（多原语交叉，EventNotifier 跨平台，线程安全约束复杂）

---

## 6. JIT 编译

**文件**：`jit.rs` 行 1–1462
**集成方式**：Cranelift IR builder → 机器码
**JitState 结构**：
```rust
pub struct JitState {
    compiled: HashMap<usize, JitEntry>,
    skipped: HashMap<usize, String>,
    dispatch_table: Vec<Option<JitEntry>>,
}
```

### 6.1 热点检测
- **阈值**：`VmOptions::hotspot_threshold`（默认 **10,000** 次）
- **检测点**：`push_frame()` 和 `do_call()` 中
- **强制编译**：入口函数首次运行强制 JIT（`force_jit_compile`）
- **编译白名单**：`is_jit_compilable()` 递归检查所有被调函数，仅允许：
  - 整数常量 + 算术/比较/跳转/返回/调用原生
  - 逻辑/位运算、集合分配、ARC no-op
  - 并发指令（Thread/Mutex/Atomic/RwLock/Channel/Condvar）
  - 非叶子整数函数

### 6.2 编译流程
1. `compile_function(idx, f, consts, funcs)` → 白名单检查
2. `optimize_function_with_deps(f, consts, funcs)` → JIT 优化（`jit_opt.rs`）
3. `cranelift_backend::jit_compile_cranelift(func, consts, funcs)` → Cranelift IR → 机器码
4. 成功 → `JitState::insert(idx, entry)`；失败 → `JitState::skip(idx, reason)`

### 6.3 编译失败回退
- 记录跳过原因（skip_reason）
- 永久回退解释器（不重试）
- 典型跳过原因："JIT whitelist mismatch"

### 6.4 全局编译缓存
`GlobalJitCache`（Mutex<HashMap<String, JitEntry>>）：按函数代码哈希缓存，多线程复用

### 6.5 JIT 优化（jit_opt.rs，约800行）
- 常量传播/折叠
- 死代码消除
- 依赖分析（`optimize_function_with_deps` 递归处理调用链）

### 6.6 JIT 原生桥（jit_native.rs，约120行）
- `set_native_registry` / `set_natives_ptr`：设置全局原生注册表指针
- `aura_jit_dispatch`：C ABI 派发函数（JIT 代码调用 VM 原生函数）

**复杂度**：高（Cranelift IR 构建、白名单递归检查、优化 pass）

---

## 7. AOT 运行时

**文件**：`aot_runtime.rs` 行 1–1134
**结构**：
```rust
pub struct AotRuntime {
    modules: HashMap<u32, AotModule>,
    next_module_id: u32,
    entry_cache: HashMap<usize, u32>,
    cross_module_symbols: HashMap<String, CrossModuleSymbol>,
    module_dependencies: HashMap<u32, Vec<ModuleDependency>>,
    shared_lib_handles: HashMap<u32, Box<dyn Any + Send + Sync>>,
    func_name_map: HashMap<u32, HashMap<String, usize>>,
}
```

### 7.1 机器码段管理
- **AotModule::load**：
  1. 从 `.auc` 提取 `SEG_MACHINE` + `SEG_DESC_TABLE`
  2. `MappedRegion::map_anonymous(size, RW)` → 复制机器码 → `protect(RX)`
  3. 解析描述符表（`AuraFuncDesc`：name_offset/len, entry_offset, num_args, arg_tags, return_tag, flags）
  4. 重建 dispatch_table（entry_offset + 16字节对齐校验）
- **W^X 保护**：`mmap_util::MappedRegion` 提供 `protect()` 切换权限

### 7.2 dispatch_table 派发
- `AotRuntime::call_func(module_id, func_idx, args)` → 查 dispatch_table → `AotEntry` 函数指针
- 签名：`AotEntry = unsafe extern "C" fn(*const JitValue, *mut JitValue, usize, *const ())`
- 与 JIT 共享 JitValue ABI（零 FFI 开销）
- `AOT_MAX_CALL_DEPTH = 1024`（Phase 3.2 深度上限）

### 7.3 跨模块调用
- `ModuleDependency`：模块依赖描述
- `CrossModuleSymbol`：跨模块符号表
- `load_shared_library()`（Tier 2）：dlopen → 解析导出符号 → `aura_aot_*` 枚举 → dlsym 获取函数地址

### 7.4 mmap_util
**文件**：`mmap_util.rs`（约 300 行）
**能力**：`MappedRegion`（匿名映射/文件映射）、`MemoryProtection`（RW/RX/R/RW+）、跨平台封装

**复杂度**：高（W^X 内存保护、跨平台 mmap、符号表解析）

---

## 8. FFI 机制

### 8.1 回调蹦床（ffi.rs，396行）
**文件**：`rust/compiler/src/vm/ffi.rs`
**机制**：
- `CallbackRegistry`：全局回调注册表（Mutex<Vec<CallbackEntry>>）
  - `register(func_idx)` → 返回 callback_id（1-based）
  - `lookup(callback_id)` → 返回 func_idx
- `CallbackDispatcher`：`Arc<dyn Fn(i64, &[i64]) -> i64 + Send + Sync>`
- **thread_local fast path**：`CURRENT_DISPATCHER` 优先
- **全局栈后备**：`DISPATCHER_STACK`（Mutex<Vec<CallbackDispatcher>>）支持跨线程
- `aura_callback_trampoline(context, a1..a8) -> i64`：C ABI 蹦床（最多8参数）
  - `context` 编码 callback_id
  - 通过 dispatcher 回调 VM 的 `call_callback()`

### 8.2 C 函数调用（P8.4 静态链接）
- `resolve_static_symbol(name)` → dlsym/GetProcAddress
- `static_call_c_with_lib(name, args, lib_handle, param_types, ret_type)` → 通用 C 函数调用
- `CType` 枚举：Int32/Int64/Float32/Float64/Bool/Char/CString/Ptr/Vo

### 8.3 FFI 缓存（ffi_cache.rs，约130行）
- `FfiCache`：C 函数指针缓存，避免重复 dlsym
- 线程安全（Mutex）

### 8.4 动态 FFI（dynamic_ffi.rs，约100行）
- `DynamicLoader`：`load_lib(path, abi)` → 动态加载库
- `resolve_func(name)` → 从已加载库解析函数
- `register_func(name, f)` → 注册原生函数

**复杂度**：中（蹦床机制清晰，但 thread_local + 全局栈双层派发有微妙语义）

---

## 9. 调试器

**文件**：`debugger.rs` 行 1–1297
**模式**：`DebugMode::Vm` / `DebugMode::Jit` / `DebugMode::Aot`

### 9.1 核心能力
| 能力 | 实现 |
|------|------|
| **断点** | `Breakpoint { id, target: Line/Function/JitFunc/AotSymbol, resolved_func_idx, resolved_instr_idx, enabled, hit_count, condition, mode }` |
| **单步** | `StepMode::In`（步进）、`Over`（步过）、`Out`（步出） |
| **运行/继续** | `continue_execution()` 清除暂停状态 |
| **条件断点** | `condition: Option<String>`（预留，当前未实现条件求值） |
| **停止原因** | `StopReason::Breakpoint / Step / Completion / Error / JitCompiled / AotCompiled` |

### 9.2 源代码映射
- `SourceMapping::from_module(module)` → 从 `line_table` 建立指令级行号映射
- `line_to_instr(line)` → `(func_idx, instr_idx)`
- `func_for_line(line)` → 函数信息
- `func_idx_by_name(name)` → 按名查找

### 9.3 寄存器读取
- 通过 `Vm::frames()` 直接读取帧栈：`func`, `ip`, `locals`, `stack`
- `debug_step()` 单步执行一条指令

### 9.4 栈回溯
- `Vm::frames()` 返回 `&Vec<Frame>`，每帧含函数名和 ip
- 可从帧栈推导调用栈

### 9.5 JIT/AOT 调试
- `JitDebugInfo { func_states, total_compile_count, fallback_count }`
- `JitFuncState { func_idx, func_name, is_compiled, is_skipped, skip_reason, call_count }`
- `AotDebugInfo { ll_path, exe_path, dwarf_functions, external_debugger, supports_breakpoints }`

**复杂度**：中（基础设施完整，但条件断点未实现）

---

## 10. 模块与 ABI

### 10.1 跨模块调用（multi_module.rs，266行）
**文件**：`rust/compiler/src/vm/multi_module.rs`
**MultiModuleVm 结构**：
```rust
pub struct MultiModuleVm {
    modules: HashMap<String, BytecodeModule>,
    entry_module: String,
    symbol_table: HashMap<String, (String, usize)>,
}
```
**能力**：
- `load_module(name, module)` → 注册导出符号
- `resolve_symbol(name)` → 跨模块符号查找
- `has_symbol(name)` → 符号存在性检查
- `load_modules_from_dir(vm, dir, entry)` → 从目录加载

**CallExternal 实现**（interp.rs 行796-820）：
1. 从 imports 表查找目标模块
2. 从 ModuleRegistry 查找已加载模块
3. 从导出索引获取函数索引
4. `do_call()` 调用

### 10.2 模块注册表（mod.rs 行100-148）
**ModuleRegistry**：`{ modules: HashMap<[u8;16], RegisteredModule>, name_index: HashMap<String, [u8;16]> }`
- `register()` / `find_by_uuid()` / `find_by_name()` / `find_export()` / `list_modules()`
- UUID = 16字节数组，标识模块唯一身份

### 10.3 调用约定 ABI（abi.rs，210行）
**文件**：`rust/compiler/src/vm/abi.rs`
**JitValue**（`#[repr(C)]` 双字段结构）：
```rust
pub struct JitValue {
    pub tag: i64,      // 类型标签
    pub payload: i64,  // 值载荷
}
```
**类型标签常量**：
| 常量 | 值 | 说明 |
|------|-----|------|
| `TAG_INT` | 0 | 整数 |
| `TAG_FLOAT` | 1 | 浮点 |
| `TAG_BOOL` | 2 | 布尔 |
| `TAG_NULL` | 3 | 空 |
| `TAG_STR` | 4 | 字符串指针 |
| `TAG_PTR` | 5 | 原始指针 |
| `TAG_OBJ` | 6 | 堆对象 |
| `TAG_FUNC` | 7 | 函数 |
| `TAG_ARRAY` | 8 | 数组 |
| `TAG_LIST` | 9 | 列表 |
| `TAG_MAP` | 10 | 映射 |
| `TAG_CLOSURE` | 11 | 闭包 |
| `TAG_CSTRING` | 12 | C 字符串 |

**AotEntry**：`unsafe extern "C" fn(*const JitValue, *mut JitValue, usize, *const ())`
**AotCallContext**：`{ runtime, module_id, func_idx, call_depth, exception }`

**复杂度**：低（纯数据结构定义，但跨 VM/JIT/AOT 共享，是关键基础设施）

---

## 附录

### A. 总指令数
**106** 个 Instr 变体

### B. 总原生函数数
- **去别名**：约 **93** 个独立原生函数
- **含全部别名/全名**：约 **112** 个注册条目

### C. 总代码行数
| 文件 | 实际行数 |
|------|---------|
| mod.rs | 1,890 |
| interp.rs | 2,468 |
| native.rs | 1,454 |
| jit.rs | 1,462 |
| aot_runtime.rs | 1,134 |
| debugger.rs | 1,297 |
| concurrent_native.rs | 1,077 |
| heap.rs | 498 |
| value.rs | 388 |
| abi.rs | 210 |
| coroutine.rs | 153 |
| actor.rs | 393 |
| channel.rs | 206 |
| multi_module.rs | 266 |
| ffi.rs | 396 |
| mmap_util.rs | ~300 |
| serialize.rs | ~300 |
| jit_opt.rs | ~800 |
| jit_native.rs | ~120 |
| event_notifier.rs | ~500 |
| ipc.rs | ~150 |
| actor_process.rs | ~200 |
| channel_tcp.rs | ~170 |
| thread_pool.rs | ~120 |
| ffi_cache.rs | ~130 |
| dynamic_ffi.rs | ~100 |
| **总计** | **约 12,900** |

### D. 纯 Aura 难以等价实现的部分（风险预判）

| 模块 | 风险等级 | 原因 |
|------|---------|------|
| **FFI/C 回调蹦床** | 🔴 极高 | 需要 C ABI、函数指针、thread_local 派发器，纯 Aura 无法直接操作裸指针和函数指针 |
| **JIT (Cranelift)** | 🔴 极高 | 需要 IR 构建、机器码生成、内存保护（W^X），纯 Aura 无法生成原生代码 |
| **AOT 运行时** | 🔴 极高 | mmap 映射、机器码段加载、dispatch_table 函数指针，纯 Aura 无法操作 |
| **mmap_util** | 🔴 极高 | 系统调用级内存映射，纯 Aura 无法直接调用 |
| **C 函数静态链接** | 🔴 极高 | dlsym/GetProcAddress，纯 Aura 无法解析 C 符号 |
| **EventNotifier** | 🔴 高 | epoll/select/eventfd 系统调用，纯 Aura 无 OS 原语 |
| **并发同步原语** | 🔴 高 | RawMutex/RawRwLock/RawCondvar 依赖 AtomicBool + Condvar，纯 Aura 无原子操作 |
| **线程管理** | 🟡 中高 | OS 线程创建/JoinHandle/mpsc 通道，纯 Aura 无进程/线程 API |
| **Memory/Cpu 原语** | 🟡 中 | 裸内存读写、原子加法、内存屏障，需要底层指针操作 |
| **协程调度器** | 🟢 中 | 帧栈快照 + 轮转调度，纯 Aura 可实现（状态机模式） |
| **Actor 模型** | 🟢 低-中 | 消息队列 + 监督树，纯 Aura 可实现 |
| **Channel** | 🟢 低 | VecDeque + 简单同步，纯 Aura 可实现 |
| **Value/Heap** | 🟢 低 | 纯数据结构，ARC 可实现 |
| **解释执行循环** | 🟢 低 | 栈式字节码解释，纯 Aura 可实现 |
| **异常处理** | 🟢 低-中 | Handler 栈 + 栈截断，纯 Aura 可实现但复杂度高 |
| **调试器** | 🟡 中 | 断点/单步基础设施可实现，但 JIT/AOT 调试不可行 |
| **模块系统** | 🟢 低 | 符号表 + 跨模块查找，纯 Aura 可实现 |

### E. 结论
纯 Aura 重写 VM 的**可行范围**约占总代码量的 **40-50%**（Value/Heap/解释器/异常/Actor/Channel/协程/模块系统），其余 **50-60%**（JIT/AOT/FFI/并发同步/mmap/C FFI）必须保留 Rust 实现或降级为纯 Aura 标准库调用。**最大障碍**是 OS 系统调用和机器码执行能力。
