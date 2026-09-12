//! Phase 1c 体积验证测试 — 按需链接验证
//!
//! 验证：
//! 1. 无 import 时注册全部 std 函数（需 std-all feature）
//! 2. 有 import 时只注册 imported 模块的函数
//! 3. 未 import 的 std 代码不编译进二进制

use compiler::codegen::compile_source;
use compiler::vm::native::NativeRegistry;
use compiler::vm::{Vm, VmOptions};

// ─────────────────────────────────────────────────────────────────────────────
// 测试 1: 无 import 时注册全部 std 函数（需 std-all feature）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_full_registry_without_imports() {
    let src = r#"
        fun main(): Int {
            return 42
        }
    "#;
    let module = compile_source(src).expect("compilation should succeed");
    assert!(
        module.enabled_modules.is_empty(),
        "无 import 时 enabled_modules 应为空"
    );

    // 创建 VM 时使用全量注册（向后兼容）
    let vm = Vm::new(&module, VmOptions::default()).expect("VM creation should succeed");
    assert!(
        vm.contains_native("aura.lang.std.Math.sin"),
        "全量注册应包含 aura.lang.std.Math.sin"
    );
    assert!(
        vm.contains_native("aura.lang.std.IO.readLine"),
        "全量注册应包含 aura.lang.std.IO.readLine"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 2: 有 import 时只注册 imported 模块（需 std-all feature）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_partial_registry_with_imports() {
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

    // math 模块应注册
    assert!(
        vm.contains_native("aura.lang.std.Math.sin"),
        "import aura.lang.std.Math.* 后应注册 aura.lang.std.Math.sin"
    );
    assert!(
        vm.contains_native("aura.lang.std.Math.cos"),
        "import aura.lang.std.Math.* 后应注册 aura.lang.std.Math.cos"
    );

    // 未 import 的模块不应注册
    assert!(
        !vm.contains_native("aura.lang.std.IO.readLine"),
        "未 import aura.lang.std.IO 时不应注册 aura.lang.std.IO.readLine"
    );
    assert!(
        !vm.contains_native("aura.lang.std.String.contains"),
        "未 import aura.lang.std.String 时不应注册 aura.lang.std.String.contains"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 3: 多个 import 时只注册指定的模块（需 std-all）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_multiple_imports_only_registers_specified() {
    let src = r#"
        import aura.lang.std.Math.*
        import aura.lang.std.String.*
        fun main(): String {
            return toStr(sqrt(16.0))
        }
    "#;
    let module = compile_source(src).expect("compilation should succeed");
    assert!(
        module.enabled_modules.contains(&"math".to_string()),
        "enabled_modules 应包含 'math'"
    );
    assert!(
        module.enabled_modules.contains(&"string".to_string()),
        "enabled_modules should contain 'string'"
    );
    assert_eq!(
        module.enabled_modules.len(),
        2,
        "should only contain 2 modules"
    );

    let vm = Vm::new(&module, VmOptions::default()).expect("VM creation should succeed");

    // 两个模块都应注册
    assert!(vm.contains_native("aura.lang.std.Math.sin"));
    assert!(vm.contains_native("aura.lang.std.String.contains"));

    // 其他模块不应注册
    assert!(
        !vm.contains_native("aura.lang.std.IO.readLine"),
        "未 import aura.lang.std.IO 时不应注册"
    );
    assert!(
        !vm.contains_native("aura.lang.std.Network.connect"),
        "未 import aura.lang.std.Network 时不应注册"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 4: Prelude 始终注册（无论有无 import）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_prelude_always_registered() {
    // 无 import
    let src1 = r#"
        fun main(): Int {
            return abs(-5)
        }
    "#;
    let module1 = compile_source(src1).expect("compilation should succeed");
    let vm1 = Vm::new(&module1, VmOptions::default()).expect("VM creation should succeed");
    assert!(
        vm1.contains_native("abs"),
        "prelu 'abs' should always be registered"
    );

    // 有 import
    let src2 = r#"
        import aura.lang.std.Math.*
        fun main(): Float {
            return sin(1.0)
        }
    "#;
    let module2 = compile_source(src2).expect("compilation should succeed");
    let vm2 = Vm::new(&module2, VmOptions::default()).expect("VM creation should succeed");
    assert!(
        vm2.contains_native("abs"),
        "prelu 'abs' should always be registered (even with import)"
    );
    assert!(
        vm2.contains_native("println"),
        "prelu 'println' should always be registered"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 5: NativeRegistry::with_modules 直接验证（需 std-all）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_native_registry_with_modules_direct() {
    // 空模块：只注册 prelude
    let reg_empty = NativeRegistry::with_modules(&[]);
    assert!(reg_empty.contains("println"));
    assert!(reg_empty.contains("abs"));
    assert!(!reg_empty.contains("aura.lang.std.Math.sin"));

    // 只注册 math
    let reg_math = NativeRegistry::with_modules(&["math"]);
    assert!(reg_math.contains("aura.lang.std.Math.sin"));
    assert!(reg_math.contains("aura.lang.std.Math.cos"));
    assert!(!reg_math.contains("aura.lang.std.IO.readLine"));

    // 注册 math + io
    let reg_both = NativeRegistry::with_modules(&[
        "math", "io",
    ]);
    assert!(reg_both.contains("aura.lang.std.Math.sin"));
    assert!(reg_both.contains("aura.lang.std.IO.readLine"));
    assert!(!reg_both.contains("aura.lang.std.String.contains"));
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 6: 并发模块按需注册（需 std-concurrent）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-concurrent")]
fn test_concurrent_module_on_demand() {
    // 无 import 时并发函数不注册（使用 with_modules）
    let reg_no_concurrent = NativeRegistry::with_modules(&["math"]);
    assert!(
        !reg_no_concurrent.contains("aura.lang.std.Coroutine.spawn"),
        "未 import aura.lang.std.Coroutine 时不应注册并发函数"
    );

    // 有 import 时并发函数注册
    let reg_with_concurrent = NativeRegistry::with_modules(&["concurrent"]);
    assert!(
        reg_with_concurrent.contains("aura.lang.std.Coroutine.spawn"),
        "import aura.lang.std.Coroutine 后应注册并发函数"
    );
}
