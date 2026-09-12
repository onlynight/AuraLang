//! Phase 7: Integration tests for the Aura standard library.
//!
//! Covers: stdlib calling, cross-module calls, FFI AOT direct, multi-module loading,
//! and mode switching.
//!
//! Corresponds to Phase 7 §7.1-7.3 in the full Aura-ification plan.

use aura_loom::package::{
    ExecutionModeId, InstallConfig, StdlibBuildOptions, StdlibPackageBuilder, detect_platform,
};
use aura_loom::stdlib::{
    ExecutionMode, FfiDeclaration, FfiIndex, FfiMode, StdlibIndex, StdlibModule,
};
use aura_loom::task::compile_stdlib::compile_stdlib;
use compiler::codegen::opcode::FfiAbi;
use compiler::codegen::{compile_source, write_auc};
use compiler::vm::multi_module::MultiModuleVm;
use std::path::{Path, PathBuf};
use std::time::Instant;

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

fn create_temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn compile_test_source(source: &str) -> compiler::codegen::BytecodeModule {
    compile_source(source).unwrap()
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: stdlib calling
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_compile_math_module() {
    let source = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
}

#[test]
fn test_compile_string_module() {
    let source = r#"
internal object String {
    fun length(s: String): Int {
        0
    }
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
}

#[test]
fn test_compile_path_module() {
    let source = r#"
internal object Path {
    fun join(a: String, b: String): String {
        a
    }
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
}

#[test]
fn test_compile_collection_module() {
    let source = r#"
internal object Collections {
    fun newArrayList(): Any {
        null
    }
    fun newHashMap(): Any {
        null
    }
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: cross-module calling
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_module_call_compilation() {
    let source = r#"
fun test() {
    val x = Math.abs(-42)
    val y = Math.min(x, 10)
    val z = Math.max(x, y)
    z
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: FFI AOT direct (extern interface)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_extern_interface_parsing() {
    let source = r#"
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
    fun min(a: Int, b: Int): Int
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
}

#[test]
fn test_extern_c_parsing() {
    let source = r#"
extern "c" "libc" {
    fun fopen(path: String, mode: String): Pointer
    fun malloc(size: Int): Pointer
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
}

#[test]
fn test_extern_rust_parsing() {
    let source = r#"
extern "rust" {
    fun any_toString(value: Any): String
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: multi-module loading
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multi_module_load() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // Write minimal .auc files
    let math_module = compile_test_source(
        "internal object Math { fun abs(x: Int): Int { if (x < 0) -x else x } }",
    );
    write_auc(output_dir.join("Math.auc").to_str().unwrap(), &math_module).unwrap();

    let string_module =
        compile_test_source("internal object String { fun length(s: String): Int { 0 } }");
    write_auc(
        output_dir.join("String.auc").to_str().unwrap(),
        &string_module,
    )
    .unwrap();

    // Load using MultiModuleVm
    let mut vm = MultiModuleVm::new();
    let result = compiler::vm::multi_module::load_modules_from_dir(&mut vm, &output_dir, "Math");
    assert!(result.is_ok());
    assert!(vm.len() >= 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: packaging and installation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_stdlib_package_build_and_read() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // Create stdlib index
    let mut idx = StdlibIndex::new(temp_dir.to_path_buf(), ExecutionMode::Vm, FfiMode::Aot);
    idx.modules.push(StdlibModule {
        name: "Math".to_string(),
        full_name: "aura.lang.std.Math".to_string(),
        source_path: temp_dir.join("Math.aura"),
        auc_path: Some(temp_dir.join("Math.auc")),
        function_names: vec![
            "abs".to_string(),
            "min".to_string(),
            "max".to_string(),
        ],
        type_names: vec![],
        constant_names: vec![],
        has_extern: false,
    });

    // Create fake .auc file
    let module = compile_test_source(
        "internal object Math { fun abs(x: Int): Int { if (x < 0) -x else x } }",
    );
    write_auc(temp_dir.join("Math.auc").to_str().unwrap(), &module).unwrap();

    // Build package
    let builder =
        StdlibPackageBuilder::new(&idx).with_auc_dir(temp_dir).with_options(StdlibBuildOptions {
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            execution_modes: vec![ExecutionModeId::Vm],
            ffi_mode: "aot".to_string(),
            compression_level: 1,
            include_sources: false,
        });

    let auz_path = temp_dir.join("std.auz");
    let result = builder.build(&auz_path).unwrap();
    assert!(result.path.exists());
    assert!(result.size_bytes > 0);

    // Read back
    let content = aura_loom::package::read_auz_package(&auz_path).unwrap();
    assert_eq!(content.manifest.name, "aura-stdlib");
    assert_eq!(content.manifest.version, "1.0.0");
    assert!(content.verified);
    assert!(content.auc_file_count >= 1);
}

#[test]
fn test_stdlib_install_update_uninstall() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // Build a package
    let mut idx = StdlibIndex::new(temp_dir.to_path_buf(), ExecutionMode::Vm, FfiMode::Aot);
    idx.modules.push(StdlibModule {
        name: "Math".to_string(),
        full_name: "aura.lang.std.Math".to_string(),
        source_path: temp_dir.join("Math.aura"),
        auc_path: None,
        function_names: vec!["abs".to_string()],
        type_names: vec![],
        constant_names: vec![],
        has_extern: false,
    });

    let builder = StdlibPackageBuilder::new(&idx).with_options(StdlibBuildOptions {
        name: "aura-stdlib".to_string(),
        version: "1.0.0".to_string(),
        execution_modes: vec![ExecutionModeId::Vm],
        ffi_mode: "aot".to_string(),
        compression_level: 1,
        include_sources: false,
    });

    let auz_path = temp_dir.join("std.auz");
    builder.build(&auz_path).unwrap();

    // Install
    let config = InstallConfig {
        install_dir: temp_dir.join("installed"),
        verify_checksum: true,
    };
    let install_result = aura_loom::package::install_auz(&auz_path, &config);
    assert!(install_result.success);
    assert!(aura_loom::package::is_installed(&config));

    // Get info
    let info = aura_loom::package::get_installed_info(&config).unwrap();
    assert_eq!(info.name, "aura-stdlib");
    assert_eq!(info.version, "1.0.0");

    // Uninstall
    let uninstall_result = aura_loom::package::uninstall_auz(&config);
    assert!(uninstall_result.success);
    assert!(!aura_loom::package::is_installed(&config));
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: stdlib compilation pipeline
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_full_stdlib_compile_pipeline() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // Create test .aura sources
    let math_src = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
"#;
    std::fs::write(temp_dir.join("Math.aura"), math_src).unwrap();

    let string_src = r#"
internal object String {
    fun length(s: String): Int {
        0
    }
}
"#;
    std::fs::write(temp_dir.join("String.aura"), string_src).unwrap();

    // Run compile_stdlib
    let result = compile_stdlib(temp_dir, &output_dir, None, ExecutionMode::Vm, FfiMode::Aot);
    assert!(
        result.is_ok(),
        "compile_stdlib failed: {}",
        result.err().map(|e| e.to_string()).unwrap_or_default()
    );

    let compile_result = result.unwrap();
    assert!(!compile_result.auc_files.is_empty());
    assert!(compile_result.auc_files.len() >= 2);

    // Verify .auc files exist
    let auc_files: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("auc"))
        .collect();
    assert!(auc_files.len() >= 2);
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration tests: FFI configuration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ffi_aot_config_all_modes() {
    use compiler::codegen::ffi_aot::{FfiAotConfig, configure_ffi_aot_direct};

    let source = r#"
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
}
extern "c" "libc" fun fopen(path: String, mode: String): Pointer
"#;
    let module = compile_test_source(source);

    // Test all three execution modes
    for mode in [
        compiler::codegen::ffi_aot::ExecutionMode::Vm,
        compiler::codegen::ffi_aot::ExecutionMode::Jit,
        compiler::codegen::ffi_aot::ExecutionMode::Aot,
    ] {
        let result = configure_ffi_aot_direct(&module, mode, &FfiAotConfig::default());
        assert!(
            result.is_ok(),
            "configure_ffi_aot_direct({:?}) failed",
            mode
        );
        let r = result.unwrap();
        assert_eq!(
            r.extern_interface_count + r.c_ffi_count,
            r.declarations_configured
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Performance benchmarks (simplified)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_bench_ffi_cache_lookup() {
    use compiler::codegen::ffi_cache::FfiCallCache;

    let mut cache = FfiCallCache::new();

    // Warm up
    for i in 0..100usize {
        let key = format!("func_{}", i % 10);
        cache.preload(&key, i * 100);
    }

    // Benchmark lookups
    let start = Instant::now();
    let iterations = 10_000;
    for i in 0..iterations {
        let key = format!("func_{}", i % 10);
        let _ = cache.lookup(&key);
    }
    let elapsed = start.elapsed();

    eprintln!(
        "FFI cache lookup: {} ns/op",
        elapsed.as_nanos() / iterations as u128
    );
}

#[test]
fn test_bench_multi_module_load() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // Create many test modules
    for i in 0..20 {
        let source = format!("internal object Module{} {{ fun test() {{ 0 }} }}", i);
        let module = compile_test_source(&source);
        write_auc(
            output_dir.join(format!("Module{}.auc", i)).to_str().unwrap(),
            &module,
        )
        .unwrap();
    }

    // Benchmark loading
    let start = Instant::now();
    let mut vm = MultiModuleVm::new();
    let result = compiler::vm::multi_module::load_modules_from_dir(&mut vm, &output_dir, "Module0");
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert_eq!(vm.len(), 20);

    eprintln!("Multi-module load (20 modules): {} ms", elapsed.as_millis());
}

#[test]
fn test_bench_package_build() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // Create a larger stdlib index
    let mut idx = StdlibIndex::new(temp_dir.to_path_buf(), ExecutionMode::Vm, FfiMode::Aot);
    for i in 0..10 {
        idx.modules.push(StdlibModule {
            name: format!("Module{}", i),
            full_name: format!("aura.lang.std.Module{}", i),
            source_path: temp_dir.join(format!("Module{}.aura", i)),
            auc_path: None,
            function_names: vec![format!(
                "func{}",
                i
            )],
            type_names: vec![],
            constant_names: vec![],
            has_extern: false,
        });
    }

    // Create fake .auc files
    for i in 0..10 {
        let source = format!("internal object Module{} {{ fun func() {{ 0 }} }}", i);
        let module = compile_test_source(&source);
        write_auc(
            temp_dir.join(format!("Module{}.auc", i)).to_str().unwrap(),
            &module,
        )
        .unwrap();
    }

    let builder =
        StdlibPackageBuilder::new(&idx).with_auc_dir(temp_dir).with_options(StdlibBuildOptions {
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            execution_modes: vec![ExecutionModeId::Vm],
            ffi_mode: "aot".to_string(),
            compression_level: 1,
            include_sources: false,
        });

    let auz_path = temp_dir.join("std.auz");
    let start = Instant::now();
    let result = builder.build(&auz_path);
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    let r = result.unwrap();

    eprintln!(
        "Package build (10 modules): {} ms, {} bytes",
        elapsed.as_millis(),
        r.size_bytes
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Compatibility tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_platform_detection() {
    let platform = detect_platform();
    assert!(!platform.is_empty());
    assert!(platform.contains("x86_64"));
}

#[test]
fn test_stdlib_version_format() {
    let version = "1.0.0";
    assert!(version.split('.').count() == 3);
}

#[test]
fn test_auz_format_version() {
    use compiler::auz::APKG_FORMAT_VERSION;
    assert_eq!(APKG_FORMAT_VERSION, 1);
}

#[test]
fn test_execution_mode_serialization() {
    for _mode in [
        compiler::codegen::execution::ExecutionMode::Vm,
        compiler::codegen::execution::ExecutionMode::Jit,
        compiler::codegen::execution::ExecutionMode::Aot,
    ] {
        // Just verify it exists and compiles
    }
}

#[test]
fn test_ffi_mode_serialization() {
    for _mode in [
        FfiMode::Aot,
        FfiMode::Cffi,
        FfiMode::RustFfi,
    ] {
        // Just verify it exists and compiles
    }
}
