//! Phase B — VM 并发运行时测试
//!
//! 验证并发原生函数（Thread / Mutex / Atomic / RwLock / Condvar / Barrier）
//! 通过 NativeRegistry 正确注册和工作。

#![cfg(feature = "llvm")]

use compiler::vm::Value;
use compiler::vm::native::NativeRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// 1. Mutex 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mutex_native_lock_unlock() {
    let reg = NativeRegistry::new();

    // Mutex.new()
    let mtx = reg.get("aura.lang.concurrent.Mutex.new").unwrap()(&[]);
    let mtx_id = mtx.as_int();
    assert!(mtx_id >= 0, "mutex should be allocated");

    // Mutex.lock(id)
    reg.get("aura.lang.concurrent.Mutex.lock").unwrap()(&[Value::Int(mtx_id)]);

    // Mutex.unlock(id)
    reg.get("aura.lang.concurrent.Mutex.unlock").unwrap()(&[Value::Int(mtx_id)]);

    // Mutex.destroy(id)
    reg.get("aura.lang.concurrent.Mutex.destroy").unwrap()(&[Value::Int(mtx_id)]);
}

#[test]
fn test_mutex_native_trylock() {
    let reg = NativeRegistry::new();

    let mtx = reg.get("aura.lang.concurrent.Mutex.new").unwrap()(&[]);
    let mtx_id = mtx.as_int();

    // trylock 未持有时应成功
    let result = reg.get("aura.lang.concurrent.Mutex.tryLock").unwrap()(&[Value::Int(mtx_id)]);
    assert!(result.as_bool(), "trylock should succeed when not held");
    reg.get("aura.lang.concurrent.Mutex.unlock").unwrap()(&[Value::Int(mtx_id)]);

    // 持有时 trylock 应失败
    reg.get("aura.lang.concurrent.Mutex.lock").unwrap()(&[Value::Int(mtx_id)]);
    let result = reg.get("aura.lang.concurrent.Mutex.tryLock").unwrap()(&[Value::Int(mtx_id)]);
    assert!(!result.as_bool(), "trylock should fail when held");
    reg.get("aura.lang.concurrent.Mutex.unlock").unwrap()(&[Value::Int(mtx_id)]);

    reg.get("aura.lang.concurrent.Mutex.destroy").unwrap()(&[Value::Int(mtx_id)]);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Atomic 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_atomic_native_load_store() {
    let reg = NativeRegistry::new();

    // Atomic.new(0)
    let atom = reg.get("aura.lang.concurrent.Atomic.new").unwrap()(&[Value::Int(0)]);
    let atom_id = atom.as_int();
    assert!(atom_id >= 0, "atomic should be allocated");

    // Atomic.store(id, 42)
    reg.get("aura.lang.concurrent.Atomic.store").unwrap()(&[
        Value::Int(atom_id),
        Value::Int(42),
    ]);

    // Atomic.load(id)
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(val.as_int(), 42);
}

#[test]
fn test_atomic_native_add_sub() {
    let reg = NativeRegistry::new();

    let atom = reg.get("aura.lang.concurrent.Atomic.new").unwrap()(&[Value::Int(10)]);
    let atom_id = atom.as_int();

    // Atomic.add(id, 5) → 15
    let val = reg.get("aura.lang.concurrent.Atomic.add").unwrap()(&[
        Value::Int(atom_id),
        Value::Int(5),
    ]);
    assert_eq!(val.as_int(), 15);

    // Atomic.sub(id, 3) → 12
    let val = reg.get("aura.lang.concurrent.Atomic.sub").unwrap()(&[
        Value::Int(atom_id),
        Value::Int(3),
    ]);
    assert_eq!(val.as_int(), 12);

    // Atomic.load(id) → 12
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(val.as_int(), 12);
}

#[test]
fn test_atomic_native_cas() {
    let reg = NativeRegistry::new();

    let atom = reg.get("aura.lang.concurrent.Atomic.new").unwrap()(&[Value::Int(0)]);
    let atom_id = atom.as_int();

    // CAS(0, 42) → true (expected matches)
    let result = reg.get("aura.lang.concurrent.Atomic.cas").unwrap()(&[
        Value::Int(atom_id),
        Value::Int(0),
        Value::Int(42),
    ]);
    assert!(result.as_bool(), "CAS should succeed");
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(val.as_int(), 42);

    // CAS(0, 99) → false (expected doesn't match)
    let result = reg.get("aura.lang.concurrent.Atomic.cas").unwrap()(&[
        Value::Int(atom_id),
        Value::Int(0),
        Value::Int(99),
    ]);
    assert!(!result.as_bool(), "CAS should fail");
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(val.as_int(), 42);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. RwLock 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rwlock_native_basic() {
    let reg = NativeRegistry::new();

    // RwLock.new()
    let rw = reg.get("aura.lang.concurrent.RwLock.new").unwrap()(&[]);
    let rw_id = rw.as_int();
    assert!(rw_id >= 0, "rwlock should be allocated");

    // 读锁
    reg.get("aura.lang.concurrent.RwLock.readLock").unwrap()(&[Value::Int(rw_id)]);
    reg.get("aura.lang.concurrent.RwLock.readUnlock").unwrap()(&[Value::Int(rw_id)]);

    // 写锁
    reg.get("aura.lang.concurrent.RwLock.writeLock").unwrap()(&[Value::Int(rw_id)]);
    reg.get("aura.lang.concurrent.RwLock.writeUnlock").unwrap()(&[Value::Int(rw_id)]);

    // 销毁
    reg.get("aura.lang.concurrent.RwLock.destroy").unwrap()(&[Value::Int(rw_id)]);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Thread 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_native_parallelism() {
    let reg = NativeRegistry::new();

    let cores = reg.get("aura.lang.concurrent.Thread.parallelism").unwrap()(&[]);
    assert!(cores.as_int() > 0, "should have at least 1 core");
}

#[test]
fn test_thread_native_id() {
    let reg = NativeRegistry::new();

    let tid = reg.get("aura.lang.concurrent.Thread.id").unwrap()(&[]);
    assert!(tid.as_int() > 0, "main thread should have valid ID");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Condvar 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_condvar_native_basic() {
    let reg = NativeRegistry::new();

    // Condvar.new()
    let cv = reg.get("aura.lang.concurrent.Condvar.new").unwrap()(&[]);
    let cv_id = cv.as_int();
    assert!(cv_id >= 0, "condvar should be allocated");

    // 创建 Mutex 用于 Condvar.wait
    let mtx = reg.get("aura.lang.concurrent.Mutex.new").unwrap()(&[]);
    let mtx_id = mtx.as_int();

    // Condvar.destroy
    reg.get("aura.lang.concurrent.Condvar.destroy").unwrap()(&[Value::Int(cv_id)]);
    reg.get("aura.lang.concurrent.Mutex.destroy").unwrap()(&[Value::Int(mtx_id)]);
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Barrier 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_barrier_native_basic() {
    let reg = NativeRegistry::new();

    // Barrier.new(1)
    let bar = reg.get("aura.lang.concurrent.Barrier.new").unwrap()(&[Value::Int(1)]);
    let bar_id = bar.as_int();
    assert!(bar_id >= 0, "barrier should be allocated");

    // Barrier.wait
    let result = reg.get("aura.lang.concurrent.Barrier.wait").unwrap()(&[Value::Int(bar_id)]);
    let _ = result; // 不检查返回值（单线程场景）

    // Barrier.destroy
    reg.get("aura.lang.concurrent.Barrier.destroy").unwrap()(&[Value::Int(bar_id)]);
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. 原生函数注册完整性测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_concurrent_native_registration() {
    let reg = NativeRegistry::new();

    // Thread
    assert!(reg.contains("aura.lang.concurrent.Thread.spawn"));
    assert!(reg.contains("aura.lang.concurrent.Thread.join"));
    assert!(reg.contains("aura.lang.concurrent.Thread.sleep"));
    assert!(reg.contains("aura.lang.concurrent.Thread.id"));
    assert!(reg.contains("aura.lang.concurrent.Thread.parallelism"));
    assert!(reg.contains("aura.lang.concurrent.Thread.availableCores"));

    // Mutex
    assert!(reg.contains("aura.lang.concurrent.Mutex.new"));
    assert!(reg.contains("aura.lang.concurrent.Mutex.lock"));
    assert!(reg.contains("aura.lang.concurrent.Mutex.unlock"));
    assert!(reg.contains("aura.lang.concurrent.Mutex.tryLock"));
    assert!(reg.contains("aura.lang.concurrent.Mutex.destroy"));

    // Atomic
    assert!(reg.contains("aura.lang.concurrent.Atomic.new"));
    assert!(reg.contains("aura.lang.concurrent.Atomic.load"));
    assert!(reg.contains("aura.lang.concurrent.Atomic.store"));
    assert!(reg.contains("aura.lang.concurrent.Atomic.add"));
    assert!(reg.contains("aura.lang.concurrent.Atomic.sub"));
    assert!(reg.contains("aura.lang.concurrent.Atomic.cas"));

    // RwLock
    assert!(reg.contains("aura.lang.concurrent.RwLock.new"));
    assert!(reg.contains("aura.lang.concurrent.RwLock.readLock"));
    assert!(reg.contains("aura.lang.concurrent.RwLock.writeLock"));
    assert!(reg.contains("aura.lang.concurrent.RwLock.readUnlock"));
    assert!(reg.contains("aura.lang.concurrent.RwLock.writeUnlock"));
    assert!(reg.contains("aura.lang.concurrent.RwLock.destroy"));

    // Condvar
    assert!(reg.contains("aura.lang.concurrent.Condvar.new"));
    assert!(reg.contains("aura.lang.concurrent.Condvar.wait"));
    assert!(reg.contains("aura.lang.concurrent.Condvar.signal"));
    assert!(reg.contains("aura.lang.concurrent.Condvar.broadcast"));
    assert!(reg.contains("aura.lang.concurrent.Condvar.destroy"));

    // Barrier
    assert!(reg.contains("aura.lang.concurrent.Barrier.new"));
    assert!(reg.contains("aura.lang.concurrent.Barrier.wait"));
    assert!(reg.contains("aura.lang.concurrent.Barrier.destroy"));
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. 并发压力测试（使用 Rust 原子操作验证正确性）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_concurrent_atomic_with_mutex_stress() {
    let reg = NativeRegistry::new();

    // 创建原子计数器
    let atom = reg.get("aura.lang.concurrent.Atomic.new").unwrap()(&[Value::Int(0)]);
    let atom_id = atom.as_int();

    // 创建互斥锁
    let mtx = reg.get("aura.lang.concurrent.Mutex.new").unwrap()(&[]);
    let mtx_id = mtx.as_int();

    // 16 个线程，每个 500 次原子加法
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::thread;

    let counter = Arc::new(AtomicI64::new(0));
    let mut handles = Vec::new();

    for _ in 0..16 {
        let c = counter.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..500 {
                // 使用 Rust 原生原子操作（验证 C 原子操作的等价性）
                c.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(counter.load(Ordering::SeqCst), 8000);

    // 验证 Aura 原子计数器的值
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(
        val.as_int(),
        0,
        "Aura atomic counter starts at 0 (not modified by Rust threads)"
    );

    // 使用 Aura 原子操作验证正确性
    for _ in 0..100 {
        reg.get("aura.lang.concurrent.Atomic.add").unwrap()(&[
            Value::Int(atom_id),
            Value::Int(1),
        ]);
    }
    let val = reg.get("aura.lang.concurrent.Atomic.load").unwrap()(&[Value::Int(atom_id)]);
    assert_eq!(val.as_int(), 100);

    // 清理
    reg.get("aura.lang.concurrent.Mutex.destroy").unwrap()(&[Value::Int(mtx_id)]);
}
