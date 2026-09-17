# Phase D — JIT 后端并发支持 完成报告

> **状态**：✅ 完成  
> **日期**：2026-07-04  
> **关联设计文档**：`docs/coroutine/05-Phase-D-JIT后端并发支持.md`

---

## 1. 完成项总览

| 项目 | 状态 | 说明 |
|------|------|------|
| 并发指令 JIT 白名单 | ✅ 完成 | 26 个并发指令加入 `is_jit_compilable` 白名单 |
| 名称调度器 | ✅ 完成 | `aura_jit_call_native_by_name` 函数 |
| 并发指令 JIT 发射 | ✅ 完成 | 26 个并发指令 Cranelift IR 发射 |
| JIT 并发测试 | ✅ 完成 | 8 个测试全部通过 |

---

## 2. 并发指令 JIT 白名单

### 2.1 新增白名单指令（26 个）

位于 `compiler/src/vm/jit.rs` `is_jit_compilable_inner` 函数：

**Thread**（5 个）：
- `ThreadSpawn`, `ThreadJoin`, `ThreadSleep`, `ThreadId`, `ThreadParallelism`

**Mutex**（4 个）：
- `MutexNew`, `MutexLock`, `MutexUnlock`, `MutexTryLock`

**Atomic**（5 个）：
- `AtomicNew`, `AtomicLoad`, `AtomicStore`, `AtomicAdd`, `AtomicCas`

**RwLock**（5 个）：
- `RwLockNew`, `RwLockReadLock`, `RwLockWriteLock`, `RwLockReadUnlock`, `RwLockWriteUnlock`

**Channel**（3 个）：
- `ChannelNew`, `ChannelSend`, `ChannelRecv`

**Condvar**（4 个）：
- `CondvarNew`, `CondvarWait`, `CondvarSignal`, `CondvarBroadcast`

---

## 3. 名称调度器

### 3.1 新增函数

`aura_jit_call_native_by_name` 位于 `compiler/src/vm/jit_native.rs`：

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aura_jit_call_native_by_name(
    name_ptr: *const u8,  // 函数名 C 字符串指针
    name_len: i64,        // 函数名长度
    argc: i64,            // 参数个数
    args_ptr: *const JitValue,  // 参数数组指针
    out_ptr: *mut JitValue,     // 返回值输出指针
)
```

### 3.2 工作原理

1. JIT 编译时，函数名作为栈上常量写入
2. JIT 发射 `call aura_jit_call_native_by_name(name_ptr, name_len, argc, args_ptr, out_ptr)`
3. 调度器从 NativeRegistry 按名称查找原生函数
4. 调用原生函数并返回结果

---

## 4. 并发指令 JIT 发射

### 4.1 emit_concurrent_call 辅助函数

位于 `compiler/src/vm/jit.rs`，处理并发指令的通用 JIT 发射：

```rust
fn emit_concurrent_call(
    fb: &mut FunctionBuilder,
    func_name: &str,
    arg_count: i64,
    push: &mut dyn FnMut(...),
    pop: &mut dyn FnMut(...),
    stack_slot: &StackSlot,
    sp: Variable,
    ptr_ty: types::Type,
    native_name_dispatch_entry: FuncRef,
)
```

### 4.2 发射流程

1. 在栈上分配空间存储函数名
2. 逐字节写入函数名 + null 终止符
3. 准备调用栈帧（args_sp, args_ptr, out_ptr）
4. 调用 `aura_jit_call_native_by_name`
5. 从 out_ptr 加载返回值并 push

### 4.3 函数名映射

| JIT 指令 | 调用函数名 | 参数数 |
|----------|-----------|--------|
| ThreadSpawn | `aura.lang.std.Thread.spawn` | 2 |
| ThreadJoin | `aura.lang.std.Thread.join` | 1 |
| ThreadSleep | `aura.lang.std.Thread.sleep` | 1 |
| ThreadId | `aura.lang.std.Thread.id` | 0 |
| ThreadParallelism | `aura.lang.std.Thread.parallelism` | 0 |
| MutexNew | `aura.lang.std.Mutex.new` | 0 |
| MutexLock | `aura.lang.std.Mutex.lock` | 1 |
| MutexUnlock | `aura.lang.std.Mutex.unlock` | 1 |
| MutexTryLock | `aura.lang.std.Mutex.tryLock` | 1 |
| AtomicNew | `aura.lang.std.Atomic.new` | 1 |
| AtomicLoad | `aura.lang.std.Atomic.load` | 1 |
| AtomicStore | `aura.lang.std.Atomic.store` | 2 |
| AtomicAdd | `aura.lang.std.Atomic.add` | 2 |
| AtomicCas | `aura.lang.std.Atomic.cas` | 3 |
| RwLockNew | `aura.lang.std.RwLock.new` | 0 |
| RwLockReadLock | `aura.lang.std.RwLock.readLock` | 1 |
| RwLockWriteLock | `aura.lang.std.RwLock.writeLock` | 1 |
| RwLockReadUnlock | `aura.lang.std.RwLock.readUnlock` | 1 |
| RwLockWriteUnlock | `aura.lang.std.RwLock.writeUnlock` | 1 |
| ChannelNew | `aura.lang.std.Channel.newChannel` | 1 |
| ChannelSend | `aura.lang.std.Channel.channelSend` | 2 |
| ChannelRecv | `aura.lang.std.Channel.channelRecv` | 1 |
| CondvarNew | `aura.lang.std.Condvar.new` | 0 |
| CondvarWait | `aura.lang.std.Condvar.wait` | 2 |
| CondvarSignal | `aura.lang.std.Condvar.signal` | 1 |
| CondvarBroadcast | `aura.lang.std.Condvar.broadcast` | 1 |

---

## 5. 修改文件清单

| 文件 | 修改类型 | 说明 |
|------|---------|------|
| `compiler/src/vm/jit.rs` | 修改 | 白名单 + 26 个并发指令 emit + `emit_concurrent_call` 辅助函数 |
| `compiler/src/vm/jit_native.rs` | 修改 | 新增 `aura_jit_call_native_by_name` 名称调度器 |
| `compiler/tests/concurrent_jit_tests.rs` | **新增** | 8 个 JIT 并发测试 |
| `compiler/src/codegen/aot/mod.rs` | 修改 | Channel 函数名修正（newChannel/channelSend/channelRecv） |

---

## 6. 测试结果

```
test test_jit_mixed_concurrent_and_arithmetic ... ok
test test_jit_name_based_dispatcher_registered ... ok
test test_jit_whitelist_atomic_instructions ... ok
test test_jit_whitelist_channel_instructions ... ok
test test_jit_whitelist_condvar_instructions ... ok
test test_jit_whitelist_mutex_instructions ... ok
test test_jit_whitelist_rwlock_instructions ... ok
test test_jit_whitelist_thread_instructions ... ok

test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 6.1 测试覆盖

| 测试 | 覆盖内容 |
|------|---------|
| test_jit_whitelist_thread_instructions | Thread 指令 JIT 白名单 |
| test_jit_whitelist_mutex_instructions | Mutex 指令 JIT 白名单 |
| test_jit_whitelist_atomic_instructions | Atomic 指令 JIT 白名单 |
| test_jit_whitelist_rwlock_instructions | RwLock 指令 JIT 白名单 |
| test_jit_whitelist_channel_instructions | Channel 指令 JIT 白名单 |
| test_jit_whitelist_condvar_instructions | Condvar 指令 JIT 白名单 |
| test_jit_name_based_dispatcher_registered | 名称调度器注册完整性（27 个函数） |
| test_jit_mixed_concurrent_and_arithmetic | 并发 + 算术指令混合 JIT 编译 |

---

## 7. 待后续完成

| 项目 | 优先级 | 说明 |
|------|--------|------|
| Phase E: 标准库暴露 | 高 | Thread/Mutex/Atomic/Future Aura 类 + Coroutine.aura/Actor.aura 改造 |
| JIT 并发性能优化 | 中 | 函数名常量池优化（避免栈写入） |
| JIT 并发集成测试 | 中 | 完整 JIT 编译 + 执行并发程序 |
