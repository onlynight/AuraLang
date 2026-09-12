//! 内存管理核心（Layer 0，不能上移）。
//!
//! - `malloc` / `free`：带头的原始分配（头内记录大小与引用计数）；
//! - `arc_increment` / `arc_decrement`：原子引用计数；
//! - `string_new` / `string_length` / `string_concat`：字符串基础操作。
//!
//! 三态执行模式共享同一内存语义；AOT 模式下这些函数由 LLVM IR
//! 直接调用对应 C ABI（malloc/free）。

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::Trap;
use super::vm_core::Value;

/// 分配头大小（size + arc）。
pub const HEADER_SIZE: usize = 16;

#[repr(C)]
struct Header {
    size: usize,
    arc: AtomicUsize,
}

/// `malloc(size)`：分配 `size` 字节并写入管理头。
/// 返回**用户区指针**（头部之后）。
pub fn malloc(size: usize) -> Result<*mut u8, Trap> {
    if size == 0 {
        return Err(Trap::new("malloc: size must be greater than 0"));
    }
    let total = size.checked_add(HEADER_SIZE).ok_or_else(|| Trap::new("malloc: size overflow"))?;
    let layout = std::alloc::Layout::from_size_align(total, 16)
        .map_err(|e| Trap::new(format!("malloc: layout error: {e}")))?;
    // SAFETY: layout 大小非零且对齐合法
    let raw = unsafe { std::alloc::alloc(layout) };
    if raw.is_null() {
        return Err(Trap::new("malloc: allocation failed (out of memory)"));
    }
    // SAFETY: raw 指向 total 字节的合法分配
    let head = raw as *mut Header;
    unsafe {
        (*head).size = size;
        (*head).arc = AtomicUsize::new(1);
    }
    // SAFETY: 用户区在头部之后，仍处于分配范围内
    Ok(unsafe { raw.add(HEADER_SIZE) })
}

/// 从用户指针取分配布局。
unsafe fn layout_of(user: *mut u8) -> std::alloc::Layout {
    // SAFETY: 调用方保证 user 来自 malloc
    unsafe {
        let head = user.sub(HEADER_SIZE) as *mut Header;
        let total = (*head).size + HEADER_SIZE;
        std::alloc::Layout::from_size_align(total, 16).unwrap()
    }
}

/// `free(ptr)`：释放 malloc 返回的用户指针。
///
/// # Safety
/// `ptr` 必须是 [`malloc`] 返回且尚未释放的指针。
pub unsafe fn free(ptr: *mut u8) {
    assert!(!ptr.is_null(), "free: null pointer");
    // SAFETY: 调用方保证指针有效且未释放
    unsafe {
        let layout = layout_of(ptr);
        std::alloc::dealloc(ptr.sub(HEADER_SIZE), layout);
    }
}

/// `arc_increment(ptr)`：引用计数 +1，返回新计数。
///
/// # Safety
/// `ptr` 必须是 [`malloc`] 返回的活跃指针。
pub unsafe fn arc_increment(ptr: *mut u8) -> usize {
    // SAFETY: 调用方保证指针有效
    unsafe {
        let head = ptr.sub(HEADER_SIZE) as *mut Header;
        (*head).arc.fetch_add(1, Ordering::AcqRel) + 1
    }
}

/// `arc_decrement(ptr)`：引用计数 -1，返回新计数（不自动释放）。
///
/// # Safety
/// `ptr` 必须是 [`malloc`] 返回的活跃指针。
pub unsafe fn arc_decrement(ptr: *mut u8) -> usize {
    // SAFETY: 调用方保证指针有效
    unsafe {
        let head = ptr.sub(HEADER_SIZE) as *mut Header;
        (*head).arc.fetch_sub(1, Ordering::AcqRel) - 1
    }
}

/// 读取块大小。
///
/// # Safety
/// `ptr` 必须是 [`malloc`] 返回的活跃指针。
pub unsafe fn block_size(ptr: *mut u8) -> usize {
    // SAFETY: 调用方保证指针有效
    unsafe { (*(ptr.sub(HEADER_SIZE) as *mut Header)).size }
}

/// RAII 测试辅助：作用域结束自动 free。
pub struct ManagedPtr(*mut u8);

impl ManagedPtr {
    pub fn new(size: usize) -> Result<Self, Trap> {
        Ok(Self(malloc(size)?))
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.0
    }
}

impl Drop for ManagedPtr {
    fn drop(&mut self) {
        // SAFETY: self.0 来自 malloc 且仅释放一次
        unsafe { free(self.0) }
    }
}

// SAFETY: 指针本身可跨线程传递（指向堆内存）
unsafe impl Send for ManagedPtr {}

// ---------------------------------------------------------------------------
// 字符串操作
// ---------------------------------------------------------------------------

/// `string_new(s)`：构造字符串值。
pub fn string_new(s: &str) -> Value {
    Value::Str(Rc::from(s))
}

/// `string_length(s)`：字符数（非字节数）。
pub fn string_length(v: &Value) -> Result<usize, Trap> {
    Ok(v.as_str()?.chars().count())
}

/// `string_concat(a, b)`：字符串拼接。
pub fn string_concat(a: &Value, b: &Value) -> Result<Value, Trap> {
    let x = a.as_str()?;
    let y = b.as_str()?;
    let mut s = String::with_capacity(x.len() + y.len());
    s.push_str(x);
    s.push_str(y);
    Ok(Value::Str(Rc::from(s.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malloc_free_roundtrip() {
        let p = ManagedPtr::new(64).unwrap();
        assert!(!p.as_ptr().is_null());
        // SAFETY: 测试内自管理
        unsafe {
            assert_eq!(block_size(p.as_ptr()), 64);
            std::ptr::write_bytes(p.as_ptr(), 0xAB, 64);
            assert_eq!(*p.as_ptr(), 0xAB);
        }
    }

    #[test]
    fn arc_counters() {
        let p = ManagedPtr::new(16).unwrap();
        // SAFETY: 测试内自管理
        unsafe {
            assert_eq!(arc_count_of(p.as_ptr()), 1);
            assert_eq!(arc_increment(p.as_ptr()), 2);
            assert_eq!(arc_increment(p.as_ptr()), 3);
            assert_eq!(arc_decrement(p.as_ptr()), 2);
            assert_eq!(arc_decrement(p.as_ptr()), 1);
        }
    }

    // SAFETY: 包装 arc_count 读取
    unsafe fn arc_count_of(p: *mut u8) -> usize {
        // SAFETY: 测试内自管理
        unsafe { (*(p.sub(HEADER_SIZE) as *mut Header)).arc.load(Ordering::Acquire) }
    }

    #[test]
    fn string_ops() {
        let s = string_new("héllo");
        assert_eq!(string_length(&s).unwrap(), 5);
        let t = string_concat(&s, &string_new(" 世界")).unwrap();
        assert_eq!(string_length(&t).unwrap(), 8);
        assert!(matches!(&t, Value::Str(x) if x.as_ref() == "héllo 世界"));
        assert!(string_length(&Value::Int(1)).is_err());
    }

    #[test]
    fn malloc_zero_errors() {
        assert!(malloc(0).is_err());
    }
}
