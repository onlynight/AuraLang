# Aura JIT 模式优化方案

> **文档定位**：JIT 模式（Cranelift）的优化现状、已完成项、待完成项与性能目标
> **配套文档**：`01-现状分析.md`（整体纯 Aura 化进度）、`02-技术方案.md`（架构设计）
> **关联分析**：`docs/JIT性能分析.md`（P5 遗留问题与 Fix A/B）、`docs/jit优化指南.md`（栈式 vs 寄存器式 VM）
> **日期**：2026-09-13

---

## 一、优化现状总览

### 1.1 已完成（P5–P7）

| 阶段 | 优化项 | 位置 | 效果 |
|------|--------|------|------|
| P5 | Fix A：入口函数强制编译 | `vm/mod.rs::force_jit_compile` | sum(60000) 从 1.0x → **115x** |
| P5 | Fix B：递归函数 JIT 支持 | `vm/jit.rs` dispatch_table + call_indirect | fib(25) 从 1.0x → **91x** |
| P7 | 7 个字节码优化 pass | `vm/jit_opt.rs` | 编译质量提升（见 §1.2） |
| P7 | Cranelift speed 优化标志 | `vm/jit.rs` `opt_level: "speed"` | 编译延迟 + 执行性能 |

### 1.2 七个优化 pass 详解

`compiler/src/vm/jit_opt.rs:146-188`：

| # | Pass | 函数 | 作用 | 典型收益 |
|---|------|------|------|----------|
| 1 | **常量折叠** | `fold_constants` | 编译期计算常量算术/比较 | 消除运行时计算 |
| 2 | **死码消除** | `eliminate_dead_code` | 删除 LoadVar/StoreVar 同槽对 | 减少指令数 10-30% |
| 3 | **跳转线程化** | `thread_jumps` | 消除「Jump 紧跟 Jump」的冗余跳转 | 简化控制流 |
| 4 | **强度削弱** | `strength_reduce` | `/2^n`→`>>n`、`*2^n`→`<<n` | 除法变移位 |
| 5 | **指令调度** | `schedule_instructions` | 独立 Load 重排，提高 ILP | 减少 CPU 停顿 |
| 6 | **函数内联** | `inline_functions` | 内联 <20 指令的小函数 | 消除调用开销 |
| 7 | **循环展开** | `unroll_loops` | 2x 展开简单循环 | 减少分支开销 |

### 1.3 当前性能（P7 优化后）

| 场景 | VM（解释器） | JIT（Cranelift） | AOT（LLVM） | JIT 加速 | AOT 加速 |
|------|-------------|------------------|-------------|----------|----------|
| sum(60000) | 55.7 ms/op | **0.51 ms/op** | 4.53 ms/op | **109x** | 12x |
| fib(25) | 230 ms/op | **2.97 ms/op** | 5.52 ms/op | **78x** | 42x |

**关键发现**（`docs/JIT性能分析.md:175-179`）：
- JIT 在两个场景中均**快于 AOT**
- fib(25)：JIT 是 AOT 的 **1.86x** 快
- sum(60000)：JIT 是 AOT 的 **8.9x** 快
- 原因：AOT 基准每次迭代启动新进程（~4ms 开销），JIT 在进程内执行

---

## 二、JIT 专属优化（与 AOT 的区别）

### 2.1 JIT 独有优势

| 维度 | JIT | AOT | JIT 优势 |
|------|-----|-----|----------|
| **编译时机** | 运行时（热点触发） | 编译期（一次性） | JIT 可针对实际运行数据优化 |
| **进程开销** | 0（进程内） | ~4ms（fork 子进程） | JIT 无进程启动开销 |
| **增量优化** | 支持（逐步优化热点） | 不支持（一次性） | JIT 可 OSR（On-Stack Replacement） |
| **编译延迟** | ~1-5ms | ~200-2000ms | JIT 毫秒级响应 |
| **去重编译** | ❌ 每 VM 独立编译 | ✅ 编译一次全局共享 | AOT 更优 |

### 2.2 JIT 独有劣势

| 维度 | JIT | AOT | 影响 |
|------|-----|-----|------|
| **优化深度** | Cranelift 基线（浅） | LLVM O3（深） | AOT 稳态吞吐更高 |
| **编译去重** | ❌ N 线程编译 N 次 | ✅ 编译一次共享 | JIT 浪费 CPU |
| **冷启动** | 首次运行需编译 | 编译期已完成 | AOT 冷启动快 |
| **调试支持** | DWARF 有限 | 完整 DWARF | AOT 调试更好 |

---

## 三、待完成优化项

### 3.1 P8：JIT 去重编译（消除 N 倍浪费）

**问题**：多线程场景下，同一函数被 N 个 VM 实例各编译一次，浪费 CPU。

**方案**：
- 引入全局编译缓存（`Mutex<HashMap<String, JitEntry>>`）
- 编译前查缓存，命中则复用；未命中则编译并写入缓存
- 需处理线程安全（编译锁 + 双重检查锁）

**预期收益**：N 线程场景下编译 CPU 浪费从 N 倍降为 1 倍。

### 3.2 P8：OSR（On-Stack Replacement）

**问题**：当前 JIT 只能编译完整函数，无法替换正在执行的函数中间状态。

**方案**：
- 检测循环热点（循环体执行次数超过阈值）
- 在循环入口处触发 OSR，将当前栈帧替换为 JIT 编译的循环体
- 需要栈帧转换（VM 栈帧 → JIT 栈帧）

**预期收益**：覆盖「入口函数内的循环热点」（Fix A 已部分覆盖，但 OSR 更精确）。

### 3.3 P8：逃逸分析 + 标量替换

**问题**：当前 JIT 无法将堆对象替换为栈变量，所有对象分配都走堆。

**方案**：
- 分析对象生命周期，若未逃逸函数边界则分配到栈上
- 需 Cranelift 栈槽（`StackSlot`）支持
- 与 ARC 引用计数交互（逃逸分析后无需 Retain/Release）

**预期收益**：消除堆分配开销，GC 压力降低。

### 3.4 P9：类型反馈 + 投机内联

**问题**：当前 JIT 无法利用运行时类型信息做投机优化。

**方案**：
- 在字节码中插入类型反馈点（记录运行时实际类型）
- JIT 编译时利用类型反馈做单态/双态推测
- 投机失败时去优化（回退解释器）

**预期收益**：虚方法调用性能提升（从 vtable 查找变为直接调用）。

### 3.5 P9：多线程 JIT 编译池

**问题**：当前 JIT 编译在主线程执行，编译期间阻塞程序执行。

**方案**：
- 引入 JIT 编译线程池（类似 Java 的 C2 编译线程）
- 热点检测后提交编译任务，主线程继续执行解释器
- 编译完成后原子替换分发表条目

**预期收益**：消除编译期间的执行停顿。

### 3.6 P10：Profile-Guided 优化

**问题**：当前 JIT 无运行时 profile 数据，优化决策基于静态分析。

**方案**：
- 收集运行时 profile（分支预测、循环次数、类型分布）
- 根据 profile 数据调整优化策略
- 支持多级优化（Tier 1: C1 快速 → Tier 2: C2 深度）

**预期收益**：稳态性能向 AOT/LLVM 靠拢。

---

## 四、性能目标

### 4.1 短期（P8，2-3 周）

| 指标 | 当前 | 目标 |
|------|------|------|
| 编译去重 | ❌ 无 | ✅ 全局缓存，N 线程只编译 1 次 |
| OSR | ❌ 无 | ✅ 循环体 OSR |
| 逃逸分析 | ❌ 无 | ✅ 基础逃逸分析（函数内对象） |
| sum(60000) | 0.51 ms/op | <0.3 ms/op |
| fib(25) | 2.97 ms/op | <2.0 ms/op |

### 4.2 中期（P9，4-6 周）

| 指标 | 当前 | 目标 |
|------|------|------|
| 类型反馈 | ❌ 无 | ✅ 单态/双态推测 |
| 投机内联 | ❌ 无 | ✅ 基于类型反馈 |
| 编译线程池 | ❌ 主线程编译 | ✅ 后台编译，主线程不阻塞 |
| 虚方法调用 | vtable 查找 | 直接调用（投机） |
| sum(60000) | 0.51 ms/op | <0.2 ms/op |

### 4.3 长期（P10，8-12 周）

| 指标 | 当前 | 目标 |
|------|------|------|
| Profile-Guided | ❌ 无 | ✅ 运行时 profile 驱动优化 |
| 多级优化 | ❌ 单级（C1） | ✅ 两级（C1 快速 → C2 深度） |
| 去优化（deopt） | ❌ 仅回退解释器 | ✅ 回退到 C1 编译 |
| sum(60000) | 0.51 ms/op | <0.1 ms/op |
| fib(25) | 2.97 ms/op | <1.0 ms/op |

---

## 五、与 Java JIT 的对比

| 维度 | Aura JIT | Java JIT（C1/C2） | 差距 |
|------|---------|-------------------|------|
| **优化深度** | Cranelift 基线 | C2 深度优化（逃逸分析、去虚拟化、O3 内联） | 大 |
| **编译去重** | ❌ 无（P8 待做） | ✅ 编译锁 + 去重 | 中 |
| **OSR** | ❌ 无（P8 待做） | ✅ 成熟 | 大 |
| **类型反馈** | ❌ 无（P9 待做） | ✅ 成熟（单态/双态/泛态） | 大 |
| **多级优化** | ❌ 单级 | ✅ C1（快速）→ C2（深度） | 大 |
| **去优化** | ❌ 回退解释器 | ✅ 回退 C1 编译 | 中 |
| **Profile-Guided** | ❌ 无（P10 待做） | ✅ 成熟 | 大 |
| **编译延迟** | ~1-5ms（快） | C1 ~5-20ms / C2 ~100-500ms | Aura 胜 |
| **并发模型** | 线程独立 VM | 共享 JVM | 架构差异（见 §六） |

---

## 六、并发模型与 JIT 的关系

Aura 采用「线程独立 VM + Actor/Channel 消息传递」的并发模型，与 Java 的「共享 JVM + 共享内存」根本不同：

| 维度 | Aura | Java | 影响 |
|------|------|------|------|
| **JIT 状态** | 每 VM 独立（无锁） | 共享（去重编译） | Aura 无锁但浪费 CPU |
| **锁竞争** | ❌ 零 | ✅ 有（JIT 编译锁、代码缓存） | Aura 扩展性更好 |
| **编译去重** | ❌ 无（待优化） | ✅ 有 | Java 更高效 |
| **扩展性** | 线性 | 亚线性 | Aura 高并发更优 |
| **尾延迟** | 低（无锁） | 高（锁竞争 + GC STW） | Aura 低延迟更优 |

**优化方向**：Aura 应借鉴 Java 的去重机制（全局编译缓存），同时保留无锁并发优势。这是「取两者之长」的路线。

---

## 七、并发层精确阻塞优化（消除轮询空转）

### 7.1 问题现状

当前 `Channel.recv` 和 `Actor.ask` 的「阻塞」语义是**假阻塞**——轮询空转，浪费 CPU。

#### 7.1.1 Channel.recv 空转（`vm/channel.rs:92-98`）

```rust
/// 在当前协作调度模型下，recv 立即返回：
/// 若缓冲区非空则取出并返回，否则返回 Null。
pub fn recv(&mut self, id: ChannelId) -> Value {
    if let Some(ch) = self.get_mut(id) {
        ch.buffer.pop_front().unwrap_or(Value::Null)  // 空时返回 Null，不阻塞
    } else {
        Value::Null
    }
}
```

**行为**：缓冲区为空时立即返回 `Null`，调用方（协程调度器）反复调用 `recv` 形成忙轮询（busy-spin），CPU 利用率 100% 但有效工作为 0。

#### 7.1.2 Channel.recv_timeout 轮询（`vm/channel.rs:111-145`）

```rust
pub fn recv_timeout(&mut self, id: ChannelId, timeout: Duration) -> Value {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(1);  // 1ms 轮询间隔

    loop {
        // 尝试立即接收
        if let Some(ch) = self.get_mut(id) {
            if let Some(val) = ch.buffer.pop_front() {
                return val;
            }
        }
        // 检查超时
        if start.elapsed() >= timeout {
            return Value::Null;
        }
        // 休眠后重试
        std::thread::sleep(poll_interval);
    }
}
```

**行为**：1ms 轮询间隔，最多空转 1000 次/秒。虽有 `sleep`，但精度差（延迟 0-1ms），且 `sleep` 不是真正的阻塞（OS 调度器可能延迟唤醒）。

#### 7.1.3 Actor.ask 伪阻塞（`vm/actor.rs:129-160`）

```rust
/// 向 Actor 请求响应（真阻塞等待，Phase 4）
pub fn ask(&mut self, id: ActorId, msg: Value) -> Option<Value> {
    // ... 发送消息 + 注册 PendingRequest
    // 通过协程调度器实现「挂起」，实际是轮询 response_queue
}
```

**行为**：`ask` 注册 `PendingRequest` 后，由协程调度器轮询 `response_queue` 检查响应。协程挂起不占 OS 线程，但仍需调度器轮询检查。

#### 7.1.4 select 多路复用轮询（`vm/native.rs:754-764`）

```rust
fn native_select(args: &[Value]) -> Value {
    // 检查所有通道，返回第一个有值的通道的值
    // 若所有通道均为空，返回 Null（非阻塞模式）
}
```

**行为**：检查所有通道，全空时返回 `Null`，调用方反复调用形成忙轮询。

### 7.2 根因分析

| 根因 | 说明 | 影响 |
|------|------|------|
| **无事件通知机制** | Channel/Actor 没有 `Condvar`/`EventFd`，发送方无法唤醒接收方 | 接收方必须轮询 |
| **无锁设计** | Channel 用裸 `VecDeque`（无 `Mutex`），无法安全通知 | 需要加锁才能通知 |
| **协作调度模型** | 单 VM 内是协程协作调度，阻塞会卡住整个 VM | 不能简单用 `Condvar.wait()` |
| **线程本地 VM** | 每线程独立 VM，跨线程通信需通过 TCP | 同进程多线程无法共享 Channel |
| **无优先级继承** | 无锁设计无法处理优先级反转 | 高优先级协程可能被低优先级阻塞 |

### 7.3 方案设计

#### 7.3.1 总体架构

```
┌─────────────────────────────────────────────────────────────────┐
│  Channel/Actor 精确阻塞架构                                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  发送方（producer）                                               │
│  ┌─────────────────────────────────────────┐                    │
│  │  channel.send(val)                       │                    │
│  │    1. 获取 channel 锁                     │                    │
│  │    2. 写入 buffer                        │                    │
│  │    3. notify_one() → 唤醒等待的接收方      │                    │
│  │    4. 释放锁                             │                    │
│  └─────────────────────────────────────────┘                    │
│                    ↓ 事件通知                                       │
│  接收方（consumer）                                               │
│  ┌─────────────────────────────────────────┐                    │
│  │  channel.recv()                          │                    │
│  │    1. 获取 channel 锁                     │                    │
│  │    2. buffer 非空 → pop 并返回            │                    │
│  │    3. buffer 空 → Condvar.wait() 阻塞     │                    │
│  │       （OS 线程挂起，零 CPU 消耗）         │                    │
│  │    4. 被唤醒后重新检查 buffer             │                    │
│  │    5. 释放锁                             │                    │
│  └─────────────────────────────────────────┘                    │
│                                                                   │
│  关键：Mutex + Condvar 组合实现精确阻塞                              │
│        - Mutex 保护 buffer 读写                                    │
│        - Condvar 实现阻塞/唤醒                                     │
│        - 等待条件：buffer 非空                                      │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

#### 7.3.2 核心数据结构改造

**当前**（`vm/channel.rs:20-27`）：

```rust
pub struct Channel {
    pub id: ChannelId,
    pub bound: usize,
    pub buffer: VecDeque<Value>,  // 无锁，单 VM 内使用
}
```

**改造后**：

```rust
pub struct Channel {
    pub id: ChannelId,
    pub bound: usize,
    /// 缓冲区 + 锁（保护并发读写）
    pub inner: Mutex<ChannelInner>,
}

struct ChannelInner {
    buffer: VecDeque<Value>,
    /// 接收方等待条件：buffer 非空时唤醒
    recv_not_empty: Condvar,
    /// 发送方等待条件：buffer 未满时唤醒（有界 Channel）
    send_not_full: Condvar,
}
```

**关键设计**：
- `Mutex` 保护 buffer 的并发读写（解决线程安全问题）
- `recv_not_empty`：接收方阻塞条件（buffer 非空）
- `send_not_full`：发送方阻塞条件（有界 Channel 缓冲区未满）
- 双 `Condvar` 支持生产者和消费者双向阻塞

#### 7.3.3 精确阻塞的 recv 实现

```rust
/// 从 Channel 接收值（精确阻塞，零 CPU 空转）
pub fn recv(&self, id: ChannelId) -> Value {
    let guard = self.get(id).unwrap().inner.lock().unwrap();

    // 等待条件：buffer 非空
    let result = guard.recv_not_empty.wait_while(guard, |inner| {
        inner.buffer.is_empty()  // 条件为 true 时继续等待
    }).unwrap();

    result.buffer.pop_front().unwrap_or(Value::Null)
}

/// 从 Channel 接收值（带超时）
pub fn recv_timeout(&self, id: ChannelId, timeout: Duration) -> Value {
    let guard = self.get(id).unwrap().inner.lock().unwrap();

    let result = guard.recv_not_empty.wait_timeout_while(guard, timeout, |inner| {
        inner.buffer.is_empty()
    }).unwrap();

    match result {
        Ok(guard) => guard.buffer.pop_front().unwrap_or(Value::Null),
        Err(_) => Value::Null,  // 超时
    }
}
```

**关键改进**：
- `wait_while`：条件为 true 时阻塞，为 false 时返回（精确阻塞）
- `wait_timeout_while`：带超时的精确阻塞（无轮询）
- 零 CPU 空转：阻塞期间 OS 线程挂起，不消耗 CPU

#### 7.3.4 精确阻塞的 send 实现（有界 Channel）

```rust
/// 向 Channel 发送值（有界 Channel 满时精确阻塞）
pub fn send(&self, id: ChannelId, val: Value) -> bool {
    let mut guard = self.get(id).unwrap().inner.lock().unwrap();

    // 等待条件：buffer 未满（有界 Channel）
    if guard.bound > 0 {
        guard.send_not_full.wait_while(guard, |inner| {
            inner.buffer.len() >= guard.bound  // 条件为 true 时继续等待
        }).unwrap();
    }

    guard.buffer.push_back(val);

    // 唤醒等待的接收方
    guard.recv_not_empty.notify_one();

    true
}
```

**关键改进**：
- 有界 Channel 满时精确阻塞（无空转）
- 发送后唤醒接收方（零延迟通知）
- 无界 Channel（`bound == 0`）不阻塞

#### 7.3.5 Actor 精确阻塞改造

**当前**（`vm/actor.rs:129-160`）：`ask` 注册 `PendingRequest`，协程调度器轮询 `response_queue`。

**改造后**：

```rust
pub struct ActorRuntime {
    actors: Vec<Option<Actor>>,
    next_id: usize,
    /// 待处理请求（带通知机制）
    pending_requests: HashMap<u64, PendingRequest>,
    /// 响应队列 + 通知条件
    response_notify: Condvar,
    response_queue: VecDeque<(u64, Value)>,
}

/// 向 Actor 请求响应（精确阻塞，零 CPU 空转）
pub fn ask(&mut self, id: ActorId, msg: Value) -> Value {
    // 1. 发送消息
    self.send(id, msg);

    // 2. 注册 PendingRequest
    let request_id = self.next_request_id;
    self.next_request_id += 1;
    self.pending_requests.insert(request_id, PendingRequest {
        request_id,
        from_actor: self.current_actor_id,
        target_actor: id,
        // ...
    });

    // 3. 精确阻塞等待响应（零 CPU 空转）
    loop {
        let mut guard = self.response_queue_guard.lock().unwrap();
        let response = guard.response_notify.wait_while(guard, |q| {
            // 条件：队列为空 或 响应不是给自己的
            q.is_empty() || q.front().map(|(rid, _)| *rid != request_id).unwrap_or(true)
        }).unwrap();

        if let Some((rid, val)) = guard.response_queue.pop_front() {
            if rid == request_id {
                return val;  // 找到响应
            } else {
                guard.response_queue.push_front((rid, val));  // 不是自己的，放回
                guard.response_notify.notify_one();  // 唤醒其他等待者
            }
        }
    }
}
```

**关键改进**：
- `ask` 精确阻塞等待（零 CPU 空转）
- 响应到达时唤醒对应等待者（精确通知）
- 多等待者时正确路由响应

#### 7.3.6 select 多路复用精确阻塞

**当前**（`vm/native.rs:754-764`）：检查所有通道，全空返回 Null（非阻塞）。

**改造后**：

```rust
/// select 多路复用（精确阻塞，零 CPU 空转）
pub fn select(&self, channel_ids: &[ChannelId], timeout: Option<Duration>) -> Option<(ChannelId, Value)> {
    // 1. 先尝试非阻塞检查
    for &id in channel_ids {
        if let Some(val) = self.try_recv(id) {
            return Some((id, val));
        }
    }

    // 2. 全部为空，精确阻塞等待（任一通道有消息即唤醒）
    // 需要注册 waiters 到每个 Channel 的 Condvar
    let mut waiters = Vec::new();
    for &id in channel_ids {
        let channel = self.get(id).unwrap();
        let mut guard = channel.inner.lock().unwrap();
        // 注册 waiter（等待条件：buffer 非空）
        guard.recv_not_empty.wait_while(guard, |inner| inner.buffer.is_empty()).unwrap();
        // 被唤醒后检查
        if let Some(val) = guard.buffer.pop_front() {
            return Some((id, val));
        }
    }

    // 3. 超时处理
    None
}
```

**简化实现**（推荐）：

```rust
/// select 多路复用（简化实现：轮询 + 退避）
pub fn select(&self, channel_ids: &[ChannelId], timeout: Option<Duration>) -> Option<(ChannelId, Value)> {
    let deadline = timeout.map(|t| Instant::now() + t);

    loop {
        // 1. 非阻塞检查所有通道
        for &id in channel_ids {
            if let Some(val) = self.try_recv(id) {
                return Some((id, val));
            }
        }

        // 2. 检查超时
        if let Some(deadline) = deadline {
            if Instant::now() >= deadline {
                return None;  // 超时
            }
        }

        // 3. 精确休眠（无空转）
        // 使用 nanosleep 而非 sleep，支持更精确的退避
        std::thread::sleep(Duration::from_micros(100));  // 100µs 退避
    }
}
```

**设计取舍**：
- 精确阻塞实现复杂（需要跨 Channel 的 Condvar 协调）
- 简化实现用退避轮询（100µs 间隔，CPU 空转 <0.01%）
- 推荐先用简化实现，后续优化为精确阻塞

### 7.4 实现阶段规划

#### 阶段 1：Channel Mutex + Condvar 改造（P8，2 周）

**目标**：Channel 支持精确阻塞，消除轮询空转。

**任务**：
- [ ] 改造 `Channel` 结构体，引入 `Mutex<ChannelInner>` + 双 `Condvar`
- [ ] 实现精确阻塞的 `recv` / `send` / `recv_timeout`
- [ ] 保持无界 Channel 不阻塞语义
- [ ] 保持有界 Channel 满时阻塞语义
- [ ] 添加单元测试（并发 send/recv、超时、有界阻塞）

**验证**：
- 并发 send/recv 无数据竞争（Rust 编译器保证）
- recv 空时阻塞（零 CPU 消耗，ps 验证）
- recv_timeout 超时精确（误差 <100µs）
- 有界 Channel 满时 send 阻塞

**预期收益**：
- CPU 空转从 100% 降为 0%（阻塞期间）
- 延迟精度从 1ms 提升为 <100µs（Condvar 唤醒延迟）
- 内存开销增加（Mutex + Condvar 约 56 字节/Channel）

#### 阶段 2：Actor 精确阻塞改造（P8，2 周）

**目标**：`Actor.ask` 支持精确阻塞，消除协程调度器轮询。

**任务**：
- [ ] 改造 `ActorRuntime`，引入 `Condvar` 通知机制
- [ ] 实现精确阻塞的 `ask`（等待响应时挂起）
- [ ] 实现精确唤醒（响应到达时唤醒对应等待者）
- [ ] 处理多等待者场景（响应路由）
- [ ] 添加单元测试（并发 ask、超时、多等待者）

**验证**：
- 并发 ask 无数据竞争
- ask 等待时阻塞（零 CPU 消耗）
- 响应到达时精确唤醒（延迟 <100µs）
- 多等待者时响应正确路由

**预期收益**：
- 协程调度器不再轮询 `response_queue`
- ask 等待时零 CPU 消耗
- 响应延迟从轮询间隔降为 <100µs

#### 阶段 3：select 多路复用优化（P9，1 周）

**目标**：`select` 支持精确阻塞或高效退避。

**任务**：
- [ ] 实现简化版 select（退避轮询，100µs 间隔）
- [ ] 评估精确阻塞实现的复杂度
- [ ] 如果复杂度可接受，实现精确阻塞版
- [ ] 添加单元测试（多通道、超时、混合阻塞）

**验证**：
- 多通道 select 正确返回第一个有值的通道
- 超时精确（误差 <200µs）
- CPU 空转 <0.01%（退避实现）或 0%（精确阻塞实现）

**预期收益**：
- select 延迟从 1ms 降为 <200µs
- CPU 空转从 100% 降为 <0.01%

#### 阶段 4：跨线程 Channel 共享（P9，2 周）

**目标**：同进程多线程可以共享 Channel（无需 TCP）。

**任务**：
- [ ] 设计跨线程 Channel 共享机制
- [ ] 实现 `Arc<Channel>` 支持多线程共享
- [ ] 处理线程安全问题（Mutex + Condvar 已覆盖）
- [ ] 添加集成测试（多线程 send/recv）

**验证**：
- 多线程共享 Channel 无数据竞争
- 跨线程 send/recv 正确唤醒
- 延迟 <200µs（Condvar 唤醒）

**预期收益**：
- 消除同进程多线程的 TCP 开销（~50µs/消息）
- 支持更灵活的并发架构

### 7.5 性能影响预估

| 指标 | 当前（轮询） | 改造后（精确阻塞） | 提升 |
|------|-------------|-------------------|------|
| **CPU 空转** | 100%（busy-spin） | 0%（阻塞） | ✅ 100% |
| **recv 延迟** | 1ms（轮询间隔） | <100µs（Condvar 唤醒） | ✅ 10x |
| **select 延迟** | 1ms（轮询间隔） | <200µs（退避/精确） | ✅ 5x |
| **ask 延迟** | 轮询间隔（协程调度） | <100µs（Condvar 唤醒） | ✅ 10x |
| **内存开销** | 0 | ~56 字节/Channel | ⚠️ 增加 |
| **锁竞争** | ❌ 无 | ✅ 有（Mutex） | ⚠️ 增加 |
| **死锁风险** | ❌ 无 | ✅ 有（锁顺序） | ⚠️ 增加 |

**权衡**：
- 精确阻塞用「锁竞争 + 内存开销」换取「零 CPU 空转 + 低延迟」
- 高并发场景下锁竞争可能成为瓶颈，需分片（sharded）优化
- 低并发场景下精确阻塞收益明显（零空转、低延迟）

### 7.6 与 Java 的对比

| 维度 | Aura（改造后） | Java | 差异 |
|------|---------------|------|------|
| **Channel 实现** | `Mutex` + `Condvar` + `VecDeque` | `LinkedBlockingQueue`（`ReentrantLock` + `Condition`） | 类似 |
| **阻塞机制** | `Condvar.wait/notify` | `Condition.await/signal` | 类似 |
| **超时精度** | <100µs | <100µs | 持平 |
| **CPU 空转** | 0%（精确阻塞） | 0%（精确阻塞） | 持平 |
| **内存开销** | ~56 字节/Channel | ~100 字节/Queue | Aura 更省 |
| **锁竞争** | 有（Mutex） | 有（ReentrantLock） | 类似 |
| **无锁并发** | ❌ 有锁（Mutex） | ❌ 有锁（ReentrantLock） | 类似 |

**关键差异**：
- Aura 改造后与 Java 的阻塞机制趋同（`Mutex` + `Condvar` vs `ReentrantLock` + `Condition`）
- Aura 保留 Actor 隔离优势（无共享内存），Java 保留共享内存优势
- Aura 的内存开销略低（VecDeque vs LinkedList）

### 7.7 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| **死锁** | 锁顺序错误导致死锁 | 统一锁顺序（先 Channel 锁，后 Actor 锁） |
| **优先级反转** | 低优先级线程持有锁，阻塞高优先级 | 使用优先级继承锁（`parking_lot::Mutex`） |
| **锁竞争** | 高并发下 Mutex 成为瓶颈 | 分片 Channel（sharded，多个子 Channel） |
| **唤醒风暴** | 一次通知唤醒多个等待者 | `notify_one` 而非 `notify_all` |
| **虚假唤醒** | `Condvar.wait` 可能虚假唤醒 | `wait_while` 重新检查条件 |
| **内存泄漏** | 未关闭的 Channel/Actor | `Drop` trait 自动清理 |

### 7.8 关键代码位置

| 文件 | 当前行 | 改造内容 |
|------|--------|---------|
| `compiler/src/vm/channel.rs` | 20-147 | Channel 结构体 + send/recv/recv_timeout |
| `compiler/src/vm/actor.rs` | 52-336 | ActorRuntime + ask |
| `compiler/src/vm/native.rs` | 711-764 | channelSend/channelRecv/select 绑定 |
| `compiler/src/vm/thread_pool.rs` | 17-146 | ThreadPool（参考实现） |

---

## 八、关键文件

| 文件 | 职责 |
|------|------|
| `compiler/src/vm/jit.rs` | Cranelift JIT 编译（白名单、编译、派发） |
| `compiler/src/vm/jit_opt.rs` | 7 个字节码优化 pass |
| `compiler/src/vm/jit_native.rs` | 原生函数调度器（C 兼容） |
| `compiler/src/vm/mod.rs` | 热点计数、Fix A/B、JIT 派发接缝 |
| `compiler/src/vm/abi.rs` | JitValue ABI（共享调用约定） |
| `compiler/src/vm/debugger.rs` | JIT 调试支持 |
| `compiler/src/vm/channel.rs` | Channel 实现（待改造：精确阻塞） |
| `compiler/src/vm/actor.rs` | Actor 实现（待改造：精确阻塞） |
| `compiler/src/vm/thread_pool.rs` | 线程池（参考实现：Mutex+Condvar） |
| `compiler/examples/jit_bench_simple.rs` | VM vs JIT 性能对比 |
| `compiler/examples/jit_diag.rs` | JIT 状态诊断 |

---

## 九、总结

**已完成**：Fix A（入口强制编译）+ Fix B（递归支持）+ 7 个优化 pass，JIT 性能从 1.0x 提升到 **78-109x**，部分场景超过 AOT。

**待完成（JIT 层）**：JIT 去重编译（P8）、OSR（P8）、逃逸分析（P8）、类型反馈（P9）、编译线程池（P9）、Profile-Guided（P10）。

**待完成（并发层）**：Channel 精确阻塞（P8，2 周）、Actor 精确阻塞（P8，2 周）、select 多路复用优化（P9，1 周）、跨线程 Channel 共享（P9，2 周）。精确阻塞改造消除轮询空转，CPU 从 100% 降为 0%，延迟从 1ms 降为 <100µs。

**核心差距**：优化深度（Cranelift vs C2）、编译去重（无 vs 有）、OSR（无 vs 有）、类型反馈（无 vs 有）、并发精确阻塞（轮询空转 vs Condvar 精确阻塞）。这些是 Aura JIT 追赶 Java JIT 的关键路径。

**架构优势**：线程独立 VM + 无锁并发，在高并发、低延迟场景下优于 Java 的共享 JVM 模型。优化方向是借鉴 Java 的编译去重、类型反馈和精确阻塞机制，同时保留无锁并发的架构优势。
