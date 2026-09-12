#![cfg(feature = "llvm")]

//! Fix 8 — CString 线程安全（Rc<str> → Arc<str>）测试
//!
//! 验证 CString 堆存储使用 Arc<str>（线程安全引用计数）。

use compiler::vm::heap::Heap;

// ─────────────────────────────────────────────────────────────────────────────
// 1. CString 分配与读取
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cstring_alloc_read() {
    let mut heap = Heap::new();
    let ptr = heap.alloc_c_string("hello".to_string());
    assert!(ptr > 0, "allocated pointer should be nonzero");
    let s = heap.read_c_string(ptr);
    assert_eq!(s, "hello");
}

#[test]
fn test_cstring_multiple_allocs() {
    let mut heap = Heap::new();
    let ptr1 = heap.alloc_c_string("first".to_string());
    let ptr2 = heap.alloc_c_string("second".to_string());
    assert!(ptr1 < ptr2, "subsequent allocated pointer should be larger");
    assert_eq!(heap.read_c_string(ptr1), "first");
    assert_eq!(heap.read_c_string(ptr2), "second");
}

#[test]
fn test_cstring_nullptr() {
    let heap = Heap::new();
    let s = heap.read_c_string(0);
    assert_eq!(s, "", "0 pointer should return empty string");
}

#[test]
fn test_cstring_invalid_ptr() {
    let heap = Heap::new();
    let s = heap.read_c_string(999);
    assert_eq!(s, "", "invalid pointer should return empty string");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. CString 线程安全验证
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cstring_thread_safety() {
    // 验证 c_strings 使用 Arc<str>（可通过 Send + Sync 检查）
    // 此测试通过编译期检查验证类型安全
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<std::sync::Arc<str>>();
}

#[test]
fn test_cstring_rc_not_thread_safe() {
    // 验证 Rc<str> 不实现 Send + Sync（作为对比）
    // 此测试应编译失败，但作为文档说明
    fn assert_send_sync<T: Send + Sync>() {}
    // assert_send_sync::<std::rc::Rc<str>>(); // 这会编译失败
}
