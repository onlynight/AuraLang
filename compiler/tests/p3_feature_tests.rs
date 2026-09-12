//! Phase 3 Feature 验证测试
//!
//! 验证 Cargo feature flags 正确门控 std 模块编译。

use compiler::codegen::compile_source;
use compiler::vm::native::NativeRegistry;
use compiler::vm::{Vm, VmOptions};

// ─────────────────────────────────────────────────────────────────────────────
// 测试 1: 默认构建（含 std-concurrent）应注册并发函数
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-concurrent")]
fn test_concurrent_registered_with_default_features() {
    let reg = NativeRegistry::new();
    assert!(
        reg.contains("aura.lang.std.Coroutine.spawn"),
        "默认 features 应包含并发函数"
    );
    assert!(
        reg.contains("aura.lang.std.Channel.channelSend"),
        "默认 features 应包含并发函数"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 2: 无 features 构建不应注册并发函数
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(not(feature = "std-concurrent"))]
fn test_concurrent_not_registered_without_features() {
    let reg = NativeRegistry::new();
    assert!(
        !reg.contains("aura.lang.std.Coroutine.spawn"),
        "无 std-concurrent feature 时不应注册并发函数"
    );
    assert!(
        !reg.contains("aura.lang.std.Channel.channelSend"),
        "无 std-concurrent feature 时不应注册并发函数"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 3: std-math feature 应注册 math 模块
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-math")]
fn test_math_registered_with_feature() {
    let reg = NativeRegistry::new();
    assert!(
        reg.contains("aura.lang.std.Math.sin"),
        "std-math feature 应注册 aura.lang.std.Math.sin"
    );
    assert!(
        reg.contains("aura.lang.std.Math.cos"),
        "std-math feature 应注册 aura.lang.std.Math.cos"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 4: 无 std-math feature 不应注册 math 模块
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(not(feature = "std-math"))]
fn test_math_not_registered_without_feature() {
    let reg = NativeRegistry::new();
    assert!(
        !reg.contains("aura.lang.std.Math.sin"),
        "无 std-math feature 时不应注册 aura.lang.std.Math.sin"
    );
    assert!(
        !reg.contains("aura.lang.std.Math.cos"),
        "无 std-math feature 时不应注册 aura.lang.std.Math.cos"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 5: std-io feature 应注册 io 模块
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-io")]
fn test_io_registered_with_feature() {
    let reg = NativeRegistry::new();
    assert!(
        reg.contains("aura.lang.std.IO.readLine"),
        "std-io feature 应注册 aura.lang.std.IO.readLine"
    );
    assert!(
        reg.contains("aura.lang.std.IO.fileExists"),
        "std-io feature 应注册 aura.lang.std.IO.fileExists"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 6: Prelude 始终注册（无论有无 features）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_prelude_always_registered() {
    let reg = NativeRegistry::new();
    assert!(
        reg.contains("println"),
        "prelu 'println' should always be registered"
    );
    assert!(
        reg.contains("abs"),
        "prelu 'abs' should always be registered"
    );
    assert!(
        reg.contains("sqrt"),
        "prelu 'sqrt' should always be registered"
    );
    assert!(
        reg.contains("toInt"),
        "prelu 'toInt' should always be registered"
    );
    assert!(
        reg.contains("toFloat"),
        "prelu 'toFloat' should always be registered"
    );
    assert!(
        reg.contains("toStr"),
        "prelu 'toStr' should always be registered"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 7: 按需注册（import 驱动）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_on_demand_registration() {
    // 有 import 时只注册 imported 模块
    let src = r#"
        import aura.lang.std.Math.*
        fun main(): Float {
            return sin(1.0)
        }
    "#;
    let module = compile_source(src).expect("compilation should succeed");
    assert!(
        module.enabled_modules.contains(&"math".to_string()),
        "enabled_modules 应包含 'math'"
    );

    // 创建 VM 时使用按需注册
    let vm = Vm::new(&module, VmOptions::default()).expect("VM creation should succeed");

    // 如果 std-math feature 启用，math 模块应注册
    #[cfg(feature = "std-math")]
    {
        assert!(
            vm.contains_native("aura.lang.std.Math.sin"),
            "import aura.lang.std.Math.* 后应注册 aura.lang.std.Math.sin"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 8: 体积对比（粗略验证）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_full_registry_size() {
    let reg = NativeRegistry::new();
    // 全量注册应有 300+ 个函数
    let count = reg.len();
    assert!(
        count > 300,
        "full registration should have 300+ functions, actual: {}",
        count
    );
}

#[test]
#[cfg(not(feature = "std-all"))]
fn test_reduced_registry_size() {
    let reg = NativeRegistry::new();
    // 无 std-all 时函数数量应明显减少
    let count = reg.len();
    // prelu (17) + 可能的一些默认模块
    assert!(
        count < 50,
        "无 std-all 时函数数量应明显减少，实际: {}",
        count
    );
}
