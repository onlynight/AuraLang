#![cfg(feature = "llvm")]

//! Fix 13 — 扩展泄漏检测测试
//!
//! 验证运行时泄漏检测：分配跟踪、泄漏报告、清理。

use compiler::vm::heap::{Heap, LeakReport};

// ─────────────────────────────────────────────────────────────────────────────
// 1. 分配跟踪
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_heap_alloc_tracking() {
    let mut heap = Heap::new();
    assert_eq!(heap.active_count(), 0);

    let obj = heap.alloc_object(0x1234);
    assert_eq!(heap.active_count(), 1);

    let arr = heap.alloc_array(5);
    assert_eq!(heap.active_count(), 2);

    let list = heap.alloc_list(10);
    assert_eq!(heap.active_count(), 3);

    let map = heap.alloc_map();
    assert_eq!(heap.active_count(), 4);

    // 验证句柄有效
    assert!(obj < heap.active_count());
    assert!(arr < heap.active_count());
    assert!(list < heap.active_count());
    assert!(map < heap.active_count());
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 泄漏报告
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_leak_report_clean() {
    let mut heap = Heap::new();

    // 创建对象并释放
    let obj = heap.alloc_object(0x1234);
    heap.dec_ref(obj);

    // 泄漏报告应为空
    let report = heap.leak_report();
    assert_eq!(report.leaked, 0, "无泄漏时 leaked 应为 0");
}

#[test]
fn test_leak_report_with_leaks() {
    let mut heap = Heap::new();

    // 创建对象但不释放（泄漏）
    heap.alloc_object(0x1111);
    heap.alloc_object(0x2222);
    heap.alloc_array(3);

    let report = heap.leak_report();
    assert_eq!(report.leaked, 3, "3 个未释放对象应为泄漏");
    assert_eq!(report.details.len(), 3);
}

#[test]
fn test_leak_report_details() {
    let mut heap = Heap::new();

    let obj = heap.alloc_object(0xABCD);
    let arr = heap.alloc_array(10);

    let report = heap.leak_report();
    assert_eq!(report.details.len(), 2);

    // 验证详情包含正确的信息
    let has_object = report.details.iter().any(|d| d.data_type.contains("Object"));
    let has_array = report.details.iter().any(|d| d.data_type.contains("Array"));
    assert!(has_object, "应包含 Object 类型");
    assert!(has_array, "应包含 Array 类型");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 清理
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_heap_clear_all() {
    let mut heap = Heap::new();

    // 创建多个对象
    for _ in 0..10 {
        heap.alloc_object(0x1234);
    }
    assert_eq!(heap.active_count(), 10);

    // 清理
    heap.clear_all();
    assert_eq!(heap.active_count(), 0);

    let report = heap.leak_report();
    assert_eq!(report.leaked, 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 混合操作
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_mixed_alloc_free() {
    let mut heap = Heap::new();

    // 创建 5 个对象，释放 2 个
    let mut handles = Vec::new();
    for i in 0..5u16 {
        let h = heap.alloc_object(i);
        handles.push(h);
    }
    assert_eq!(heap.active_count(), 5);

    // 释放前两个
    heap.dec_ref(handles[0]);
    heap.dec_ref(handles[1]);

    // 泄漏报告应显示 3 个泄漏
    let report = heap.leak_report();
    assert_eq!(report.leaked, 3);
    assert_eq!(report.details.len(), 3);
}

#[test]
fn test_leak_report_summary() {
    let mut heap = Heap::new();
    heap.alloc_object(0x1111);
    heap.alloc_object(0x2222);

    let report = heap.leak_report();
    assert!(report.total_allocs >= 2);
    assert!(report.active_allocs >= 2);
    assert!(report.leaked >= 2);
}
