# Phase E: 标准库暴露

## 目标

在 Aura 标准库中暴露多线程和同步原语，改造 Coroutine.aura 和 Actor.aura，使用户可以直接使用并发 API。

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `aura/core/aura/lang/coroutine/Thread.aura` | 新增 | Thread 类 |
| `aura/core/aura/lang/coroutine/Mutex.aura` | 新增 | Mutex 类 |
| `aura/core/aura/lang/coroutine/RwLock.aura` | 新增 | RwLock 类 |
| `aura/core/aura/lang/coroutine/Atomic.aura` | 新增 | Atomic 类 |
| `aura/core/aura/lang/coroutine/Future.aura` | 新增 | Future/Promise 抽象 |
| `aura/core/aura/lang/coroutine/Coroutine.aura` | 改造 | 使用 VM 原生函数 |
| `aura/core/aura/lang/coroutine/Actor.aura` | 改造 | 使用 VM 原生函数 |
| `aura/core/aura/lang/coroutine/Channel.aura` | 新增 | Channel 类 |
| `aura/core/aura/lang/coroutine/Condvar.aura` | 新增 | 条件变量 |

## 设计

### 1. Thread 类

```aura
package aura.lang.concurrent

/// 线程句柄
internal class ThreadHandle {
    private val id: Int
    
    constructor(id: Int) {
        this.id = id
    }
    
    /// 等待线程结束，返回线程结果
    fun join(): Any {
        return aura.lang.std.Thread.join(this.id)
    }
    
    /// 获取线程 ID
    fun getId(): Int {
        return this.id
    }
}

/// 线程管理
internal object Thread {
    /// 创建并启动新线程
    fun spawn(body: () -> Any): ThreadHandle {
        val id = aura.lang.std.Thread.spawn(body)
        return ThreadHandle(id)
    }
    
    /// 休眠指定毫秒
    fun sleep(ms: Int): Unit {
        aura.lang.std.Thread.sleep(ms)
    }
    
    /// 获取当前线程 ID
    fun currentId(): Int {
        return aura.lang.std.Thread.currentId()
    }
    
    /// 获取可用并行度（CPU 核心数）
    fun availableParallelism(): Int {
        return aura.lang.std.Thread.availableParallelism()
    }
}
```

### 2. Mutex 类

```aura
package aura.lang.concurrent

/// 互斥锁
internal class Mutex {
    private val handle: Int
    
    constructor(handle: Int) {
        this.handle = handle
    }
    
    fun lock(): Unit {
        aura.lang.std.Mutex.lock(this.handle)
    }
    
    fun unlock(): Unit {
        aura.lang.std.Mutex.unlock(this.handle)
    }
    
    fun tryLock(): Boolean {
        return aura.lang.std.Mutex.tryLock(this.handle)
    }
}

internal object MutexFactory {
    fun create(): Mutex {
        val handle = aura.lang.std.Mutex.new()
        return Mutex(handle)
    }
}
```

### 3. Atomic 类

```aura
package aura.lang.concurrent

/// 原子整数
internal class AtomicInteger {
    private val handle: Int
    
    constructor(initial: Int) {
        this.handle = aura.lang.std.Atomic.ofInt(initial)
    }
    
    fun get(): Int {
        return aura.lang.std.Atomic.load(this.handle)
    }
    
    fun set(value: Int): Unit {
        aura.lang.std.Atomic.store(this.handle, value)
    }
    
    fun incrementAndGet(): Int {
        return aura.lang.std.Atomic.addAndGet(this.handle, 1)
    }
    
    fun getAndIncrement(): Int {
        return aura.lang.std.Atomic.getAndAdd(this.handle, 1)
    }
    
    fun decrementAndGet(): Int {
        return aura.lang.std.Atomic.addAndGet(this.handle, -1)
    }
    
    fun getAndDecrement(): Int {
        return aura.lang.std.Atomic.getAndAdd(this.handle, -1)
    }
    
    fun compareAndSet(expected: Int, desired: Int): Boolean {
        return aura.lang.std.Atomic.cas(this.handle, expected, desired)
    }
}
```

### 4. Future/Promise 抽象

```aura
package aura.lang.concurrent

/// Future：异步计算的抽象
internal class Future<T> {
    private val handle: Int
    
    internal constructor(handle: Int) {
        this.handle = handle
    }
    
    /// 阻塞等待结果
    fun await(): T {
        return aura.lang.std.Future.await(this.handle)
    }
    
    /// 带超时的等待
    fun awaitTimeout(ms: Int): T {
        return aura.lang.std.Future.awaitTimeout(this.handle, ms)
    }
    
    /// 检查是否完成
    fun isDone(): Boolean {
        return aura.lang.std.Future.isDone(this.handle)
    }
}

/// Promise：手动 resolve 的 Future
internal class Promise<T> {
    private val handle: Int
    
    internal constructor(handle: Int) {
        this.handle = handle
    }
    
    fun resolve(value: T): Unit {
        aura.lang.std.Promise.resolve(this.handle, value)
    }
    
    fun fail(error: Any): Unit {
        aura.lang.std.Promise.fail(this.handle, error)
    }
    
    fun future(): Future<T> {
        return Future(aura.lang.std.Promise.getFuture(this.handle))
    }
}
```

### 5. 改造 Coroutine.aura

```aura
// 改造前：纯模拟，body 不执行
// 改造后：使用 VM 原生函数

internal object Coroutine {
    /// 启动新协程（使用 VM 调度器）
    fun spawn(body: () -> Any): Int {
        return aura.lang.std.Coroutine.spawn(body)
    }
    
    /// 阻塞等待协程结果
    fun ask(co_id: Int): Any {
        return aura.lang.std.Coroutine.ask(co_id)
    }
    
    /// 检查协程状态
    fun state(co_id: Int): String {
        return aura.lang.std.Coroutine.getState(co_id)
    }
}
```

### 6. 改造 Actor.aura

```aura
internal object Actor {
    /// 创建新 Actor（VM 调度器管理）
    fun spawnActor(name: String): Int {
        return aura.lang.std.Actor.spawnActor(name)
    }
    
    /// 发送消息
    fun send(actor: Int, message: Any): Unit {
        aura.lang.std.Actor.send(actor, message)
    }
    
    /// 请求响应（阻塞等待）
    fun ask(actor: Int, message: Any): Any {
        return aura.lang.std.Actor.ask(actor, message)
    }
    
    /// 设置死亡策略
    fun setDeathStrategy(actor: Int, strategy: String): Unit {
        aura.lang.std.Actor.setDeathStrategy(actor, strategy)
    }
}
```

### 7. 原生函数注册

在 `native.rs` 中新增原生函数：

```rust
// aura.lang.std.Thread.*
r.register("aura.lang.std.Thread.spawn", native_thread_spawn);
r.register("aura.lang.std.Thread.join", native_thread_join);
r.register("aura.lang.std.Thread.sleep", native_thread_sleep);
r.register("aura.lang.std.Thread.currentId", native_thread_current_id);
r.register("aura.lang.std.Thread.availableParallelism", native_thread_available_parallelism);

// aura.lang.std.Mutex.*
r.register("aura.lang.std.Mutex.new", native_mutex_new);
r.register("aura.lang.std.Mutex.lock", native_mutex_lock);
r.register("aura.lang.std.Mutex.unlock", native_mutex_unlock);
r.register("aura.lang.std.Mutex.tryLock", native_mutex_trylock);

// aura.lang.std.Atomic.*
r.register("aura.lang.std.Atomic.ofInt", native_atomic_of_int);
r.register("aura.lang.std.Atomic.load", native_atomic_load);
r.register("aura.lang.std.Atomic.store", native_atomic_store);
r.register("aura.lang.std.Atomic.addAndGet", native_atomic_add_and_get);
r.register("aura.lang.std.Atomic.cas", native_atomic_cas);

// aura.lang.std.Future.*
r.register("aura.lang.std.Future.await", native_future_await);
r.register("aura.lang.std.Future.isDone", native_future_is_done);
```

## 测试计划

### Aura 集成测试

```rust
// compiler/tests/concurrent_stdlib_tests.rs
#[test]
fn test_thread_spawn_join_aura() { ... }
#[test]
fn test_mutex_aura() { ... }
#[test]
fn test_atomic_aura() { ... }
#[test]
fn test_future_promise_aura() { ... }
#[test]
fn test_coroutine_refactored() { ... }
#[test]
fn test_actor_refactored() { ... }
```

### 示例

```
examples/concurrent/thread_demo.aura
examples/concurrent/mutex_demo.aura
examples/concurrent/atomic_counter.aura
examples/concurrent/actor_system.aura
```

## 依赖

- Phase A: C 运行时原语
- Phase B: VM 原生函数
- Phase C: AOT 代码生成
