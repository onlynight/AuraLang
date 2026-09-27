//! Phase C — AOT 并发代码生成测试
//!
//! 验证 AOT 后端能为并发原生函数生成正确的 LLVM IR 声明。
//! 通过 inject_concurrent_natives 注入的并发函数声明在 AOT 编译时
//! 应被识别为 native 函数并生成 extern declare。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OutputFormat};
use compiler::codegen::hir::HirProgram;
use compiler::codegen::hir::{HirBlock, HirFunction, HirParam, HirType};
use compiler::codegen::opcode::FfiAbi;

// ─────────────────────────────────────────────────────────────────────────────
// 辅助：构建最小 HIR 程序
// ─────────────────────────────────────────────────────────────────────────────

fn make_empty_program() -> HirProgram {
    HirProgram {
        functions: Vec::new(),
        structs: Vec::new(),
        enums: Vec::new(),
        natives: Vec::new(),
        constants: Vec::new(),
        top_level_statements: None,
        type_aliases: Default::default(),
    }
}

fn make_native(name: &str, params: &[(&str, HirType)], ret: Option<HirType>) -> HirFunction {
    HirFunction {
        name: name.to_string(),
        params: params
            .iter()
            .map(|(n, t)| HirParam {
                name: n.to_string(),
                ty: Some(t.clone()),
                default_value: None,
                is_vararg: false,
            })
            .collect(),
        ret,
        body: HirBlock {
            stmts: Vec::new(),
        },
        is_native: true,
        type_params: Vec::new(),
        ffi_abi: FfiAbi::C,
        ffi_lib: None,
        native_attr: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. 并发原生函数注入测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_concurrent_natives_injection() {
    let mut program = make_empty_program();

    // 模拟 AOT 编译前的注入
    let before_count = program.natives.len();
    assert_eq!(before_count, 0, "empty program should have no natives");

    // 手动注入几个并发函数（模拟 inject_concurrent_natives 的行为）
    let i64_t = HirType::Named("Int".to_string());
    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.new",
        &[],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.lock",
        &[("id", i64_t.clone())],
        Some(HirType::Named("Unit".to_string())),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Atomic.new",
        &[("initial", i64_t.clone())],
        Some(i64_t.clone()),
    ));

    assert_eq!(program.natives.len(), 3);

    // 验证 native 函数声明正确
    let mutex_new = &program.natives[0];
    assert_eq!(mutex_new.name, "aura.lang.concurrent.Mutex.new");
    assert!(mutex_new.is_native);
    assert_eq!(mutex_new.params.len(), 0);
    assert!(mutex_new.ret.is_some());

    let mutex_lock = &program.natives[1];
    assert_eq!(mutex_lock.name, "aura.lang.concurrent.Mutex.lock");
    assert!(mutex_lock.is_native);
    assert_eq!(mutex_lock.params.len(), 1);
    assert_eq!(mutex_lock.params[0].name, "id");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. AOT LLVM IR 生成测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aot_generates_concurrent_ffi_declarations() {
    let mut program = make_empty_program();

    // 注入并发函数声明
    let i64_t = HirType::Named("Int".to_string());
    let void_t = HirType::Named("Unit".to_string());
    let bool_t = HirType::Named("Boolean".to_string());

    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.new",
        &[],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.lock",
        &[("id", i64_t.clone())],
        Some(void_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.unlock",
        &[("id", i64_t.clone())],
        Some(void_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Mutex.tryLock",
        &[("id", i64_t.clone())],
        Some(bool_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Atomic.new",
        &[("initial", i64_t.clone())],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Atomic.load",
        &[("id", i64_t.clone())],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Atomic.add",
        &[
            ("id", i64_t.clone()),
            ("delta", i64_t.clone()),
        ],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Atomic.cas",
        &[
            ("id", i64_t.clone()),
            ("expected", i64_t.clone()),
            ("desired", i64_t.clone()),
        ],
        Some(bool_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Thread.sleep",
        &[("ms", i64_t.clone())],
        Some(void_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Thread.id",
        &[],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.Thread.parallelism",
        &[],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.RwLock.new",
        &[],
        Some(i64_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.RwLock.readLock",
        &[("id", i64_t.clone())],
        Some(void_t.clone()),
    ));
    program.natives.push(make_native(
        "aura.lang.concurrent.RwLock.writeLock",
        &[("id", i64_t.clone())],
        Some(void_t.clone()),
    ));

    // 生成 LLVM IR
    let options = AotOptions::default();
    let codegen = AotCodeGenerator::new(options);
    let output_dir = std::env::temp_dir().join("aura_aot_concurrent_test");
    std::fs::create_dir_all(&output_dir).unwrap();

    let result = codegen.compile(&program, &output_dir, OutputFormat::LlvmIr);
    assert!(
        result.is_ok(),
        "AOT compilation should succeed: {:?}",
        result.err()
    );

    let output = result.unwrap();
    let ir_text = output.ir_text.clone();

    // 验证 LLVM IR 中包含并发函数的 extern declare
    // sanitizellvm("aura.lang.concurrent.Mutex.new") → "aura_lang_std_Mutex_new"
    // translate_to_legacy_c → "aura_mutex_new"
    assert!(
        ir_text.contains("aura_mutex_new"),
        "LLVM IR should contain aura_mutex_new declaration"
    );
    assert!(
        ir_text.contains("aura_mutex_lock"),
        "LLVM IR should contain aura_mutex_lock declaration"
    );
    assert!(
        ir_text.contains("aura_mutex_unlock"),
        "LLVM IR should contain aura_mutex_unlock declaration"
    );
    assert!(
        ir_text.contains("aura_mutex_trylock"),
        "LLVM IR should contain aura_mutex_trylock declaration"
    );
    assert!(
        ir_text.contains("aura_atomic_new") || ir_text.contains("aura_atomic_load"),
        "LLVM IR should contain aura_atomic_* declarations"
    );
    assert!(
        ir_text.contains("aura_thread_sleep"),
        "LLVM IR should contain aura_thread_sleep declaration"
    );
    assert!(
        ir_text.contains("aura_thread_id"),
        "LLVM IR should contain aura_thread_id declaration"
    );
    assert!(
        ir_text.contains("aura_thread_available_parallelism"),
        "LLVM IR should contain aura_thread_available_parallelism declaration"
    );
    assert!(
        ir_text.contains("aura_rwlock_new"),
        "LLVM IR should contain aura_rwlock_new declaration"
    );

    // 清理
    let _ = std::fs::remove_dir_all(&output_dir);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. cffi_signature 完整性测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cffi_signature_concurrent_completeness() {
    use compiler::codegen::aot::runtime::cffi_signature;

    // 验证所有并发 C 函数都有签名定义
    let concurrent_c_names = [
        "aura_thread_create",
        "aura_thread_join",
        "aura_thread_sleep",
        "aura_thread_id",
        "aura_thread_available_parallelism",
        "aura_mutex_new",
        "aura_mutex_lock",
        "aura_mutex_unlock",
        "aura_mutex_trylock",
        "aura_mutex_destroy",
        "aura_rwlock_new",
        "aura_rwlock_read_lock",
        "aura_rwlock_write_lock",
        "aura_rwlock_read_unlock",
        "aura_rwlock_write_unlock",
        "aura_rwlock_destroy",
        "aura_atomic_load",
        "aura_atomic_store",
        "aura_atomic_add",
        "aura_atomic_sub",
        "aura_atomic_cas",
    ];

    for name in &concurrent_c_names {
        assert!(
            cffi_signature(name).is_some(),
            "cffi_signature should have entry for '{}'",
            name
        );
    }

    // 验证返回类型和参数类型正确
    let (ret, params) = cffi_signature("aura_mutex_new").unwrap();
    assert_eq!(ret, "i64");
    assert_eq!(params.len(), 0);

    let (ret, params) = cffi_signature("aura_mutex_lock").unwrap();
    assert_eq!(ret, "void");
    assert_eq!(params.len(), 1);

    let (ret, params) = cffi_signature("aura_mutex_trylock").unwrap();
    assert_eq!(ret, "i32");
    assert_eq!(params.len(), 1);

    let (ret, params) = cffi_signature("aura_atomic_load").unwrap();
    assert_eq!(ret, "i64");
    assert_eq!(params.len(), 1);

    let (ret, params) = cffi_signature("aura_thread_id").unwrap();
    assert_eq!(ret, "i64");
    assert_eq!(params.len(), 0);
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. translate_to_legacy_c 映射测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_translate_to_legacy_c_concurrent() {
    use compiler::codegen::aot::runtime::translate_to_legacy_c;

    // 验证并发类名到 C 前缀的映射
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Mutex_new"),
        "aura_mutex_new"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Mutex_lock"),
        "aura_mutex_lock"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Atomic_new"),
        "aura_atomic_new"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Atomic_load"),
        "aura_atomic_load"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Thread_sleep"),
        "aura_thread_sleep"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_RwLock_new"),
        "aura_rwlock_new"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Condvar_new"),
        "aura_condvar_new"
    );
    assert_eq!(
        translate_to_legacy_c("aura_lang_std_Barrier_new"),
        "aura_barrier_new"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. RUNTIME_FUNCTIONS 完整性测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_runtime_functions_concurrent_complete() {
    use compiler::codegen::aot::runtime;

    // 验证所有并发运行时函数都已声明
    let concurrent_runtime_names = [
        "aura_thread_create",
        "aura_thread_join",
        "aura_thread_sleep",
        "aura_thread_id",
        "aura_thread_available_parallelism",
        "aura_mutex_new",
        "aura_mutex_lock",
        "aura_mutex_unlock",
        "aura_mutex_trylock",
        "aura_mutex_destroy",
        "aura_rwlock_new",
        "aura_rwlock_read_lock",
        "aura_rwlock_write_lock",
        "aura_rwlock_read_unlock",
        "aura_rwlock_write_unlock",
        "aura_rwlock_destroy",
        "aura_atomic_load",
        "aura_atomic_store",
        "aura_atomic_add",
        "aura_atomic_sub",
        "aura_atomic_cas",
    ];

    let all_runtime_names: Vec<&str> = runtime::RUNTIME_FUNCTIONS.iter().map(|r| r.name).collect();

    for name in &concurrent_runtime_names {
        assert!(
            all_runtime_names.contains(name),
            "RUNTIME_FUNCTIONS should contain '{}'",
            name
        );
    }
}
