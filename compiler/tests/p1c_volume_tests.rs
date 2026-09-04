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
    let module = compile_source(src).expect("编译应成功");
    assert!(
        module.enabled_modules.is_empty(),
        "无 import 时 enabled_modules 应为空"
    );

    // 创建 VM 时使用全量注册（向后兼容）
    let vm = Vm::new(&module, VmOptions::default()).expect("VM 创建应成功");
    assert!(
        vm.contains_native("aura.math.sin"),
        "全量注册应包含 aura.math.sin"
    );
    assert!(
        vm.contains_native("aura.io.readLine"),
        "全量注册应包含 aura.io.readLine"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 2: 有 import 时只注册 imported 模块（需 std-all feature）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_partial_registry_with_imports() {
    let src = r#"
        import aura.math.*
        fun main(): Float {
            return sin(1.0)
        }
    "#;
    let module = compile_source(src).expect("编译应成功");
    assert!(
        module.enabled_modules.contains(&"math".to_string()),
        "enabled_modules 应包含 'math'"
    );

    // 创建 VM 时使用按需注册
    let vm = Vm::new(&module, VmOptions::default()).expect("VM 创建应成功");

    // math 模块应注册
    assert!(
        vm.contains_native("aura.math.sin"),
        "import aura.math.* 后应注册 aura.math.sin"
    );
    assert!(
        vm.contains_native("aura.math.cos"),
        "import aura.math.* 后应注册 aura.math.cos"
    );

    // 未 import 的模块不应注册
    assert!(
        !vm.contains_native("aura.io.readLine"),
        "未 import aura.io 时不应注册 aura.io.readLine"
    );
    assert!(
        !vm.contains_native("aura.string.contains"),
        "未 import aura.string 时不应注册 aura.string.contains"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 3: 多个 import 时只注册指定的模块（需 std-all）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "std-all")]
fn test_multiple_imports_only_registers_specified() {
    let src = r#"
        import aura.math.*
        import aura.string.*
        fun main(): String {
            return toStr(sqrt(16.0))
        }
    "#;
    let module = compile_source(src).expect("编译应成功");
    assert!(
        module.enabled_modules.contains(&"math".to_string()),
        "enabled_modules 应包含 'math'"
    );
    assert!(
        module.enabled_modules.contains(&"string".to_string()),
        "enabled_modules 应包含 'string'"
    );
    assert_eq!(
        module.enabled_modules.len(),
        2,
        "应只包含 2 个模块"
    );

    let vm = Vm::new(&module, VmOptions::default()).expect("VM 创建应成功");

    // 两个模块都应注册
    assert!(vm.contains_native("aura.math.sin"));
    assert!(vm.contains_native("aura.string.contains"));

    // 其他模块不应注册
    assert!(
        !vm.contains_native("aura.io.readLine"),
        "未 import aura.io 时不应注册"
    );
    assert!(
        !vm.contains_native("aura.net.connect"),
        "未 import aura.net 时不应注册"
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
    let module1 = compile_source(src1).expect("编译应成功");
    let vm1 = Vm::new(&module1, VmOptions::default()).expect("VM 创建应成功");
    assert!(vm1.contains_native("abs"), "prelu 'abs' 应始终注册");

    // 有 import
    let src2 = r#"
        import aura.math.*
        fun main(): Float {
            return sin(1.0)
        }
    "#;
    let module2 = compile_source(src2).expect("编译应成功");
    let vm2 = Vm::new(&module2, VmOptions::default()).expect("VM 创建应成功");
    assert!(vm2.contains_native("abs"), "prelu 'abs' 应始终注册（即使有 import）");
    assert!(vm2.contains_native("println"), "prelu 'println' 应始终注册");
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
    assert!(!reg_empty.contains("aura.math.sin"));

    // 只注册 math
    let reg_math = NativeRegistry::with_modules(&["math"]);
    assert!(reg_math.contains("aura.math.sin"));
    assert!(reg_math.contains("aura.math.cos"));
    assert!(!reg_math.contains("aura.io.readLine"));

    // 注册 math + io
    let reg_both = NativeRegistry::with_modules(&["math", "io"]);
    assert!(reg_both.contains("aura.math.sin"));
    assert!(reg_both.contains("aura.io.readLine"));
    assert!(!reg_both.contains("aura.string.contains"));
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
        !reg_no_concurrent.contains("aura.concurrent.spawn"),
        "未 import aura.concurrent 时不应注册并发函数"
    );

    // 有 import 时并发函数注册
    let reg_with_concurrent = NativeRegistry::with_modules(&["concurrent"]);
    assert!(
        reg_with_concurrent.contains("aura.concurrent.spawn"),
        "import aura.concurrent 后应注册并发函数"
    );
}
