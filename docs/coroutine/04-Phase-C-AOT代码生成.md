# Phase C: AOT 代码生成

## 目标

在 AOT 后端（`codegen/aot/emit.rs`）中实现并发指令的 LLVM IR 生成，使并发代码能生成原生机器码。

## 文件清单

| 文件 | 操作 | 说明 |
|------|------|------|
| `compiler/src/codegen/aot/emit.rs` | 修改 | 并发指令 IR 生成 |
| `compiler/src/codegen/aot/runtime.rs` | 修改 | 新增运行时函数声明 |
| `compiler/src/codegen/aot/ffi.rs` | 修改 | 并发 FFI 声明 |

## 设计

### 1. 运行时函数声明

在 `runtime.rs` 的 `RUNTIME_FUNCTIONS` 中新增：

```rust
// ── 线程 ──
RuntimeFn { name: "aura_thread_create", ret: "i64", params: &[("fn", "i64"), ("arg", "i64")] },
RuntimeFn { name: "aura_thread_join", ret: "i32", params: &[("tid", "i64")] },
RuntimeFn { name: "aura_thread_sleep", ret: "void", params: &[("ms", "i64")] },

// ── Mutex ──
RuntimeFn { name: "aura_mutex_new", ret: "i64", params: &[] },
RuntimeFn { name: "aura_mutex_lock", ret: "void", params: &[("m", "i64")] },
RuntimeFn { name: "aura_mutex_unlock", ret: "void", params: &[("m", "i64")] },
RuntimeFn { name: "aura_mutex_trylock", ret: "i32", params: &[("m", "i64")] },

// ── 原子操作 ──
RuntimeFn { name: "aura_atomic_add", ret: "i64", params: &[("addr", "i64*"), ("delta", "i64")] },
RuntimeFn { name: "aura_atomic_load", ret: "i64", params: &[("addr", "i64*")] },
RuntimeFn { name: "aura_atomic_store", ret: "void", params: &[("addr", "i64*"), ("val", "i64")] },
RuntimeFn { name: "aura_atomic_cas", ret: "i64", params: &[("addr", "i64*"), ("exp", "i64"), ("des", "i64")] },
```

### 2. 并发指令 → LLVM IR 映射

```rust
// ThreadSpawn: 生成调用 aura_thread_create 的 IR
OpCode::ThreadSpawn(func_idx) => {
    // 1. 获取函数地址（通过函数表）
    // 2. 生成 call i64 @aura_thread_create(i64 %func_addr, i64 %arg)
    // 3. 返回线程 ID 到栈顶
}

// MutexLock: 生成调用 aura_mutex_lock 的 IR
OpCode::MutexLock => {
    // 生成 call void @aura_mutex_lock(i64 %arg.0)
}

// AtomicAdd: 生成 LLVM 内建原子操作
OpCode::AtomicAdd => {
    // 方案 A: 调用 aura_atomic_add
    // 方案 B: 使用 LLVM 内建 @llvm.atomicrmw.add（更高效）
    // 生成 atomicrmw add i64* %arg.addr, i64 %arg.delta seq_cst
}

// AtomicCAS: 生成 LLVM 内建
OpCode::AtomicCAS => {
    // 生成 cmpxchg i64* %arg.addr, i64 %arg.exp, i64 %arg.des seq_cst
}
```

### 3. ARC 跨线程安全

`aura_arc_increment`/`aura_arc_decrement` 已使用原子操作，但需要注意：

```rust
// 问题：对象在跨线程时，drop 回调可能在不同线程执行
// 解决：ARC 的 release 操作需要在全局锁或分片锁下执行

fn atomic_dec_ref(ptr: *mut ObjHeader) -> bool {
    let old_count = unsafe { __sync_sub_and_fetch(&(*ptr).refcount, 1) };
    if old_count <= 0 {
        // 最后引用释放，执行 drop 回调
        // 需要在锁保护下执行
        let lock = GLOBAL_DROPPER.lock();
        drop_object(ptr);
    }
    old_count <= 0
}
```

### 4. JIT 值 ABI 中的并发类型

```rust
// JitValue 需要支持并发句柄
pub struct JitValue {
    tag: u8,  // TAG_THREAD, TAG_MUTEX, TAG_ATOMIC, ...
    data: [u8; 24],
}

// 新标签
pub const TAG_THREAD: u8 = 0x10;
pub const TAG_MUTEX: u8 = 0x11;
pub const TAG_RWLOCK: u8 = 0x12;
pub const TAG_ATOMIC: u8 = 0x13;
pub const TAG_CONDVAR: u8 = 0x14;
pub const TAG_BARRIER: u8 = 0x15;
```

## 测试计划

### LLVM IR 单元测试

```rust
// compiler/tests/aot_concurrent_tests.rs
#[test]
fn test_thread_spawn_ir_generation() { ... }
#[test]
fn test_mutex_ir_generation() { ... }
#[test]
fn test_atomic_ir_generation() { ... }
```

### AOT 端到端测试

```rust
#[test]
fn test_aot_thread_create() { ... }
#[test]
fn test_aot_mutex() { ... }
#[test]
fn test_aot_atomic() { ... }
```

## 依赖

- Phase A: C 运行时原语
- Phase B: VM 指令集定义
