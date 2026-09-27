#!/usr/bin/env cargo-test
// Phase A — C 运行时并发原语测试
//
// 验证 aura_syscalls.c 中的线程、Mutex、RwLock、Atomic、Condvar、Barrier 实现。
// 通过 FFI 调用 C 函数，在 Rust 测试中验证。

#![cfg(feature = "llvm")]

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::thread;
use std::time::Duration;

// ─────────────────────────────────────────────────────────────────────────────
// FFI 声明（匹配 aura_syscalls.c 中的 C 函数签名）
// ─────────────────────────────────────────────────────────────────────────────

#[link(name = "aura_syscalls")]
unsafe extern "C" {
    // Thread
    fn aura_thread_create(fn_ptr: usize, arg: i64) -> i64;
    fn aura_thread_join(id: i64) -> i32;
    fn aura_thread_sleep(ms: i64) -> i32;
    fn aura_thread_id() -> i64;
    fn aura_thread_available_parallelism() -> i64;

    // Mutex
    fn aura_mutex_new() -> usize;
    fn aura_mutex_lock(id: usize) -> i32;
    fn aura_mutex_unlock(id: usize) -> i32;
    fn aura_mutex_trylock(id: usize) -> i32;
    fn aura_mutex_destroy(id: usize) -> i32;

    // RwLock
    fn aura_rwlock_new() -> usize;
    fn aura_rwlock_read_lock(id: usize) -> i32;
    fn aura_rwlock_write_lock(id: usize) -> i32;
    fn aura_rwlock_read_unlock(id: usize) -> i32;
    fn aura_rwlock_write_unlock(id: usize) -> i32;
    fn aura_rwlock_destroy(id: usize) -> i32;

    // Atomic
    fn aura_atomic_load(addr: *mut i64) -> i64;
    fn aura_atomic_store(addr: *mut i64, val: i64) -> i32;
    fn aura_atomic_add(addr: *mut i64, delta: i64) -> i64;
    fn aura_atomic_sub(addr: *mut i64, delta: i64) -> i64;
    fn aura_atomic_cas(addr: *mut i64, expected: i64, desired: i64) -> i32;

    // Barrier
    fn aura_barrier_new(count: i64) -> usize;
    fn aura_barrier_wait(id: usize) -> i32;
    fn aura_barrier_destroy(id: usize) -> i32;

    // TLS
    fn aura_tls_key_create() -> i32;
    fn aura_tls_get(key_idx: i32) -> i64;
    fn aura_tls_set(key_idx: i32, val: i64) -> i32;
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Mutex 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mutex_lock_unlock() {
    unsafe {
        let mtx = aura_mutex_new();
        assert!(mtx != 0, "mutex should be allocated");

        aura_mutex_lock(mtx);
        aura_mutex_unlock(mtx);
        aura_mutex_destroy(mtx);
    }
}

#[test]
fn test_mutex_trylock() {
    unsafe {
        let mtx = aura_mutex_new();
        assert!(mtx != 0);

        // 未持有时 trylock 应成功
        assert_eq!(aura_mutex_trylock(mtx), 1);
        aura_mutex_unlock(mtx);

        // 持有时 trylock 应失败
        aura_mutex_lock(mtx);
        assert_eq!(aura_mutex_trylock(mtx), 0);
        aura_mutex_unlock(mtx);

        aura_mutex_destroy(mtx);
    }
}

#[test]
fn test_mutex_thread_safety() {
    unsafe {
        let mtx = aura_mutex_new();
        let counter = Arc::new(AtomicI64::new(0));

        let mut handles = Vec::new();
        for _ in 0..4 {
            let c = counter.clone();
            let m = mtx;
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    aura_mutex_lock(m);
                    let old = c.load(Ordering::SeqCst);
                    thread::sleep(Duration::from_nanos(1));
                    c.store(old + 1, Ordering::SeqCst);
                    aura_mutex_unlock(m);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 4000);
        aura_mutex_destroy(mtx);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. RwLock 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rwlock_basic() {
    unsafe {
        let rw = aura_rwlock_new();
        assert!(rw != 0, "rwlock should be allocated");

        // 读锁
        aura_rwlock_read_lock(rw);
        aura_rwlock_read_unlock(rw);

        // 写锁
        aura_rwlock_write_lock(rw);
        aura_rwlock_write_unlock(rw);

        aura_rwlock_destroy(rw);
    }
}

#[test]
fn test_rwlock_concurrent_reads() {
    unsafe {
        let rw = aura_rwlock_new();
        let counter = Arc::new(AtomicI64::new(0));

        let mut handles = Vec::new();
        // 4 个读线程
        for _ in 0..4 {
            let c = counter.clone();
            let r = rw;
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    aura_rwlock_read_lock(r);
                    let _ = c.load(Ordering::SeqCst);
                    aura_rwlock_read_unlock(r);
                }
                c.fetch_add(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(counter.load(Ordering::SeqCst), 4);
        aura_rwlock_destroy(rw);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Atomic 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_atomic_load_store() {
    unsafe {
        let mut val: i64 = 0;
        aura_atomic_store(&mut val, 42);
        assert_eq!(aura_atomic_load(&mut val), 42);
    }
}

#[test]
fn test_atomic_add_sub() {
    unsafe {
        let mut val: i64 = 10;
        let old = aura_atomic_add(&mut val, 5);
        assert_eq!(old, 10);
        assert_eq!(aura_atomic_load(&mut val), 15);

        let old = aura_atomic_sub(&mut val, 3);
        assert_eq!(old, 15);
        assert_eq!(aura_atomic_load(&mut val), 12);
    }
}

#[test]
fn test_atomic_cas() {
    unsafe {
        let mut val: i64 = 0;

        // 成功 CAS
        assert_eq!(aura_atomic_cas(&mut val, 0, 42), 1);
        assert_eq!(aura_atomic_load(&mut val), 42);

        // 失败 CAS（expected 不匹配）
        assert_eq!(aura_atomic_cas(&mut val, 0, 99), 0);
        assert_eq!(aura_atomic_load(&mut val), 42);

        // 成功 CAS（expected 匹配）
        assert_eq!(aura_atomic_cas(&mut val, 42, 99), 1);
        assert_eq!(aura_atomic_load(&mut val), 99);
    }
}

#[test]
fn test_atomic_thread_safety() {
    unsafe {
        let mut val: i64 = 0;
        let addr = &mut val as *mut i64;
        let counter = Arc::new(AtomicI64::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let a = addr as i64;
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    aura_atomic_add(a as *mut i64, 1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(aura_atomic_load(addr), 8000);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Barrier 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_barrier_basic() {
    unsafe {
        let barrier = aura_barrier_new(2);
        assert!(barrier != 0, "barrier should be allocated");

        let mut handles = Vec::new();
        let started = Arc::new(AtomicI64::new(0));

        // 线程 1
        {
            let s = started.clone();
            handles.push(thread::spawn(move || {
                aura_barrier_wait(barrier);
                s.fetch_add(1, Ordering::SeqCst);
            }));
        }

        // 线程 2
        {
            let s = started.clone();
            handles.push(thread::spawn(move || {
                aura_barrier_wait(barrier);
                s.fetch_add(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // 两个线程都应到达屏障
        assert_eq!(started.load(Ordering::SeqCst), 2);
        aura_barrier_destroy(barrier);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. TLS 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_tls_basic() {
    unsafe {
        let key = aura_tls_key_create();
        assert!(key >= 0, "TLS key should be created");

        // 设置和获取
        aura_tls_set(key, 12345);
        assert_eq!(aura_tls_get(key), 12345);

        // 修改
        aura_tls_set(key, 99999);
        assert_eq!(aura_tls_get(key), 99999);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Thread 测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_available_parallelism() {
    unsafe {
        let cores = aura_thread_available_parallelism();
        assert!(cores > 0, "should have at least 1 core");
    }
}

#[test]
fn test_thread_id() {
    unsafe {
        let main_id = aura_thread_id();
        assert!(main_id > 0, "main thread should have valid ID");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. 综合并发测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_concurrent_stress() {
    unsafe {
        // 创建多个线程，使用原子操作和 mutex 进行并发计数
        let mut val: i64 = 0;
        let addr = &mut val as *mut i64;
        let mtx = aura_mutex_new();

        let mut handles = Vec::new();
        for _ in 0..16 {
            let a = addr as i64;
            let m = mtx;
            handles.push(thread::spawn(move || {
                for _ in 0..500 {
                    aura_mutex_lock(m);
                    aura_atomic_add(a as *mut i64, 1);
                    aura_mutex_unlock(m);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // 每个线程 500 次，共 16 线程 = 8000
        assert_eq!(aura_atomic_load(addr), 8000);
        aura_mutex_destroy(mtx);
    }
}
