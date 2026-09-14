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
| **无事件通知机制** | Channel/Actor 没有事件通知（EventFd/kqueue/CPipe），发送方无法唤醒接收方 | 接收方必须轮询 |
| **协作调度模型** | 单 VM 内是协程协作调度，阻塞会卡住整个 VM | 不能用 `Condvar.wait()`（会阻塞 OS 线程） |
| **单 VM 单线程** | Channel 只在单 VM 内使用，无并发访问 | **不需要 Mutex**（单线程无竞争） |
| **线程本地 VM** | 每线程独立 VM，跨线程通信通过 TCP | 同进程多线程不共享 Channel（无锁） |

**关键纠正**：单 VM 内是单线程协程调度，**不需要 Mutex**——无锁设计是正确的。问题不是"缺少锁"，而是"缺少事件通知机制"。

### 7.3 方案设计：事件驱动（无锁 + 精确阻塞）

#### 7.3.1 核心原则

**不加锁，用事件通知**：

- 单 VM 内是单线程，不需要 Mutex
- 用 EventFd（Linux）/ kqueue（macOS）/ CPipe（Windows）实现事件通知
- 发送方写入事件，接收方 epoll/kqueue 等待（精确阻塞，零 CPU 空转）
- 保持 Aura 的无锁隔离优势，不退回到 Java 的"共享内存 + 锁"模型

#### 7.3.2 总体架构

```
┌─────────────────────────────────────────────────────────────────┐
│  Channel 事件驱动架构（无锁 + 精确阻塞）                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                   │
│  发送方（producer）                                               │
│  ┌─────────────────────────────────────────┐                    │
│  │  channel.send(val)                       │                    │
│  │    1. 写入 buffer（无锁，单线程）          │                    │
│  │    2. event_fd.write(1) → 通知事件        │                    │
│  │    （无锁，无 Condvar）                    │                    │
│  └─────────────────────────────────────────┘                    │
│                    ↓ 事件通知（EventFd/kqueue/CPipe）              │
│  接收方（consumer）                                               │
│  ┌─────────────────────────────────────────┐                    │
│  │  channel.recv()                          │                    │
│  │    1. buffer 非空 → pop 并返回            │                    │
│  │    2. buffer 空 → epoll_wait() 阻塞      │                    │
│  │       （OS 线程挂起，零 CPU 消耗）         │                    │
│  │    3. 被唤醒后 pop 并返回                 │                    │
│  │    （无锁，无 Mutex）                      │                    │
│  └─────────────────────────────────────────┘                    │
│                                                                   │
│  关键：EventFd + epoll 实现精确阻塞                                 │
│        - 无锁（单 VM 单线程）                                       │
│        - 零 CPU 空转（epoll 阻塞）                                  │
│        - 保持 Aura 无锁隔离优势                                    │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

#### 7.3.3 跨平台事件通知抽象

```rust
/// 事件通知抽象（跨平台）
pub trait EventNotifier: Send + Sync {
    /// 写入事件（发送方调用）
    fn notify(&self) -> io::Result<()>;
    /// 等待事件（接收方调用，精确阻塞）
    fn wait(&self, timeout: Option<Duration>) -> io::Result<bool>;
    /// 消耗已读事件（防止重复触发）
    fn drain(&self) -> io::Result<()>;
    /// 获取 epoll/kqueue fd（用于 select 多路复用）
    fn fd(&self) -> RawFd;
}

// Linux: EventFd 实现
#[cfg(target_os = "linux")]
pub struct EventFdNotifier {
    fd: EventFd,  // libc::eventfd
}

// macOS: kqueue 实现
#[cfg(target_os = "macos")]
pub struct KqueueNotifier {
    kq: RawFd,
    id: u64,
}

// Windows: CPipe 实现
#[cfg(target_os = "windows")]
pub struct CPipeNotifier {
    pipe: CPipe,
}
```

#### 7.3.4 Channel 数据结构改造

**当前**（`vm/channel.rs:20-27`）：

```rust
pub struct Channel {
    pub id: ChannelId,
    pub bound: usize,
    pub buffer: VecDeque<Value>,  // 无锁，单 VM 内使用
}
```

**改造后**（新增事件通知，不加锁）：

```rust
pub struct Channel {
    pub id: ChannelId,
    pub bound: usize,
    /// 缓冲区（无锁，单 VM 内单线程使用）
    pub buffer: VecDeque<Value>,
    /// 事件通知（发送方写入，接收方 epoll 等待）
    pub notifier: EventNotifier,
}
```

**关键设计**：
- `buffer` 保持无锁（单 VM 单线程，不需要 Mutex）
- `notifier` 新增：EventFd/kqueue/CPipe 实现事件通知
- 无 Mutex、无 Condvar——保持 Aura 无锁优势

#### 7.3.5 精确阻塞的 recv 实现

```rust
/// 从 Channel 接收值（精确阻塞，零 CPU 空转，无锁）
pub fn recv(&mut self, id: ChannelId) -> Value {
    let ch = self.get_mut(id).unwrap();

    // 1. 先尝试非阻塞 pop
    if let Some(val) = ch.buffer.pop_front() {
        return val;
    }

    // 2. buffer 空，等待事件（精确阻塞，零 CPU 空转）
    ch.notifier.wait(None).unwrap();

    // 3. 被唤醒后 pop
    ch.buffer.pop_front().unwrap_or(Value::Null)
}

/// 从 Channel 接收值（带超时）
pub fn recv_timeout(&mut self, id: ChannelId, timeout: Duration) -> Value {
    let ch = self.get_mut(id).unwrap();

    // 1. 先尝试非阻塞 pop
    if let Some(val) = ch.buffer.pop_front() {
        return val;
    }

    // 2. buffer 空，等待事件（带超时）
    match ch.notifier.wait(Some(timeout)).unwrap() {
        true => ch.buffer.pop_front().unwrap_or(Value::Null),  // 有事件
        false => Value::Null,  // 超时
    }
}
```

**关键改进**：
- `notifier.wait()`：精确阻塞（epoll/kqueue 系统调用），零 CPU 空转
- 无 Mutex、无 Condvar——保持无锁优势
- 超时由 `notifier.wait(timeout)` 精确处理（误差 <100µs）

#### 7.3.6 精确阻塞的 send 实现

```rust
/// 向 Channel 发送值（无锁，发送后通知事件）
pub fn send(&mut self, id: ChannelId, val: Value) -> bool {
    let ch = self.get_mut(id).unwrap();

    // 1. 有界 Channel 满时返回 false（非阻塞模式）
    if ch.bound > 0 && ch.buffer.len() >= ch.bound {
        return false;
    }

    // 2. 写入 buffer（无锁，单线程）
    ch.buffer.push_back(val);

    // 3. 通知事件（唤醒等待的接收方）
    ch.notifier.notify().unwrap();

    true
}
```

**关键改进**：
- 无锁写入（单 VM 单线程）
- 发送后通知事件（零延迟唤醒接收方）
- 有界 Channel 满时非阻塞返回（不卡死发送方）

#### 7.3.7 Actor 精确阻塞改造

**当前**（`vm/actor.rs:129-160`）：`ask` 注册 `PendingRequest`，协程调度器轮询 `response_queue`。

**改造后**（事件驱动，无锁）：

```rust
pub struct ActorRuntime {
    actors: Vec<Option<Actor>>,
    next_id: usize,
    /// 待处理请求
    pending_requests: HashMap<u64, PendingRequest>,
    /// 响应队列（无锁，单 VM 内）
    response_queue: VecDeque<(u64, Value)>,
    /// 事件通知（响应到达时通知）
    response_notifier: EventNotifier,
}

/// 向 Actor 请求响应（精确阻塞，零 CPU 空转，无锁）
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
    });

    // 3. 精确阻塞等待响应（零 CPU 空转，无锁）
    loop {
        // 3.1 先检查 response_queue（非阻塞）
        while let Some((rid, val)) = self.response_queue.front().cloned() {
            if rid == request_id {
                self.response_queue.pop_front();
                return val;  // 找到响应
            }
            self.response_queue.pop_front();  // 不是自己的，丢弃
        }

        // 3.2 队列为空，等待事件（精确阻塞）
        self.response_notifier.wait(None).unwrap();
    }
}

/// Actor 处理消息后写回响应（通知等待的 ask）
pub fn respond(&mut self, request_id: u64, val: Value) {
    self.response_queue.push_back((request_id, val));
    self.response_notifier.notify().unwrap();  // 唤醒等待的 ask
}
```

**关键改进**：
- `ask` 精确阻塞等待（零 CPU 空转，无锁）
- 响应到达时 `notify()` 唤醒等待者（零延迟）
- 无 Mutex、无 Condvar——保持无锁优势

#### 7.3.8 select 多路复用（epoll/kqueue 原生支持）

**当前**（`vm/native.rs:754-764`）：检查所有通道，全空返回 Null（非阻塞）。

**改造后**（epoll/kqueue 原生多路复用）：

```rust
/// select 多路复用（epoll/kqueue 精确阻塞，零 CPU 空转，无锁）
pub fn select(
    &mut self,
    channel_ids: &[ChannelId],
    timeout: Option<Duration>,
) -> Option<(ChannelId, Value)> {
    // 1. 先尝试非阻塞检查
    for &id in channel_ids {
        if let Some(val) = self.try_recv(id) {
            return Some((id, val));
        }
    }

    // 2. 全部为空，用 epoll/kqueue 等待任一通道事件
    let mut events = Vec::with_capacity(channel_ids.len());
    for &id in channel_ids {
        let ch = self.get_mut(id).unwrap();
        events.push(EventListener {
            fd: ch.notifier.fd(),
            channel_id: id,
            events: EPOLLIN,  // 等待可读事件
        });
    }

    // 3. epoll_wait（精确阻塞，零 CPU 空转）
    let mut epoll_evts = [EpollEvent::default(); channel_ids.len()];
    let n = epoll_wait(&events, &mut epoll_evts, timeout).unwrap();

    if n > 0 {
        // 4. 找到就绪的通道，pop 并返回
        for evt in &epoll_evts[..n] {
            let id = self.fd_to_channel_id(evt.fd);
            if let Some(val) = self.try_recv(id) {
                return Some((id, val));
            }
        }
    }

    None  // 超时或无就绪通道
}
```

**关键改进**：
- epoll/kqueue 原生支持多路复用（一次等待多个通道）
- 精确阻塞，零 CPU 空转
- 无 Mutex、无 Condvar——保持无锁优势
- 一次系统调用等待所有通道（高效）

### 7.4 实现阶段规划

#### 阶段 1：EventNotifier 跨平台抽象（P8，2 周）

**目标**：实现跨平台事件通知抽象（EventFd/kqueue/CPipe）。

**任务**：
- [ ] 设计 `EventNotifier` trait（notify / wait / drain / fd）
- [ ] Linux: EventFd 实现（`libc::eventfd`）
- [ ] macOS: kqueue 实现（`libc::kqueue`）
- [ ] Windows: CPipe 实现（`windows-sys`）
- [ ] 添加单元测试（notify/wait/timeout/drain）

**验证**：
- 三平台 notify + wait 正确唤醒
- wait 超时精确（误差 <100µs）
- wait 阻塞期间零 CPU 消耗（ps 验证）
- drain 消耗事件后不重复触发

**预期收益**：
- 跨平台事件通知基础设施就绪
- 无锁、零空转的事件驱动基础

#### 阶段 2：Channel 事件驱动改造（P8，2 周）

**目标**：Channel 支持精确阻塞，消除轮询空转，**不加锁**。

**任务**：
- [ ] 改造 `Channel` 结构体，新增 `notifier: EventNotifier`
- [ ] 实现精确阻塞的 `recv` / `recv_timeout`（epoll 等待）
- [ ] 实现 `send`（写入 buffer + notify）
- [ ] 保持无界 Channel 不阻塞语义
- [ ] 保持有界 Channel 满时非阻塞语义
- [ ] 添加单元测试（send/recv/timeout/有界满）

**验证**：
- recv 空时阻塞（零 CPU 消耗，ps 验证）
- recv_timeout 超时精确（误差 <100µs）
- send 后 recv 立即唤醒（延迟 <100µs）
- 有界 Channel 满时 send 返回 false（不阻塞）
- 无锁（单线程，无数据竞争）

**预期收益**：
- CPU 空转从 100% 降为 0%（阻塞期间）
- 延迟精度从 1ms 提升为 <100µs（epoll 唤醒延迟）
- 内存开销增加（EventFd 约 40 字节/Channel）
- **无锁竞争**（保持 Aura 无锁优势）

#### 阶段 3：Actor 事件驱动改造（P8，2 周）

**目标**：`Actor.ask` 支持精确阻塞，消除协程调度器轮询，**不加锁**。

**任务**：
- [ ] 改造 `ActorRuntime`，新增 `response_notifier: EventNotifier`
- [ ] 实现精确阻塞的 `ask`（epoll 等待响应）
- [ ] 实现 `respond`（写 response_queue + notify）
- [ ] 处理多请求场景（request_id 路由）
- [ ] 添加单元测试（ask/respond/超时/多请求）

**验证**：
- ask 等待时阻塞（零 CPU 消耗）
- 响应到达时精确唤醒（延迟 <100µs）
- 多请求时响应正确路由
- 无锁（单线程，无数据竞争）

**预期收益**：
- 协程调度器不再轮询 `response_queue`
- ask 等待时零 CPU 消耗
- 响应延迟从轮询间隔降为 <100µs

#### 阶段 4：select 多路复用优化（P9，1 周）

**目标**：`select` 支持 epoll/kqueue 精确阻塞，消除轮询空转。

**任务**：
- [ ] 实现 epoll/kqueue 版 select（多通道等待）
- [ ] 实现超时支持（`epoll_wait(timeout)`）
- [ ] 添加单元测试（多通道、超时、混合阻塞）

**验证**：
- 多通道 select 正确返回第一个有值的通道
- 超时精确（误差 <200µs）
- CPU 空转 0%（epoll 阻塞）
- 无锁（单线程）

**预期收益**：
- select 延迟从 1ms 降为 <200µs
- CPU 空转从 100% 降为 0%

#### 阶段 5：跨线程 Channel 共享（P9，可选，2 周）

**目标**：同进程多线程可以共享 Channel（无需 TCP）。

**任务**：
- [ ] 设计跨线程 Channel 共享机制
- [ ] 方案 A：无锁队列（lock-free queue，如 `crossbeam::queue::ArrayQueue`）
- [ ] 方案 B：保留 TCP（当前设计，无共享内存）
- [ ] 评估复杂度，如果可接受则实现方案 A

**验证**：
- 多线程共享 Channel 无数据竞争（Rust 编译器保证）
- 跨线程 send/recv 正确唤醒
- 延迟 <200µs

**预期收益**：
- 消除同进程多线程的 TCP 开销（~50µs/消息）
- 无锁（lock-free queue 或 TCP 流式 I/O）

### 7.5 性能影响预估

| 指标 | 当前（轮询） | 改造后（事件驱动） | 提升 |
|------|-------------|-------------------|------|
| **CPU 空转** | 100%（busy-spin） | 0%（epoll 阻塞） | ✅ 100% |
| **recv 延迟** | 1ms（轮询间隔） | <100µs（epoll 唤醒） | ✅ 10x |
| **select 延迟** | 1ms（轮询间隔） | <200µs（epoll 多路复用） | ✅ 5x |
| **ask 延迟** | 轮询间隔（协程调度） | <100µs（epoll 唤醒） | ✅ 10x |
| **内存开销** | 0 | ~40 字节/Channel（EventFd） | ⚠️ 增加 |
| **锁竞争** | ❌ 无 | ❌ 无（保持无锁） | ✅ 持平 |
| **死锁风险** | ❌ 无 | ❌ 无（无锁） | ✅ 持平 |

**权衡**：
- 事件驱动用「少量内存开销」换取「零 CPU 空转 + 低延迟」
- **无锁竞争**（保持 Aura 无锁优势，不退回到 Java 模型）
- **无死锁风险**（无锁，不可能死锁）
- 跨线程共享用无锁队列或 TCP，不引入 Mutex

### 7.6 与 Java 的对比

| 维度 | Aura（事件驱动） | Java | 差异 |
|------|-----------------|------|------|
| **并发模型** | Actor 隔离 + 消息传递 | 共享堆 + 锁 | ✅ Aura 无锁隔离 |
| **Channel 实现** | `VecDeque` + `EventFd`（无锁） | `LinkedBlockingQueue`（`ReentrantLock` + `Condition`） | ✅ Aura 无锁 |
| **阻塞机制** | `epoll`/`kqueue`（事件驱动） | `Condition.await/signal`（锁 + 条件变量） | ✅ Aura 无锁 |
| **超时精度** | <100µs（epoll） | <100µs（`Condition.awaitNanos`） | 持平 |
| **CPU 空转** | 0%（epoll 阻塞） | 0%（Condition 阻塞） | 持平 |
| **内存开销** | ~40 字节/Channel（EventFd） | ~100 字节/Queue（Lock + Condition） | ✅ Aura 更省 |
| **锁竞争** | ❌ 无（无锁） | ✅ 有（ReentrantLock） | ✅ Aura 无竞争 |
| **死锁风险** | ❌ 无（无锁） | ✅ 有（锁顺序） | ✅ Aura 无死锁 |
| **无锁并发** | ✅ 无锁（事件驱动） | ❌ 有锁（ReentrantLock） | ✅ Aura 优势 |

**关键差异**：
- Aura 保持无锁隔离优势（`VecDeque` + `EventFd`，无 Mutex、无 Condvar）
- Java 用锁保护共享内存（`ReentrantLock` + `Condition`）
- Aura 的阻塞是"事件驱动"（epoll/kqueue），Java 是"锁 + 条件变量"
- Aura 无死锁风险（无锁），Java 有死锁风险（锁顺序）
- Aura 内存开销更低（EventFd ~40 字节 vs Lock+Condition ~100 字节）

**核心区别**：
- Java 需要锁是因为"多线程共享堆"——锁保护共享可变状态
- Aura 不需要锁是因为"VM 隔离，不共享可变状态"——通信通过消息传递 + 事件通知
- 事件驱动是 Aura 的解法，锁 + 条件变量是 Java 的解法——两者本质不同

### 7.7 风险与缓解

| 风险 | 影响 | 缓解 |
|------|------|------|
| **虚假唤醒** | `epoll_wait` 可能虚假唤醒 | 唤醒后检查 buffer（`pop_front` 为空则继续等待） |
| **事件丢失** | 发送方 notify 但接收方未及时 drain | 每次 notify 写 1，接收方 `epoll_wait` 后 `drain` |
| **跨平台差异** | EventFd/kqueue/CPipe 行为不同 | 抽象 `EventNotifier` trait，平台特定实现 |
| **epoll 容量** | `epoll_wait` 一次最多 1024 事件 | select 限制通道数 <1024 |
| **内存泄漏** | 未关闭的 Channel/Actor | `Drop` trait 自动清理 EventFd |
| **超时精度** | epoll 超时可能有调度延迟 | 容忍 <200µs 误差 |

**关键风险消除**：
- **死锁**：无锁，不可能死锁（Aura 优势）
- **优先级反转**：无锁，不存在优先级反转（Aura 优势）
- **锁竞争**：无锁，无竞争（Aura 优势）

### 7.8 关键代码位置

| 文件 | 当前行 | 改造内容 |
|------|--------|---------|
| `compiler/src/vm/channel.rs` | 20-147 | Channel 结构体 + send/recv/recv_timeout（新增 EventNotifier） |
| `compiler/src/vm/actor.rs` | 52-336 | ActorRuntime + ask（新增 response_notifier） |
| `compiler/src/vm/native.rs` | 711-764 | channelSend/channelRecv/select 绑定 |
| `compiler/src/vm/event_notifier.rs` | 新增 | EventNotifier trait + 跨平台实现 |
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

**待完成（并发层）**：EventNotifier 跨平台抽象（P8，2 周）、Channel 事件驱动改造（P8，2 周）、Actor 事件驱动改造（P8，2 周）、select 多路复用优化（P9，1 周）、跨线程 Channel 共享（P9，可选，2 周）。事件驱动改造消除轮询空转，CPU 从 100% 降为 0%，延迟从 1ms 降为 <100µs，**保持无锁优势**（不加 Mutex、不加 Condvar）。

**核心差距**：优化深度（Cranelift vs C2）、编译去重（无 vs 有）、OSR（无 vs 有）、类型反馈（无 vs 有）、并发精确阻塞（轮询空转 vs epoll 事件驱动）。这些是 Aura JIT 追赶 Java JIT 的关键路径。

**架构优势**：线程独立 VM + 无锁并发，在高并发、低延迟场景下优于 Java 的共享 JVM 模型。优化方向是借鉴 Java 的编译去重和类型反馈，同时用**事件驱动**（EventFd + epoll）而非锁 + 条件变量实现精确阻塞，保留 Aura 的无锁隔离优势。
