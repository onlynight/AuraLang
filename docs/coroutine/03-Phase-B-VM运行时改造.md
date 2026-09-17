# Phase B: VM 运行时改造

## 目标

改造 VM 运行时，支持真正的多线程并发：线程安全的 Heap、新指令集、M:N 协程调度、事件驱动 Channel、Actor 调度循环。

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `compiler/src/codegen/opcode.rs` | 修改 | 新增并发指令 |
| `compiler/src/vm/mod.rs` | 修改 | 并发字段、VM 上下文 |
| `compiler/src/vm/interp.rs` | 修改 | 解释器实现并发指令 |
| `compiler/src/vm/heap.rs` | 修改 | Heap 加 Mutex 保护 |
| `compiler/src/vm/coroutine.rs` | 修改 | M:N 调度器 |
| `compiler/src/vm/channel.rs` | 修改 | 事件驱动 Channel |
| `compiler/src/vm/actor.rs` | 修改 | Actor 调度循环 |
| `compiler/src/vm/native.rs` | 修改 | 新增并发原生函数 |

## 设计

### 1. 新指令集

在 `OpCode` 中新增（编号 100-137）：

```rust
// ── 线程（100-109） ──
ThreadSpawn(u16),     // 创建线程执行函数 idx，返回线程 ID
ThreadJoin,           // 等待栈顶线程 ID 结束，返回结果
ThreadSleep,          // 栈顶为毫秒数，休眠
ThreadPark,           // 当前线程停车
ThreadUnpark,         // 栈顶为线程 ID，唤醒

// ── 同步原语（110-119） ──
MutexNew,             // 创建 Mutex
MutexLock,            // 锁（阻塞）
MutexUnlock,          // 解锁
MutexTryLock,         // 尝试锁（返回 bool）
RwLockNew,
RwLockReadLock,
RwLockWriteLock,
RwLockUnlock,

// ── 原子操作（120-129） ──
AtomicNew,            // 创建 Atomic<T>
AtomicLoad,
AtomicStore,
AtomicAdd,
AtomicSub,
AtomicCAS,

// ── 条件变量（130-134） ──
CondvarNew,
CondvarWait,
CondvarSignal,
CondvarBroadcast,
CondvarWaitTimeout,

// ── 屏障（135-137） ──
BarrierNew,
BarrierWait,
BarrierReset,
```

### 2. Heap 线程安全

```rust
// 方案 A：全局锁（Phase B 采用）
pub struct Heap {
    data: Mutex<HeapData>,
}

struct HeapData {
    objects: HashMap<ObjId, Obj>,
    lists: HashMap<ObjId, Vec<Value>>,
    maps: HashMap<ObjId, BTreeMap<Value, Value>>,
    arrays: HashMap<ObjId, Vec<Value>>,
    next_id: u64,
}
```

### 3. M:N 协程调度器

```rust
/// 每个 OS 线程运行自己的协程调度器
/// 跨线程 spawn 通过通道传递协程
pub struct CoroutineScheduler {
    coroutines: Vec<Option<Coroutine>>,
    ready_queue: VecDeque<usize>,
    next_id: usize,
    // 跨线程通道
    cross_thread_inbox: Arc<Mutex<VecDeque<usize>>>,  // 从其他线程接收的协程 ID
}

impl CoroutineScheduler {
    /// 创建新协程，可选择目标线程
    pub fn spawn(&mut self, entry_func: usize) -> usize { ... }
    
    /// 跨线程 spawn：将协程发送到指定线程的调度器
    pub fn spawn_cross_thread(
        &mut self, 
        target_thread: ThreadHandle,
        entry_func: usize
    ) -> usize { ... }
}
```

### 4. 事件驱动 Channel

```rust
use std::sync::{Arc, Condvar, Mutex};

pub struct Channel {
    id: ChannelId,
    bound: usize,
    buffer: Mutex<VecDeque<Value>>,
    // 事件通知
    not_empty: Condvar,    // 唤醒等待 recv 的线程
    not_full: Condvar,     // 唤醒等待 send 的线程
    closed: Arc<AtomicBool>,
}

impl Channel {
    /// 阻塞发送：缓冲区满时等待
    pub fn send(&self, val: Value) -> Result<(), ChannelError> {
        let mut buf = self.buffer.lock().unwrap();
        while buf.len() >= self.bound && !self.closed.load(Ordering::SeqCst) {
            self.not_full.wait(buf).unwrap();
        }
        buf.push_back(val);
        self.not_empty.notify_one();
        Ok(())
    }
    
    /// 阻塞接收：缓冲区空时等待
    pub fn recv(&self) -> Option<Value> {
        let mut buf = self.buffer.lock().unwrap();
        while buf.is_empty() && !self.closed.load(Ordering::SeqCst) {
            self.not_empty.wait(buf).unwrap();
        }
        let val = buf.pop_front();
        self.not_full.notify_one();
        val
    }
}
```

### 5. Actor 调度循环

```rust
pub struct ActorRuntime {
    actors: Mutex<HashMap<ActorId, Arc<Actor>>>,
    scheduler: Arc<Scheduler>,
}

impl ActorRuntime {
    /// 启动 Actor 调度循环（在独立线程中运行）
    pub fn start_actor_loop(&self, actor_id: ActorId, handler: ActorHandler) {
        let actors = self.actors.clone();
        thread::spawn(move || {
            loop {
                let msg = actors.lock().unwrap()
                    .get(&actor_id)
                    .expect("actor not found")
                    .mailbox
                    .lock()
                    .unwrap()
                    .recv();
                
                match msg {
                    Ok(msg) => handler.handle(msg),
                    Err(_) => break,  // 邮箱关闭
                }
            }
        });
    }
}
```

## 测试计划

### Rust 单元测试

```rust
// compiler/tests/vm_multithread_tests.rs
#[test]
fn test_thread_spawn_join() { ... }
#[test]
fn test_mutex_lock_unlock() { ... }
#[test]
fn test_atomic_operations() { ... }
#[test]
fn test_channel_event_driven() { ... }
#[test]
fn test_actor_scheduler() { ... }
#[test]
fn test_concurrent_stress() { ... }
```

### VM 集成测试

```rust
#[test]
fn test_spawn_thread_in_aura() { ... }
#[test]
fn test_mutex_in_aura() { ... }
#[test]
fn test_channel_in_aura() { ... }
#[test]
fn test_actor_message_delivery() { ... }
```

## 依赖

- Phase A: C 运行时原语
