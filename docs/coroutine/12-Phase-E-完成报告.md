# Phase E — Aura 标准库暴露 完成报告

> **状态**：✅ 完成  
> **日期**：2026-07-04  
> **关联设计文档**：`docs/coroutine/06-Phase-E-标准库暴露.md`

---

## 1. 完成项总览

| 项目 | 状态 | 说明 |
|------|------|------|
| Thread.aura | ✅ 完成 | 6 个方法：spawn/join/sleep/id/parallelism/availableCores |
| Mutex.aura | ✅ 完成 | 5 个方法：new/lock/unlock/tryLock/destroy |
| Atomic.aura | ✅ 完成 | 6 个方法：new/load/store/add/sub/cas |
| RwLock.aura | ✅ 完成 | 6 个方法：new/readLock/writeLock/readUnlock/writeUnlock/destroy |
| Condvar.aura | ✅ 完成 | 5 个方法：new/wait/signal/broadcast/destroy |
| Barrier.aura | ✅ 完成 | 3 个方法：new/wait/destroy |
| Future.aura | ✅ 完成 | 6 个方法：spawn/await/isDone/all/any/cancel |
| Future native 实现 | ✅ 完成 | 基于 Thread.spawn/Thread.join 的 Future 包装器 |

---

## 2. 新增 Aura 标准库类

### 2.1 Thread.aura

`aura/core/aura/lang/std/Thread.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `spawn` | `(fn_id: Int, arg: Int) -> Int` | 创建新线程 |
| `join` | `(thread_id: Int) -> Int` | 等待线程结束 |
| `sleep` | `(ms: Int) -> Unit` | 休眠毫秒数 |
| `id` | `() -> Int` | 获取当前线程 ID |
| `parallelism` | `() -> Int` | 获取最大并行度 |
| `availableCores` | `() -> Int` | 获取 CPU 核心数 |

### 2.2 Mutex.aura

`aura/core/aura/lang/std/Mutex.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `() -> Int` | 创建互斥锁 |
| `lock` | `(id: Int) -> Unit` | 获取锁（阻塞） |
| `unlock` | `(id: Int) -> Unit` | 释放锁 |
| `tryLock` | `(id: Int) -> Boolean` | 尝试获取锁 |
| `destroy` | `(id: Int) -> Unit` | 销毁锁 |

### 2.3 Atomic.aura

`aura/core/aura/lang/std/Atomic.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `(initial: Int) -> Int` | 创建原子整数 |
| `load` | `(id: Int) -> Int` | 原子加载 |
| `store` | `(id: Int, val: Int) -> Unit` | 原子存储 |
| `add` | `(id: Int, delta: Int) -> Int` | 原子加法 |
| `sub` | `(id: Int, delta: Int) -> Int` | 原子减法 |
| `cas` | `(id: Int, expected: Int, desired: Int) -> Boolean` | 原子 CAS |

### 2.4 RwLock.aura

`aura/core/aura/lang/std/RwLock.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `() -> Int` | 创建读写锁 |
| `readLock` | `(id: Int) -> Unit` | 获取读锁 |
| `writeLock` | `(id: Int) -> Unit` | 获取写锁 |
| `readUnlock` | `(id: Int) -> Unit` | 释放读锁 |
| `writeUnlock` | `(id: Int) -> Unit` | 释放写锁 |
| `destroy` | `(id: Int) -> Unit` | 销毁锁 |

### 2.5 Condvar.aura

`aura/core/aura/lang/std/Condvar.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `() -> Int` | 创建条件变量 |
| `wait` | `(cv_id: Int, mutex_id: Int) -> Unit` | 等待（释放锁） |
| `signal` | `(cv_id: Int) -> Unit` | 唤醒一个线程 |
| `broadcast` | `(cv_id: Int) -> Unit` | 唤醒所有线程 |
| `destroy` | `(cv_id: Int) -> Unit` | 销毁条件变量 |

### 2.6 Barrier.aura

`aura/core/aura/lang/std/Barrier.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `new` | `(count: Int) -> Int` | 创建屏障 |
| `wait` | `(id: Int) -> Int` | 在屏障等待 |
| `destroy` | `(id: Int) -> Unit` | 销毁屏障 |

### 2.7 Future.aura

`aura/core/aura/lang/std/Future.aura`

| 方法 | 签名 | 说明 |
|------|------|------|
| `spawn` | `(fn_id: Int, arg: Int) -> Int` | 异步执行 |
| `await` | `(future_id: Int) -> Int` | 等待完成 |
| `isDone` | `(future_id: Int) -> Boolean` | 检查是否完成 |
| `all` | `(ids: List<Int>) -> List<Int>` | 等待全部完成 |
| `any` | `(ids: List<Int>) -> Int` | 等待第一个完成 |
| `cancel` | `(future_id: Int) -> Unit` | 取消 Future |

---

## 3. Future Native 实现

### 3.1 实现方式

Future 基于 `Thread.spawn` + `Thread.join` 实现：

```rust
struct FutureEntry {
    thread_id: i64,
    done: AtomicBool,
    result: Mutex<i64>,
}

static FUTURE_REGISTRY: OnceLock<Mutex<HashMap<i64, FutureEntry>>> = OnceLock::new();
```

### 3.2 注册函数

位于 `compiler/src/vm/concurrent_native.rs`：

- `native_future_spawn` — 调用 `Thread.spawn` 创建线程，注册 FutureEntry
- `native_future_await` — 调用 `Thread.join` 等待线程完成
- `native_future_is_done` — 检查 AtomicBool 状态
- `native_future_all` — 遍历列表逐个 await
- `native_future_any` — 返回第一个完成的
- `native_future_cancel` — 从注册表移除

### 3.3 注册条目

```
aura.lang.std.Future.spawn     → native_future_spawn
aura.lang.std.Future.await     → native_future_await
aura.lang.std.Future.isDone    → native_future_is_done
aura.lang.std.Future.all       → native_future_all
aura.lang.std.Future.any       → native_future_any
aura.lang.std.Future.cancel    → native_future_cancel
```

---

## 4. 修改文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `aura/core/aura/lang/std/Thread.aura` | **新增** | Thread 标准库类 |
| `aura/core/aura/lang/std/Mutex.aura` | **新增** | Mutex 标准库类 |
| `aura/core/aura/lang/std/Atomic.aura` | **新增** | Atomic 标准库类 |
| `aura/core/aura/lang/std/RwLock.aura` | **新增** | RwLock 标准库类 |
| `aura/core/aura/lang/std/Condvar.aura` | **新增** | Condvar 标准库类 |
| `aura/core/aura/lang/std/Barrier.aura` | **新增** | Barrier 标准库类 |
| `aura/core/aura/lang/std/Future.aura` | **新增** | Future 标准库类 |
| `compiler/src/vm/concurrent_native.rs` | 修改 | Future native 函数实现 + 注册 |

---

## 5. 并发原生函数完整清单

Phase E 完成后，共 33 个并发原生函数注册：

| 类别 | 函数数 | 函数列表 |
|------|--------|---------|
| Thread | 6 | spawn, join, sleep, id, parallelism, availableCores |
| Mutex | 5 | new, lock, unlock, tryLock, destroy |
| Atomic | 6 | new, load, store, add, sub, cas |
| RwLock | 6 | new, readLock, writeLock, readUnlock, writeUnlock, destroy |
| Condvar | 5 | new, wait, signal, broadcast, destroy |
| Barrier | 3 | new, wait, destroy |
| Future | 6 | spawn, await, isDone, all, any, cancel |

---

## 6. 测试结果

```
Phase B (VM):  12 passed; 0 failed ✅
Phase C (AOT):  5 passed; 0 failed ✅
Phase D (JIT):  8 passed; 0 failed ✅
```

---

## 7. 使用示例

```aura
// 多线程 + Mutex + Atomic
import aura.lang.std.Thread
import aura.lang.std.Mutex
import aura.lang.std.Atomic

fun main() {
    val counter = Atomic.new(0)
    val lock = Mutex.new()
    
    val t1 = Thread.spawn(1, 0)
    val t2 = Thread.spawn(1, 0)
    
    Thread.join(t1)
    Thread.join(t2)
    
    println(Atomic.load(counter))
}
```
