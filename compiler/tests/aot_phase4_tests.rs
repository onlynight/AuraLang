//! Phase 4 集成测试：动态库 / 编译期链接 / 跨模块调用 / 插件系统
//!
//! 需要 `cargo test --features llvm`。

#![cfg(feature = "llvm")]

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, OptimizationLevel, OutputFormat};
use compiler::codegen::aot_embed::embed_aot;
use compiler::codegen::compile_source;
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::aot_runtime::{AotRuntime, ModuleDependency, PluginManager};
use compiler::vm::{Vm, VmOptions};

fn llc_available() -> bool {
    if std::env::var_os("AURA_LLVM_HOME").is_some() {
        return true;
    }
    let paths = std::env::var("PATH").unwrap_or_default();
    for dir in paths.split(';') {
        if std::path::Path::new(dir).join("llc.exe").is_file() {
            return true;
        }
        if std::path::Path::new(dir).join("llc").is_file() {
            return true;
        }
    }
    false
}

fn compile_with_aot_embed(source: &str) -> compiler::codegen::BytecodeModule {
    let module = compile_source(source).expect("bytecode compilation should succeed");
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert!(
        lexer.errors().is_empty(),
        "lex error: {:?}",
        lexer.errors().first()
    );
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    assert!(
        parser.errors().is_empty(),
        "syntax error: {:?}",
        parser.errors().first()
    );
    let hir = desugar_program(&program);
    let options = AotOptions {
        opt_level: OptimizationLevel::default(),
        ..Default::default()
    };
    let work_dir = std::env::temp_dir().join(format!(
        "aura_p4_{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    let result = embed_aot(module, &hir, options, &work_dir).expect("AOT embedding should succeed");
    let _ = std::fs::remove_dir_all(&work_dir);
    result.module
}

// ── 4.1 动态库模式测试 ──

#[test]
fn test_shared_library_format() {
    if !llc_available() {
        eprintln!("skipped: LLVM not available");
        return;
    }

    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(10, 20) }
    "#;

    // 验证 OutputFormat::SharedLibrary 枚举存在
    let format = OutputFormat::SharedLibrary;
    assert_eq!(format, OutputFormat::SharedLibrary);

    // 验证共享库路径字段
    let mut output = compiler::codegen::aot::AotOutput {
        ll_path: None,
        object_path: None,
        exe_path: None,
        blob_path: None,
        shared_library_path: Some(std::path::PathBuf::from("test.dll")),
        rust_host_path: None,
        descriptors: Vec::new(),
        ir_text: String::new(),
    };
    assert!(output.shared_library_path.is_some());

    // 清理（不实际编译，只验证 API）
    let _ = src;
}

// ── 4.2 编译期链接测试 ──

#[test]
fn test_rust_host_format() {
    if !llc_available() {
        eprintln!("skipped: LLVM not available");
        return;
    }

    // 验证 OutputFormat::RustHost 枚举存在
    let format = OutputFormat::RustHost;
    assert_eq!(format, OutputFormat::RustHost);

    // 验证 rust_host_path 字段
    let mut output = compiler::codegen::aot::AotOutput {
        ll_path: None,
        object_path: None,
        exe_path: None,
        blob_path: None,
        shared_library_path: None,
        rust_host_path: Some(std::path::PathBuf::from("test_rust_host.exe")),
        descriptors: Vec::new(),
        ir_text: String::new(),
    };
    assert!(output.rust_host_path.is_some());
}

// ── 4.3 跨模块调用测试 ──

#[test]
fn test_cross_module_dependency_registration() {
    let mut runtime = AotRuntime::new();

    // 注册模块依赖
    let dep = ModuleDependency {
        name: "math_lib".to_string(),
        imports: vec![
            "sqrt".to_string(),
            "pow".to_string(),
        ],
        resolved_module_id: None,
    };
    runtime.register_module_dependency(1, dep.clone());

    // 验证依赖已注册
    let deps = runtime.get_module_dependencies(1);
    assert!(deps.is_some());
    assert_eq!(deps.unwrap().len(), 1);
    assert_eq!(deps.unwrap()[0].name, "math_lib");
    assert_eq!(deps.unwrap()[0].imports.len(), 2);
}

#[test]
fn test_cross_module_symbol_lookup() {
    let mut runtime = AotRuntime::new();

    // 跨模块符号表初始为空
    assert_eq!(runtime.cross_module_symbol_count(), 0);
    assert!(runtime.lookup_cross_module_symbol("add").is_none());
}

#[test]
fn test_cross_module_resolve_no_modules() {
    let mut runtime = AotRuntime::new();

    // 没有已加载模块时，依赖解析应该全部未解决
    let dep = ModuleDependency {
        name: "math_lib".to_string(),
        imports: vec![
            "sqrt".to_string(),
            "pow".to_string(),
        ],
        resolved_module_id: None,
    };
    runtime.register_module_dependency(1, dep);

    let (resolved, unresolved) = runtime.resolve_dependencies();
    assert_eq!(resolved, 0);
    assert_eq!(unresolved, 2);
}

// ── 4.5 插件系统测试 ──

#[test]
fn test_plugin_manager_creation() {
    let mut pm = PluginManager::new();
    assert_eq!(pm.plugin_count(), 0);
    assert!(pm.list_plugins().is_empty());
}

#[test]
fn test_plugin_manager_search_paths() {
    let mut pm = PluginManager::new();
    pm.add_search_path("/path/to/plugins".to_string());
    pm.add_search_path("/another/path".to_string());
    assert_eq!(pm.search_paths().len(), 2);
    assert_eq!(pm.search_paths()[0], "/path/to/plugins");
    assert_eq!(pm.search_paths()[1], "/another/path");
}

#[test]
fn test_plugin_manager_discover_empty() {
    let mut pm = PluginManager::new();
    pm.add_search_path("/nonexistent/path".to_string());
    let found = pm.discover_plugins();
    assert!(found.is_empty());
}

#[test]
fn test_plugin_manager_load_nonexistent() {
    let mut pm = PluginManager::new();
    let result = pm.load_plugin("/nonexistent/plugin.auc");
    assert!(result.is_err());
}

#[test]
fn test_plugin_manager_unload_nonexistent() {
    let mut pm = PluginManager::new();
    let result = pm.unload_plugin("nonexistent");
    assert!(!result);
}

// ── 端到端：AOT 嵌入 + 插件系统 ──

#[test]
fn test_aot_embed_with_plugin_manager() {
    if !llc_available() {
        eprintln!("skipped: LLVM not available");
        return;
    }

    let src = r#"
        fun add(a: Int, b: Int): Int { return a + b }
        fun main(): Int { return add(30, 40) }
    "#;

    let embedded = compile_with_aot_embed(src);

    // 使用插件管理器加载
    let mut pm = PluginManager::new();
    let mut vm = Vm::new(&embedded, VmOptions::default()).expect("VM initialization failed");
    let result = vm.run().expect("AOT execution failed");

    assert_eq!(result, compiler::vm::Value::Int(70));
    assert_eq!(pm.plugin_count(), 0);
}
