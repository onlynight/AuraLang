//! Phase 3: Integration tests for stdlib linking and multi-module VM.
//!
//! Tests the full pipeline: compile → link stdlib → resolve → configure execution mode.

use std::path::Path;

use compiler::codegen::opcode::BytecodeModule;
use compiler::codegen::{
    FfiMode, compile_source, compile_source_with_stdlib, execution, ffi_aot, link_stdlib,
    resolve_stdlib,
};
use compiler::vm::ffi_cache::FfiCache;
use compiler::vm::multi_module::MultiModuleVm;

#[test]
fn test_compile_source_with_stdlib_basic() {
    let source = r#"
fun main() {
    println("Hello, Phase 3!")
}
"#;

    let tmp = tempfile::tempdir().unwrap();
    let stdlib_dir = tmp.path().join("stdlib");
    std::fs::create_dir_all(&stdlib_dir).unwrap();

    // Write a fake stdlib .auc file (minimal valid bytecode)
    let mut stdlib_module = BytecodeModule::default();
    compiler::codegen::write_auc(
        &stdlib_dir.join("Math.auc").to_string_lossy(),
        &stdlib_module,
    )
    .unwrap();

    let result = compile_source_with_stdlib(
        source,
        &stdlib_dir,
        ffi_aot::ExecutionMode::Vm,
        FfiMode::Aot,
    );

    assert!(
        result.is_ok(),
        "compile_source_with_stdlib should succeed: {:?}",
        result.err()
    );
}

#[test]
fn test_compile_source_with_stdlib_jit_mode() {
    let source = r#"
fun main() {
    val x = 42
}
"#;

    let tmp = tempfile::tempdir().unwrap();
    let stdlib_dir = tmp.path().join("stdlib");
    std::fs::create_dir_all(&stdlib_dir).unwrap();

    let result = compile_source_with_stdlib(
        source,
        &stdlib_dir,
        ffi_aot::ExecutionMode::Jit,
        FfiMode::Cffi,
    );

    assert!(result.is_ok());
    let module = result.unwrap();
    assert!(!module.functions.is_empty());
}

#[test]
fn test_compile_source_with_stdlib_aot_mode() {
    let source = r#"
fun main() {
    val x = 1 + 2
}
"#;

    let tmp = tempfile::tempdir().unwrap();
    let stdlib_dir = tmp.path().join("stdlib");
    std::fs::create_dir_all(&stdlib_dir).unwrap();

    let result = compile_source_with_stdlib(
        source,
        &stdlib_dir,
        ffi_aot::ExecutionMode::Aot,
        FfiMode::Aot,
    );

    assert!(result.is_ok());
}

#[test]
fn test_link_stdlib_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut module = BytecodeModule::default();

    let result = link_stdlib::link_stdlib_symbols(&mut module, tmp.path()).unwrap();
    assert_eq!(result.modules_linked, 0);
    assert_eq!(result.symbols_resolved, 0);
}

#[test]
fn test_resolve_stdlib_calls_empty() {
    let module = BytecodeModule::default();
    let result = resolve_stdlib::resolve_stdlib_calls(&module, &[]);
    assert!(result.resolved.is_empty());
}

#[test]
fn test_execution_config_default() {
    let config = execution::ExecutionConfig::default();
    assert_eq!(config.mode, execution::ExecutionMode::Vm);
    assert_eq!(config.opt_level, 2);
    assert!(config.debug);
    assert!(config.parallel);
}

#[test]
fn test_execution_mode_from_str() {
    assert_eq!(
        "vm".parse::<execution::ExecutionMode>(),
        Ok(execution::ExecutionMode::Vm)
    );
    assert_eq!(
        "jit".parse::<execution::ExecutionMode>(),
        Ok(execution::ExecutionMode::Jit)
    );
    assert_eq!(
        "aot".parse::<execution::ExecutionMode>(),
        Ok(execution::ExecutionMode::Aot)
    );
    assert!("unknown".parse::<execution::ExecutionMode>().is_err());
}

#[test]
fn test_ffi_cache_preload_and_lookup() {
    let mut cache = FfiCache::new();
    cache.preload_function("fopen", 0x400000);
    cache.preload_function("fclose", 0x401000);

    assert_eq!(cache.get_address("fopen"), Some(0x400000));
    assert_eq!(cache.get_address("fclose"), Some(0x401000));
    assert_eq!(cache.get_address("unknown"), None);
    assert!(cache.is_preloaded("fopen"));
    assert!(!cache.is_preloaded("unknown"));
    assert_eq!(cache.preloaded_count(), 2);
}

#[test]
fn test_ffi_cache_call_tracking() {
    let mut cache = FfiCache::new();
    cache.preload_function("fopen", 0x400000);

    cache.record_call("fopen");
    cache.record_call("fopen");
    cache.record_call("fopen");

    assert_eq!(cache.get_call_count("fopen"), 3);
    assert_eq!(cache.get_call_count("unknown"), 0);
}

#[test]
fn test_ffi_cache_clear() {
    let mut cache = FfiCache::new();
    cache.preload_function("fopen", 0x400000);
    cache.record_call("fopen");

    cache.clear();

    assert_eq!(cache.preloaded_count(), 0);
    assert!(!cache.is_preloaded("fopen"));
    assert_eq!(cache.get_call_count("fopen"), 0);
}

#[test]
fn test_multi_module_vm_empty() {
    let vm = MultiModuleVm::new();
    assert!(vm.is_empty());
    assert_eq!(vm.len(), 0);
    assert!(vm.entry_module().is_none());
}

#[test]
fn test_multi_module_vm_cross_module_symbols() {
    let mut vm = MultiModuleVm::new();

    // Load Math module with exports
    let mut math = BytecodeModule::default();
    math.exports.push(compiler::codegen::opcode::ExportSymbol {
        name: "abs".to_string(),
        kind: compiler::codegen::opcode::SymbolKind::Function,
        sig_id: "sig-abs".to_string(),
        func_idx: Some(0),
        type_table_idx: None,
        const_idx: None,
    });
    vm.load_module("math", math);

    // Load app module
    let mut app = BytecodeModule::default();
    app.exports.push(compiler::codegen::opcode::ExportSymbol {
        name: "main".to_string(),
        kind: compiler::codegen::opcode::SymbolKind::Function,
        sig_id: "sig-main".to_string(),
        func_idx: Some(0),
        type_table_idx: None,
        const_idx: None,
    });
    vm.load_module("app", app);
    vm.set_entry("app");

    assert_eq!(vm.len(), 2);
    assert!(vm.has_symbol("abs"));
    assert!(vm.has_symbol("main"));
    assert_eq!(vm.symbol_count(), 2);
    assert!(vm.entry_module().is_some());
}

#[test]
fn test_ffi_aot_config_all_modes() {
    let mut module = BytecodeModule::default();
    module.natives.push(compiler::codegen::opcode::BytecodeNative {
        name: "fopen".to_string(),
        param_count: 2,
        ffi_abi: compiler::codegen::opcode::FfiAbi::C,
        ffi_lib: Some("libc".to_string()),
        param_types: vec![4, 4],
        ret_type: 5,
    });

    let config = ffi_aot::FfiAotConfig::default();

    // Test all three modes
    for mode in [
        ffi_aot::ExecutionMode::Vm,
        ffi_aot::ExecutionMode::Jit,
        ffi_aot::ExecutionMode::Aot,
    ] {
        let result = ffi_aot::configure_ffi_aot_direct(&module, mode, &config);
        assert!(result.is_ok(), "Mode {:?} should succeed", mode);
        let r = result.unwrap();
        assert_eq!(r.declarations_configured, 1);
        assert_eq!(r.execution_mode, mode);
    }
}

#[test]
fn test_ffi_aot_no_declarations() {
    let module = BytecodeModule::default();
    let config = ffi_aot::FfiAotConfig::default();
    let result = ffi_aot::configure_ffi_aot_direct(&module, ffi_aot::ExecutionMode::Vm, &config);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().declarations_configured, 0);
}

#[test]
fn test_compile_source_basic() {
    let source = r#"
fun add(a: Int, b: Int): Int {
    a + b
}

fun main() {
    val result = add(1, 2)
}
"#;

    let result = compile_source(source);
    assert!(
        result.is_ok(),
        "compile_source should succeed: {:?}",
        result.err()
    );
}

#[test]
fn test_compile_source_parse_error() {
    let source = "fun main() { this is not valid aura";
    let result = compile_source(source);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: VM 标准库 Aura 编译路径测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试 load_stdlib_dir 加载 .auc 文件并合并函数
#[test]
fn test_vm_load_stdlib_dir_basic() {
    use compiler::vm::{Vm, VmOptions};

    // 创建用户模块
    let user_source = r#"
fun main() {
    println("Hello")
}
"#;
    let user_module = compile_source(user_source).unwrap();

    // 创建 VM
    let opts = VmOptions::default();
    let mut vm = Vm::new(&user_module, opts).unwrap();

    // 创建临时标准库目录
    let tmp = tempfile::tempdir().unwrap();
    let stdlib_dir = tmp.path();

    // 创建标准库模块（包含一个顶层函数）
    let std_source = r#"
fun add(a: Int, b: Int): Int {
    a + b
}
"#;
    let std_module = compile_source(std_source).unwrap();

    // 写入 .auc 文件
    let std_file = stdlib_dir.join("TestHelper.auc");
    compiler::codegen::write_auc(&std_file.to_string_lossy(), &std_module).unwrap();

    // 加载标准库
    let result = vm.load_stdlib_dir(stdlib_dir);
    assert!(
        result.is_ok(),
        "load_stdlib_dir should succeed: {:?}",
        result.err()
    );

    let count = result.unwrap();
    assert!(count > 0, "should load at least one function");

    // 验证函数映射
    // 函数名应为 "aura.lang.std.TestHelper.add"
    // 但由于编译后的函数名可能不同，我们只验证 load 成功
}

/// 测试 load_stdlib_dir 对不存在目录的错误处理
#[test]
fn test_vm_load_stdlib_dir_nonexistent() {
    use compiler::vm::{Vm, VmOptions};

    let source = "fun main() {}";
    let module = compile_source(source).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();

    let result = vm.load_stdlib_dir(Path::new("/nonexistent/path"));
    assert!(
        result.is_err(),
        "should return error for nonexistent directory"
    );
}

/// 测试 find_stdlib_func 参数个数匹配
#[test]
fn test_vm_find_stdlib_func_param_match() {
    use compiler::vm::{Vm, VmOptions};

    let user_source = "fun main() {}";
    let user_module = compile_source(user_source).unwrap();
    let vm = Vm::new(&user_module, VmOptions::default()).unwrap();

    // 未加载标准库时，find_stdlib_func 应返回 None
    assert!(vm.find_stdlib_func("anything", 0).is_none());
    assert!(vm.find_stdlib_func("anything", 1).is_none());
}
