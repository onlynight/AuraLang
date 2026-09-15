# Phase B — VM 运行时改造 完成报告

> **状态**：✅ 完成  
> **日期**：2026-07-04  
> **关联设计文档**：`docs/coroutine/03-Phase-B-VM运行时改造.md`

---

## 1. 完成项总览

| 项目 | 状态 | 说明 |
|------|------|------|
| 新指令集（OpCode） | ✅ 完成 | 26 条并发指令（0x54-0x6E），含 byte/operand_size/from_byte/write/Display 五件套 |
| VM 指令解码（Instr） | ✅ 完成 | `decode_function` 处理全部 26 条新 OpCode |
| VM 解释器分发 | ✅ 完成 | `exec_instr` 新增 26 条并发指令处理分支 |
| 调试器支持 | ✅ 完成 | `format_instr` 支持全部新指令的字符串化 |
| 并发原生函数（Rust 实现） | ✅ 完成 | Thread/Mutex/Atomic/RwLock/Condvar/Barrier 全部 30+ 原生函数 |
| NativeRegistry 集成 | ✅ 完成 | `NativeRegistry::new()` 和 `with_modules()` 自动注册 |
| 并发 VM 测试 | ✅ 完成 | 12 个测试全部通过 |

---

## 2. 新指令集（OpCode）

### 2.1 指令分配表

| 字节 | OpCode | 操作数 | 说明 |
|------|--------|--------|------|
| 0x54 | ThreadSpawn(u16) | 2B func_idx | 创建线程 |
| 0x55 | ThreadJoin | 0B | 等待线程结束 |
| 0x56 | ThreadSleep | 0B | 线程休眠（ms） |
| 0x57 | ThreadId | 0B | 获取线程 ID |
| 0x58 | ThreadParallelism | 0B | 获取并行度 |
| 0x59 | MutexNew | 0B | 创建 Mutex |
| 0x5A | MutexLock | 0B | 加锁 |
| 0x5B | MutexUnlock | 0B | 解锁 |
| 0x5C | MutexTryLock | 0B | 尝试加锁 |
| 0x5D | AtomicNew | 0B | 创建 Atomic |
| 0x5E | AtomicLoad | 0B | 原子读取 |
| 0x5F | AtomicStore | 0B | 原子写入 |
| 0x60 | AtomicAdd | 0B | 原子加法 |
| 0x61 | AtomicCas | 0B | 原子 CAS |
| 0x62 | RwLockNew | 0B | 创建 RwLock |
| 0x63 | RwLockReadLock | 0B | 获取读锁 |
| 0x64 | RwLockWriteLock | 0B | 获取写锁 |
| 0x65 | RwLockReadUnlock | 0B | 释放读锁 |
| 0x66 | RwLockWriteUnlock | 0B | 释放写锁 |
| 0x67 | ChannelNew | 0B | 创建 Channel |
| 0x68 | ChannelSend | 0B | 发送消息 |
| 0x69 | ChannelRecv | 0B | 接收消息 |
| 0x6A | CondvarNew | 0B | 创建 Condvar |
| 0x6B | CondvarWait | 0B | 等待条件 |
| 0x6C | CondvarSignal | 0B | 唤醒一个 |
| 0x6D | CondvarBroadcast | 0B | 唤醒全部 |

### 2.2 修改文件

- `compiler/src/codegen/opcode.rs` — OpCode enum 新增 26 个变体 + byte/operand_size/from_byte/write/Display 五件套
- `compiler/src/vm/mod.rs` — Instr enum 新增 26 个变体 + decode_function 匹配
- `compiler/src/vm/interp.rs` — exec_instr 新增 26 个分支
- `compiler/src/vm/debugger.rs` — format_instr 新增 26 个分支

---

## 3. 并发原生函数

### 3.1 新增文件

- `compiler/src/vm/concurrent_native.rs` — 并发原生函数实现

### 3.2 实现方式

采用 **Rust 标准库原语**（非 C FFI）实现所有并发原语：

| 原语 | Rust 实现 | 线程安全机制 |
|------|-----------|-------------|
| Mutex | `RawMutex`（AtomicBool + Condvar + Mutex） | spinlock + 条件变量阻塞 |
| Atomic | `AtomicI64`（std::sync::atomic） | CPU 原子指令 |
| RwLock | `RawRwLock`（AtomicI64 readers/writers + Condvar） | 读写锁语义 |
| Condvar | `RawCondvar`（std::sync::Condvar） | 内核条件变量 |
| Barrier | `RawBarrier`（AtomicI64 remaining + Condvar） | 计数屏障 |

### 3.3 原生函数清单（30 个）

**Thread**（6 个）：
- `aura.lang.std.Thread.spawn` — 创建线程
- `aura.lang.std.Thread.join` — 等待线程
- `aura.lang.std.Thread.sleep` — 休眠
- `aura.lang.std.Thread.id` — 获取线程 ID
- `aura.lang.std.Thread.parallelism` — 获取并行度
- `aura.lang.std.Thread.availableCores` — 获取核心数

**Mutex**（5 个）：
- `aura.lang.std.Mutex.new` — 创建
- `aura.lang.std.Mutex.lock` — 加锁
- `aura.lang.std.Mutex.unlock` — 解锁
- `aura.lang.std.Mutex.tryLock` — 尝试加锁
- `aura.lang.std.Mutex.destroy` — 销毁

**Atomic**（6 个）：
- `aura.lang.std.Atomic.new` — 创建
- `aura.lang.std.Atomic.load` — 读取
- `aura.lang.std.Atomic.store` — 写入
- `aura.lang.std.Atomic.add` — 加法
- `aura.lang.std.Atomic.sub` — 减法
- `aura.lang.std.Atomic.cas` — 比较交换

**RwLock**（6 个）：
- `aura.lang.std.RwLock.new` — 创建
- `aura.lang.std.RwLock.readLock` — 读锁
- `aura.lang.std.RwLock.writeLock` — 写锁
- `aura.lang.std.RwLock.readUnlock` — 释放读锁
- `aura.lang.std.RwLock.writeUnlock` — 释放写锁
- `aura.lang.std.RwLock.destroy` — 销毁

**Condvar**（5 个）：
- `aura.lang.std.Condvar.new` — 创建
- `aura.lang.std.Condvar.wait` — 等待
- `aura.lang.std.Condvar.signal` — 唤醒一个
- `aura.lang.std.Condvar.broadcast` — 唤醒全部
- `aura.lang.std.Condvar.destroy` — 销毁

**Barrier**（3 个）：
- `aura.lang.std.Barrier.new` — 创建
- `aura.lang.std.Barrier.wait` — 等待
- `aura.lang.std.Barrier.destroy` — 销毁

---

## 4. 测试

### 4.1 新增文件

- `compiler/tests/concurrent_vm_tests.rs` — 12 个并发 VM 测试

### 4.2 测试结果

```
test test_atomic_native_add_sub ... ok
test test_atomic_native_cas ... ok
test test_atomic_native_load_store ... ok
test test_barrier_native_basic ... ok
test test_concurrent_atomic_with_mutex_stress ... ok
test test_concurrent_native_registration ... ok
test test_condvar_native_basic ... ok
test test_mutex_native_lock_unlock ... ok
test test_mutex_native_trylock ... ok
test test_rwlock_native_basic ... ok
test test_thread_native_id ... ok
test test_thread_native_parallelism ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 4.3 测试覆盖

| 测试 | 覆盖内容 |
|------|---------|
| test_mutex_native_lock_unlock | Mutex 创建/加锁/解锁/销毁 |
| test_mutex_native_trylock | Mutex tryLock 成功/失败场景 |
| test_atomic_native_load_store | Atomic 创建/写入/读取 |
| test_atomic_native_add_sub | Atomic 加减法 |
| test_atomic_native_cas | Atomic 比较交换（成功/失败） |
| test_rwlock_native_basic | RwLock 读写锁/释放/销毁 |
| test_thread_native_parallelism | Thread 并行度查询 |
| test_thread_native_id | Thread ID 获取 |
| test_condvar_native_basic | Condvar 创建/销毁 |
| test_barrier_native_basic | Barrier 创建/等待/销毁 |
| test_concurrent_native_registration | 全部 30 个原生函数注册完整性 |
| test_concurrent_atomic_with_mutex_stress | 并发压力测试（16 线程 × 500 次原子操作） |

---

## 5. 修改文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `compiler/src/codegen/opcode.rs` | 修改 | 新增 26 个 OpCode 变体 + 五件套 |
| `compiler/src/vm/mod.rs` | 修改 | 新增 26 个 Instr 变体 + decode_function 匹配 |
| `compiler/src/vm/interp.rs` | 修改 | exec_instr 新增 26 个分发分支 |
| `compiler/src/vm/debugger.rs` | 修改 | format_instr 新增 26 个分支 |
| `compiler/src/vm/concurrent_native.rs` | **新增** | 并发原生函数实现（30 个函数） |
| `compiler/src/vm/native.rs` | 修改 | 注册 concurrent_native 函数 |
| `compiler/tests/concurrent_vm_tests.rs` | **新增** | 12 个并发 VM 测试 |

---

## 6. 待后续完成

| 项目 | 优先级 | 说明 |
|------|--------|------|
| Heap 线程安全改造 | 高 | `AtomicUsize rc` + `Mutex<HashMap>` 包装 |
| M:N 协程调度器 | 高 | 跨线程协程调度，`thread::spawn` 集成 |
| 事件驱动 Channel | 中 | 替换 sleep-poll 为 `Condvar` 阻塞等待 |
| Actor 调度循环 | 中 | 消息分发循环 + 跨线程 mailbox |
| 字节码序列化版本升级 | 低 | 新指令需递增 .auc 版本号 |
| AOT/JIT 并发指令支持 | 低 | LLVM IR / Cranelift 代码生成 |
