#![cfg(feature = "llvm")]

//! Fix 9 — 默认按需加载 std 测试
//!
//! 验证 NativeRegistry::new() 仅加载 prelude，不加载全部 std 模块。

use compiler::vm::native::NativeRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// 1. new() 仅加载 prelude
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_new_only_loads_prelude() {
    let registry = NativeRegistry::new();
    let count = registry.len();
    // prelude 有 18 个函数；并发模块（默认启用）增加 11 个
    #[cfg(feature = "std-concurrent")]
    let expected_min = 29; // 18 + 11
    #[cfg(not(feature = "std-concurrent"))]
    let expected_min = 18;
    assert!(
        count >= expected_min,
        "new() 应至少加载 {} 个函数，实际 {}",
        expected_min,
        count
    );
    assert!(
        count < 100,
        "new() 不应加载全部 std 模块（396 个），实际 {}",
        count
    );
}

#[test]
fn test_new_has_prelude_functions() {
    let registry = NativeRegistry::new();
    // 验证 prelude 函数存在
    assert!(registry.contains("println"));
    assert!(registry.contains("print"));
    assert!(registry.contains("abs"));
    assert!(registry.contains("sqrt"));
    assert!(registry.contains("CString"));
    assert!(registry.contains("makeCallback"));
}

#[test]
fn test_new_no_std_functions() {
    let registry = NativeRegistry::new();
    // 验证 std 模块函数不存在
    assert!(!registry.contains("aura.math.PI"));
    assert!(!registry.contains("aura.io.readFile"));
    assert!(!registry.contains("aura.fs.exists"));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. with_modules() 加载指定模块
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "std-math")]
#[test]
fn test_with_modules_loads_math() {
    let registry = NativeRegistry::with_modules(&["math"]);
    let count = registry.len();
    // prelude + math 模块（应大于空列表）
    let empty_registry = NativeRegistry::with_modules(&[]);
    assert!(
        count > empty_registry.len(),
        "with_modules(&[\"math\"]) 应加载额外模块"
    );
    assert!(registry.contains("aura.math.PI"));
}

#[cfg(not(feature = "std-math"))]
#[test]
fn test_with_modules_loads_math_skipped() {
    // std-math feature 未启用，跳过测试
}

#[test]
fn test_with_modules_empty() {
    let registry = NativeRegistry::with_modules(&[]);
    let count = registry.len();
    // prelude (18) + 并发（11，默认启用，但 with_modules 不加载并发）
    assert_eq!(count, 18, "空模块列表应仅加载 18 个 prelude 函数");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 并发模块（条件编译）
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "std-concurrent")]
#[test]
fn test_concurrent_functions_loaded() {
    let registry = NativeRegistry::new();
    assert!(registry.contains("aura.concurrent.spawn"));
    assert!(registry.contains("aura.concurrent.newChannel"));
}
