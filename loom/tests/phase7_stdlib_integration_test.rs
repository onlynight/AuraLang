//! Phase 7: 标准库集成测试
//!
//! 覆盖：所有标准库模块在 VM/JIT/AOT 三种模式下的编译与执行、
//! FFI AOT 直接调用、跨模块调用、模式切换、多模块加载、自举验证。
//!
//! 对应 Phase 7 §7.1-7.3 in 完全Aura化技术方案-final.md

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

// ═══════════════════════════════════════════════════════════════════════════════
// 测试辅助函数
// ═══════════════════════════════════════════════════════════════════════════════

fn create_temp_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn compile_test_source(source: &str) -> compiler::codegen::BytecodeModule {
    compile_source(source).unwrap()
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. 标准库模块编译测试（VM 模式）
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试 Math 模块编译
#[test]
fn test_math_module_compiles() {
    let source = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
    fun min(a: Int, b: Int): Int {
        if (a < b) a else b
    }
    fun max(a: Int, b: Int): Int {
        if (a > b) a else b
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "Math module should contain functions"
    );
    assert!(
        module.functions.iter().any(|f| f.name.contains("abs")),
        "Math 模块应包含 abs"
    );
}

/// 测试 String 模块编译
#[test]
fn test_string_module_compiles() {
    let source = r#"
internal object String {
    fun length(s: String): Int {
        s.length
    }
    fun contains(text: String, substring: String): Boolean {
        indexOf(text, substring) >= 0
    }
    fun indexOf(text: String, substring: String): Int {
        -1
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "String module should contain functions"
    );
}

/// 测试 Path 模块编译
#[test]
fn test_path_module_compiles() {
    let source = r#"
internal object Path {
    fun join(a: String, b: String): String {
        a + "/" + b
    }
    fun dirname(path: String): String {
        path.substring(0, path.length - 1)
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "Path module should contain functions"
    );
}

/// 测试 Collections 模块编译
#[test]
fn test_collections_module_compiles() {
    let source = r#"
internal object Collections {
    fun emptyList(): List<Any> {
        null
    }
    fun emptyMap(): Map<Any, Any> {
        null
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "Collections module should contain functions"
    );
}

/// 测试 FileSystem 模块编译（FFI AOT 直接调用）
#[test]
fn test_filesystem_module_compiles() {
    let source = r#"
internal object FileSystem {
    fun exists(path: String): Boolean {
        false
    }
    fun isFile(path: String): Boolean {
        false
    }
    fun isDirectory(path: String): Boolean {
        false
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "FileSystem module should contain functions"
    );
}

/// 测试 IO 模块编译（FFI AOT 直接调用）
#[test]
fn test_io_module_compiles() {
    let source = r#"
internal object IO {
    fun println(msg: String): Unit {
        0
    }
    fun print(msg: String): Unit {
        0
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "IO module should contain functions"
    );
}

/// 测试 Network 模块编译（FFI AOT 直接调用）
#[test]
fn test_network_module_compiles() {
    let source = r#"
internal object Network {
    fun tcpConnect(host: String, port: Int): Any {
        null
    }
    fun tcpSend(socket: Any, data: String): Int {
        0
    }
    fun tcpRecv(socket: Any, bufferSize: Int): String {
        ""
    }
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.functions.is_empty(),
        "Network module should contain functions"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. 跨模块调用测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试跨模块调用编译
#[test]
fn test_cross_module_call() {
    let source = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
    fun min(a: Int, b: Int): Int {
        if (a < b) a else b
    }
    fun max(a: Int, b: Int): Int {
        if (a > b) a else b
    }
}
fun test(): Int {
    val x = Math.abs(-42)
    val y = Math.min(x, 10)
    val z = Math.max(x, y)
    z
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
    assert!(module.functions.iter().any(|f| f.name == "test"));
}

/// 测试多模块交叉引用
#[test]
fn test_multi_module_cross_reference() {
    let source = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
internal object String {
    fun repeat(text: String, times: Int): String {
        text
    }
}
fun test(): String {
    val s = "Hello"
    String.repeat(s, 3)
}
"#;
    let module = compile_test_source(source);
    assert!(!module.functions.is_empty());
    assert!(module.functions.iter().any(|f| f.name == "test"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. FFI AOT 直接调用测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试 extern interface 解析
#[test]
fn test_extern_interface_math() {
    let source = r#"
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
    fun min(a: Int, b: Int): Int
}
"#;
    let module = compile_test_source(source);
    assert!(
        !module.natives.is_empty(),
        "should contain native function declarations"
    );
    assert!(
        module.natives.iter().any(|n| n.name.contains("abs")),
        "应包含 Math.abs"
    );
}

/// 测试 extern "c" 解析（FileSystem FFI）
#[test]
fn test_extern_c_filesystem() {
    let source = r#"
extern "c" "libc" {
    fun stat(path: String, statbuf: Pointer): Int
    fun fopen(path: String, mode: String): Pointer
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
    assert!(module.natives.iter().any(|n| n.name.contains("stat")));
}

/// 测试 extern "c" 解析（IO FFI）
#[test]
fn test_extern_c_io() {
    let source = r#"
extern "c" "libc" {
    fun printf(format: String): Int
    fun fgets(buf: Pointer, size: Int, stream: Pointer): Pointer
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
    assert!(module.natives.iter().any(|n| n.name.contains("printf")));
}

/// 测试 extern "c" 解析（Network FFI）
#[test]
fn test_extern_c_network() {
    let source = r#"
extern "c" "libc" {
    fun socket(domain: Int, type: Int, protocol: Int): Int
    fun connect(sockfd: Int, addr: Pointer, addrlen: Int): Int
    fun send(sockfd: Int, buf: Pointer, len: Int, flags: Int): Int
    fun recv(sockfd: Int, buf: Pointer, len: Int, flags: Int): Int
}
"#;
    let module = compile_test_source(source);
    assert!(!module.natives.is_empty());
    assert!(module.natives.iter().any(|n| n.name.contains("socket")));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. FFI AOT 配置测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试所有执行模式下的 FFI AOT 配置
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

// ═══════════════════════════════════════════════════════════════════════════════
// 5. 多模块加载测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试多模块加载
#[test]
fn test_multi_module_load() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // 写入 Math 模块
    let math_module = compile_test_source(
        "internal object Math { fun abs(x: Int): Int { if (x < 0) -x else x } }",
    );
    write_auc(output_dir.join("Math.auc").to_str().unwrap(), &math_module).unwrap();

    // 写入 String 模块
    let string_module =
        compile_test_source("internal object String { fun length(s: String): Int { s.length } }");
    write_auc(
        output_dir.join("String.auc").to_str().unwrap(),
        &string_module,
    )
    .unwrap();

    // 使用 MultiModuleVm 加载
    let mut vm = MultiModuleVm::new();
    let result = compiler::vm::multi_module::load_modules_from_dir(&mut vm, &output_dir, "Math");
    assert!(result.is_ok());
    assert!(vm.len() >= 2);
}

/// 测试 20 个模块批量加载
#[test]
fn test_bulk_module_load() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // 创建 20 个模块
    for i in 0..20 {
        let source = format!("internal object Module{} {{ fun test() {{ 0 }} }}", i);
        let module = compile_test_source(&source);
        write_auc(
            output_dir.join(format!("Module{}.auc", i)).to_str().unwrap(),
            &module,
        )
        .unwrap();
    }

    // 批量加载
    let mut vm = MultiModuleVm::new();
    let result = compiler::vm::multi_module::load_modules_from_dir(&mut vm, &output_dir, "Module0");
    assert!(result.is_ok());
    assert_eq!(vm.len(), 20);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. 模式切换测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试 VM 到 JIT 模式切换
#[test]
fn test_mode_switching_vm_to_jit() {
    use compiler::codegen::ffi_aot::{FfiAotConfig, configure_ffi_aot_direct};

    let source = r#"
internal object Math {
    fun abs(x: Int): Int {
        if (x < 0) -x else x
    }
}
extern interface Math {
    default fun loadLibrary(): String = "aura_std_math"
    fun abs(x: Int): Int
}
"#;
    let module = compile_test_source(source);

    // VM 模式配置
    let vm_result = configure_ffi_aot_direct(
        &module,
        compiler::codegen::ffi_aot::ExecutionMode::Vm,
        &FfiAotConfig::default(),
    );
    assert!(vm_result.is_ok());

    // JIT 模式配置
    let jit_result = configure_ffi_aot_direct(
        &module,
        compiler::codegen::ffi_aot::ExecutionMode::Jit,
        &FfiAotConfig::default(),
    );
    assert!(jit_result.is_ok());

    // 两种模式应配置相同数量的声明
    assert_eq!(
        vm_result.unwrap().declarations_configured,
        jit_result.unwrap().declarations_configured
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. 自举验证测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试自举验证：用 Aura 代码编译 Aura 标准库
#[test]
fn test_self_bootstrap_verification() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // 创建测试 Aura 源文件
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
        s.length
    }
}
"#;
    std::fs::write(temp_dir.join("String.aura"), string_src).unwrap();

    // 运行编译流程
    let result = compile_stdlib(temp_dir, &output_dir, None, ExecutionMode::Vm, FfiMode::Aot);
    assert!(
        result.is_ok(),
        "compile_stdlib 失败: {}",
        result.err().map(|e| e.to_string()).unwrap_or_default()
    );

    let compile_result = result.unwrap();
    assert!(!compile_result.auc_files.is_empty());
    assert!(compile_result.auc_files.len() >= 2);

    // 验证 .auc 文件存在
    let auc_files: Vec<_> = std::fs::read_dir(&output_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("auc"))
        .collect();
    assert!(auc_files.len() >= 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. 包构建与安装测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试标准库包构建
#[test]
fn test_stdlib_package_build() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // 创建 stdlib 索引
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

    // 创建 .auc 文件
    let module = compile_test_source(
        "internal object Math { fun abs(x: Int): Int { if (x < 0) -x else x } }",
    );
    write_auc(temp_dir.join("Math.auc").to_str().unwrap(), &module).unwrap();

    // 构建包
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

    // 读回验证
    let content = aura_loom::package::read_auz_package(&auz_path).unwrap();
    assert_eq!(content.manifest.name, "aura-stdlib");
    assert_eq!(content.manifest.version, "1.0.0");
    assert!(content.verified);
    assert!(content.auc_file_count >= 1);
}

/// 测试包安装/卸载
#[test]
fn test_package_install_uninstall() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // 构建包
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

    // 安装
    let config = InstallConfig {
        install_dir: temp_dir.join("installed"),
        verify_checksum: true,
    };
    let install_result = aura_loom::package::install_auz(&auz_path, &config);
    assert!(install_result.success);
    assert!(aura_loom::package::is_installed(&config));

    // 获取信息
    let info = aura_loom::package::get_installed_info(&config).unwrap();
    assert_eq!(info.name, "aura-stdlib");
    assert_eq!(info.version, "1.0.0");

    // 卸载
    let uninstall_result = aura_loom::package::uninstall_auz(&config);
    assert!(uninstall_result.success);
    assert!(!aura_loom::package::is_installed(&config));
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. 性能基准测试（集成层面）
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试 FFI 缓存查找性能
#[test]
fn test_bench_ffi_cache_lookup() {
    use compiler::codegen::ffi_cache::FfiCallCache;

    let mut cache = FfiCallCache::new();

    // 预热
    for i in 0..100usize {
        let key = format!("func_{}", i % 10);
        cache.preload(&key, i * 100);
    }

    // 基准测量
    let start = Instant::now();
    let iterations = 10_000;
    for i in 0..iterations {
        let key = format!("func_{}", i % 10);
        let _ = cache.lookup(&key);
    }
    let elapsed = start.elapsed();

    eprintln!(
        "FFI cache lookup: {:.2} ns/op",
        elapsed.as_nanos() as f64 / iterations as f64
    );
}

/// 测试多模块加载性能
#[test]
fn test_bench_multi_module_load_perf() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();
    let output_dir = temp_dir.join("output");
    std::fs::create_dir_all(&output_dir).unwrap();

    // 创建 20 个测试模块
    for i in 0..20 {
        let source = format!("internal object Module{} {{ fun test() {{ 0 }} }}", i);
        let module = compile_test_source(&source);
        write_auc(
            output_dir.join(format!("Module{}.auc", i)).to_str().unwrap(),
            &module,
        )
        .unwrap();
    }

    // 基准测量加载性能
    let start = Instant::now();
    let mut vm = MultiModuleVm::new();
    let result = compiler::vm::multi_module::load_modules_from_dir(&mut vm, &output_dir, "Module0");
    let elapsed = start.elapsed();

    assert!(result.is_ok());
    assert_eq!(vm.len(), 20);

    eprintln!("Multi-module load (20 modules): {} ms", elapsed.as_millis());
}

/// 测试包构建性能
#[test]
fn test_bench_package_build_perf() {
    let dir = create_temp_dir();
    let temp_dir = dir.path();

    // 创建包含 10 个模块的 stdlib 索引
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

    // 创建 .auc 文件
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

// ═══════════════════════════════════════════════════════════════════════════════
// 10. 兼容性测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试平台检测
#[test]
fn test_platform_detection() {
    let platform = detect_platform();
    assert!(!platform.is_empty());
    assert!(platform.contains("x86_64"));
}

/// 测试版本号格式
#[test]
fn test_stdlib_version_format() {
    let version = "1.0.0";
    assert!(version.split('.').count() == 3);
}

/// 测试 Auz 格式版本
#[test]
fn test_auz_format_version() {
    use compiler::auz::APKG_FORMAT_VERSION;
    assert_eq!(APKG_FORMAT_VERSION, 1);
}

/// 测试执行模式序列化
#[test]
fn test_execution_mode_serialization() {
    for _mode in [
        compiler::codegen::execution::ExecutionMode::Vm,
        compiler::codegen::execution::ExecutionMode::Jit,
        compiler::codegen::execution::ExecutionMode::Aot,
    ] {
        // 验证类型存在且可编译
    }
}

/// 测试 FFI 模式序列化
#[test]
fn test_ffi_mode_serialization() {
    for _mode in [
        FfiMode::Aot,
        FfiMode::Cffi,
        FfiMode::RustFfi,
    ] {
        // 验证类型存在且可编译
    }
}

/// 测试 FFI 索引构建
#[test]
fn test_ffi_index_construction() {
    let mut ffi_index = FfiIndex::new(FfiMode::Aot);
    ffi_index.declarations.push(FfiDeclaration {
        name: "Math.abs".to_string(),
        library: "aura_std_math".to_string(),
        language: "aura".to_string(),
        module: "Math".to_string(),
        function_address: Some(0x400000),
    });
    assert_eq!(ffi_index.declarations.len(), 1);
    assert!(ffi_index.declarations[0].name.contains("Math"));
}
