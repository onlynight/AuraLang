#![cfg(feature = "llvm")]

//! Fix 10 — 多线程运行时测试
//!
//! 验证 ThreadPool 基础功能：创建、执行任务、关闭。

use compiler::vm::thread_pool::{AtomicCounter, ThreadPool};
use std::sync::Arc;
use std::time::Duration;
use std::thread;

// ─────────────────────────────────────────────────────────────────────────────
// 1. 线程池创建
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_pool_new() {
    let mut pool = ThreadPool::new(4);
    assert_eq!(pool.size(), 4);
    pool.shutdown();
}

#[test]
fn test_thread_pool_default() {
    let mut pool = ThreadPool::default_pool();
    assert!(pool.size() > 0, "默认线程池大小应大于 0");
    pool.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 任务执行
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_pool_execute() {
    let mut pool = ThreadPool::new(2);
    let counter = Arc::new(AtomicCounter::new());

    // 提交 10 个任务
    for _ in 0..10 {
        let c = counter.clone();
        pool.execute(move || {
            c.increment();
        });
    }

    // 等待任务完成（简单方式：短暂休眠后检查）
    thread::sleep(Duration::from_millis(100));
    assert_eq!(counter.value(), 10, "应执行 10 个任务");

    pool.shutdown();
}

#[test]
fn test_thread_pool_concurrent_execution() {
    let mut pool = ThreadPool::new(4);
    let counter = Arc::new(AtomicCounter::new());

    // 提交 100 个任务到 4 个线程
    for _ in 0..100 {
        let c = counter.clone();
        pool.execute(move || {
            c.increment();
        });
    }

    thread::sleep(Duration::from_millis(200));
    assert_eq!(counter.value(), 100, "应执行 100 个任务（线程安全）");

    pool.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 线程池关闭
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_pool_shutdown() {
    let mut pool = ThreadPool::new(2);
    pool.shutdown();
    // shutdown 后不应 panic
}

#[test]
fn test_thread_pool_drop() {
    // Drop 不应 panic
    let _pool = ThreadPool::new(2);
    drop(_pool);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 并发安全
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_thread_pool_thread_safe() {
    let mut pool = ThreadPool::new(4);
    let counter = Arc::new(AtomicCounter::new());

    // 提交 100 个任务（4 线程 × 25 任务）
    for _ in 0..100 {
        let c = counter.clone();
        pool.execute(move || {
            c.increment();
        });
    }

    thread::sleep(Duration::from_millis(200));
    assert_eq!(counter.value(), 100, "应执行 100 个任务（4 线程 × 25 任务）");

    pool.shutdown();
}
