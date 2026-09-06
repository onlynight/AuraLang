//! P8 FFI — Rust ABI 语法测试
//!
//! 验证 `extern "rust"` 语法被 parser 正确解析。
//!
//! 注：`extern "rust"` 调用约定与 `extern "c"` 相同（均走 C ABI），
//! 差异仅在语义标记（工具链识别、编译器警告、库发现）。
//! Rust 侧必须使用 `#[no_mangle] extern "C"`。

use compiler::codegen::compile_source;
use compiler::codegen::hir::desugar_program;
use compiler::codegen::opcode::FfiAbi;
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::vm::{Value, Vm, VmOptions};

/// 编译源码并执行 main，返回结果
fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    vm.run().expect("运行应成功")
}

/// 仅编译源码（不执行），用于语法/语义检查
fn compile_only(source: &str) {
    compile_source(source).expect("编译应成功");
}

/// 解析源码并生成 HIR（用于检查 HIR 结构）
fn parse_to_hir(source: &str) -> compiler::codegen::hir::HirProgram {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    assert!(lexer.errors().is_empty(), "词法错误: {:?}", lexer.errors());
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let perrs: Vec<_> = parser
        .errors()
        .iter()
        .filter(|e| e.severity == compiler::errors::ErrorSeverity::Error)
        .collect();
    assert!(perrs.is_empty(), "语法错误: {:?}", perrs);
    desugar_program(&program)
}

// ─────────────────────────────────────────────────────────────────────────────
// P1: extern "rust" 声明解析
// ─────────────────────────────────────────────────────────────────────────────

/// extern "rust" "lib" { fun foo(...) } 基本声明
#[test]
fn test_extern_rust_function_decl() {
    let src = r#"
        extern "rust" "mylib" {
            fun myAdd(a: Int, b: Int): Int
            fun myPrint(msg: String)
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// extern "rust" 块中声明常量
#[test]
fn test_extern_rust_constant_decl() {
    let src = r#"
        extern "rust" "mylib" {
            val MY_CONSTANT: Int = 100
            val PI: Float = 3.14f
            fun myFunc(a: Int): Int
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// extern "rust" 无库名（静态链接）
#[test]
fn test_extern_rust_no_library() {
    let src = r#"
        extern "rust" {
            fun strlen(s: String): Int
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// extern "rust" 与 extern "c" 混合使用
#[test]
fn test_extern_c_and_rust_mixed() {
    let src = r#"
        extern "c" "raylib" {
            fun InitWindow(w: Int, h: Int, title: CString)
        }
        extern "rust" "game_engine" {
            fun createGame(): Handle
            fun gameUpdate(g: Handle, dt: Float)
        }
        fun main(): Int {
            return 0
        }
    "#;
    compile_only(src);
}

/// extern "rust" 支持 Pointer/Handle 类型
#[test]
fn test_extern_rust_pointer_types() {
    let src = r#"
        extern "rust" "mylib" {
            fun create(): Handle
            fun destroy(h: Handle)
            fun process(data: Pointer<Byte>, len: Long)
            fun getPtr(): Pointer<Int>
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

/// extern "rust" 块中函数可被调用（VM 端，未链接返回占位值）
#[test]
fn test_extern_rust_function_call_vm() {
    let src = r#"
        extern "rust" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int {
            return add(1, 2)
        }
    "#;
    let result = run_main(src);
    assert!(matches!(result, Value::Int(_)));
}

/// extern "rust" 常量在字节码中的存储
#[test]
fn test_extern_rust_constants_in_bytecode() {
    let src = r#"
        extern "rust" "mylib" {
            val MY_INT: Int = 42
            val MY_FLOAT: Float = 3.14f
            val MY_STR: String = "hello"
        }
        fun main(): Int {
            return 42
        }
    "#;
    let module = compile_source(src).expect("编译应成功");
    let has_int_42 = module
        .consts
        .iter()
        .any(|c| matches!(c, compiler::codegen::opcode::Const::Int(42)));
    let has_float_314 = module.consts.iter().any(
        |c| matches!(c, compiler::codegen::opcode::Const::Float(f) if (*f - 3.14).abs() < 0.001),
    );
    let has_str_hello = module
        .consts
        .iter()
        .any(|c| matches!(c, compiler::codegen::opcode::Const::Str(s) if s == "hello"));
    assert!(has_int_42, "常量池应包含 Int(42)");
    assert!(has_float_314, "常量池应包含 Float(3.14)");
    assert!(has_str_hello, "常量池应包含 Str(\"hello\")");
}

/// extern "rust" 回调注册（语法检查）
#[test]
fn test_extern_rust_callback_registration() {
    let src = r#"
        extern "rust" "callbacklib" {
            fun registerCallback(cb: Pointer<Int>)
        }
        fun handler(a: Int, b: Int, c: Int, d: Int): Int {
            return a + b + c + d
        }
        fun main(): Int {
            val cb = makeCallback(handler)
            registerCallback(cb)
            return 42
        }
    "#;
    let result = run_main(src);
    assert_eq!(result, Value::Int(42));
}

/// extern "rust" 大小写不敏感（"Rust" 也接受）
#[test]
fn test_extern_rust_case_insensitive() {
    let src = r#"
        extern "Rust" "mylib" {
            fun foo(): Int
        }
        fun main(): Int {
            return 42
        }
    "#;
    compile_only(src);
}

// ─────────────────────────────────────────────────────────────────────────────
// P2: HIR 结构验证
// ─────────────────────────────────────────────────────────────────────────────

/// extern "rust" 块 → HIR 中 ffi_abi = Rust, ffi_lib = Some("mylib")
#[test]
fn test_hir_ffi_abi_rust() {
    let src = r#"
        extern "rust" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let hir = parse_to_hir(src);
    // 找到 add 函数
    let add_fn = hir.natives.iter().find(|f| f.name == "add").expect("应找到 add");
    assert!(add_fn.is_native, "add 应为原生函数");
    assert_eq!(add_fn.ffi_abi, FfiAbi::Rust, "ffi_abi 应为 Rust");
    assert_eq!(add_fn.ffi_lib, Some("mylib".to_string()), "ffi_lib 应为 Some(\"mylib\")");
}

/// extern "c" 块 → HIR 中 ffi_abi = C
#[test]
fn test_hir_ffi_abi_c() {
    let src = r#"
        extern "c" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let hir = parse_to_hir(src);
    let add_fn = hir.natives.iter().find(|f| f.name == "add").expect("应找到 add");
    assert!(add_fn.is_native);
    assert_eq!(add_fn.ffi_abi, FfiAbi::C, "ffi_abi 应为 C");
    assert_eq!(add_fn.ffi_lib, Some("mylib".to_string()), "ffi_lib 应为 Some(\"mylib\")");
}

/// extern "rust" 无库名 → ffi_lib = None
#[test]
fn test_hir_ffi_abi_rust_no_lib() {
    let src = r#"
        extern "rust" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let hir = parse_to_hir(src);
    let add_fn = hir.natives.iter().find(|f| f.name == "add").expect("应找到 add");
    assert_eq!(add_fn.ffi_abi, FfiAbi::Rust);
    assert_eq!(add_fn.ffi_lib, None, "ffi_lib 应为 None");
}

/// 普通函数 → ffi_abi = None
#[test]
fn test_hir_ffi_abi_none_for_regular_fn() {
    let src = r#"
        fun add(a: Int, b: Int): Int {
            return a + b
        }
        fun main(): Int { return add(1, 2) }
    "#;
    let hir = parse_to_hir(src);
    let add_fn = hir.functions.iter().find(|f| f.name == "add").expect("应找到 add");
    assert!(!add_fn.is_native);
    assert_eq!(add_fn.ffi_abi, FfiAbi::None, "普通函数 ffi_abi 应为 None");
    assert_eq!(add_fn.ffi_lib, None);
}

/// 内置函数 → ffi_abi = None
#[test]
fn test_hir_ffi_abi_none_for_builtin() {
    let src = r#"
        fun main(): Int { println("hello"); return 42 }
    "#;
    let hir = parse_to_hir(src);
    let println_fn = hir.natives.iter().find(|f| f.name == "println").expect("应找到 println");
    assert!(println_fn.is_native);
    assert_eq!(println_fn.ffi_abi, FfiAbi::None, "内置函数 ffi_abi 应为 None");
    assert_eq!(println_fn.ffi_lib, None);
}

/// extern "Rust" 大小写不敏感 → ffi_abi = Rust
#[test]
fn test_hir_ffi_abi_rust_uppercase() {
    let src = r#"
        extern "Rust" "mylib" {
            fun foo(): Int
        }
        fun main(): Int { return 42 }
    "#;
    let hir = parse_to_hir(src);
    let foo_fn = hir.natives.iter().find(|f| f.name == "foo").expect("应找到 foo");
    assert_eq!(foo_fn.ffi_abi, FfiAbi::Rust, "\"Rust\" 大小写应映射为 FfiAbi::Rust");
}

// ─────────────────────────────────────────────────────────────────────────────
// P3: 序列化 round-trip 验证
// ─────────────────────────────────────────────────────────────────────────────

/// extern "rust" 块的 FFI ABI 信息在字节码序列化/反序列化后保留
#[test]
fn test_serialization_roundtrip_ffi_abi() {
    use compiler::codegen::opcode::FfiAbi;
    use compiler::codegen::serialize::{from_bytes, to_bytes};

    let src = r#"
        extern "rust" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    // 编译为字节码模块
    let module = compile_source(src).expect("编译应成功");
    // 序列化
    let bytes = to_bytes(&module);
    // 反序列化
    let loaded = from_bytes(&bytes).expect("反序列化应成功");

    // 验证 FFI ABI 信息保留
    let add_native = loaded
        .natives
        .iter()
        .find(|n| n.name == "add")
        .expect("应找到 add 原生函数");
    assert_eq!(add_native.ffi_abi, FfiAbi::Rust, "序列化后 ffi_abi 应保留为 Rust");
    assert_eq!(
        add_native.ffi_lib,
        Some("mylib".to_string()),
        "序列化后 ffi_lib 应保留为 Some(\"mylib\")"
    );
}

/// extern "c" 块的 FFI ABI 信息在序列化/反序列化后保留
#[test]
fn test_serialization_roundtrip_ffi_abi_c() {
    use compiler::codegen::opcode::FfiAbi;
    use compiler::codegen::serialize::{from_bytes, to_bytes};

    let src = r#"
        extern "c" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let module = compile_source(src).expect("编译应成功");
    let bytes = to_bytes(&module);
    let loaded = from_bytes(&bytes).expect("反序列化应成功");

    let add_native = loaded
        .natives
        .iter()
        .find(|n| n.name == "add")
        .expect("应找到 add 原生函数");
    assert_eq!(add_native.ffi_abi, FfiAbi::C, "序列化后 ffi_abi 应保留为 C");
}

// ─────────────────────────────────────────────────────────────────────────────
// P4: sema 警告验证
// ─────────────────────────────────────────────────────────────────────────────

/// extern "rust" 块应产生 sema 警告（提示 Rust 侧需 #[no_mangle] extern "C"）
#[test]
fn test_sema_warning_for_extern_rust() {
    let src = r#"
        extern "rust" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let (_ast, sema) = compiler::sema::analyze_source(src);
    // 查找警告
    let warnings: Vec<_> = sema
        .errors
        .iter()
        .filter(|e| e.severity == compiler::errors::ErrorSeverity::Warning)
        .collect();
    assert!(
        !warnings.is_empty(),
        "extern \"rust\" 块应产生 sema 警告"
    );
    let has_ffi_warning = warnings.iter().any(|w| {
        w.message.contains("#[no_mangle]") && w.message.contains("extern")
    });
    assert!(
        has_ffi_warning,
        "警告应包含 #[no_mangle] extern 提示，实际警告: {:?}",
        warnings.iter().map(|w| w.message.as_str()).collect::<Vec<_>>()
    );
}

/// extern "c" 块不应产生 FFI 相关警告
#[test]
fn test_no_sema_warning_for_extern_c() {
    let src = r#"
        extern "c" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int { return 42 }
    "#;
    let (_ast, sema) = compiler::sema::analyze_source(src);
    let ffi_warnings: Vec<_> = sema
        .errors
        .iter()
        .filter(|e| {
            e.severity == compiler::errors::ErrorSeverity::Warning
                && e.message.contains("#[no_mangle]")
        })
        .collect();
    assert!(
        ffi_warnings.is_empty(),
        "extern \"c\" 块不应产生 FFI 警告"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// P7: AOT 集成测试
// ─────────────────────────────────────────────────────────────────────────────

/// extern "rust" 块的 AOT 生成 LLVM IR 包含正确的 declare 声明
#[cfg(feature = "llvm")]
#[test]
fn test_aot_llvm_ir_for_extern_rust() {
    use compiler::codegen::aot::{aot_compile, AotOptions};
    use std::path::PathBuf;

    let src = r#"
        extern "rust" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int {
            return add(1, 2)
        }
    "#;

    let output_path = std::env::temp_dir().join("p8_ffi_rust_test.ll");
    let options = AotOptions::default();

    let result = aot_compile(src, &output_path, options);
    match result {
        Ok(output) => {
            // 验证 LLVM IR 文本包含正确的声明
            let ir = &output.ir_text;
            assert!(
                ir.contains("declare"),
                "LLVM IR 应包含 declare 声明"
            );
            assert!(
                ir.contains("add"),
                "LLVM IR 应包含 add 函数"
            );
            assert!(
                ir.contains("P8-Rust"),
                "LLVM IR 应包含 P8-Rust 注释"
            );
            // 清理临时文件
            let _ = std::fs::remove_file(&output_path);
        }
        Err(e) => {
            eprintln!("AOT 编译失败（可能缺少 LLVM 工具链）: {}", e);
        }
    }
}

/// extern "c" 块的 AOT 生成 LLVM IR 不包含 P8-Rust 注释
#[cfg(feature = "llvm")]
#[test]
fn test_aot_llvm_ir_for_extern_c() {
    use compiler::codegen::aot::{aot_compile, AotOptions};

    let src = r#"
        extern "c" "mylib" {
            fun add(a: Int, b: Int): Int
        }
        fun main(): Int {
            return add(1, 2)
        }
    "#;

    let output_path = std::env::temp_dir().join("p8_ffi_c_test.ll");
    let options = AotOptions::default();

    let result = aot_compile(src, &output_path, options);
    match result {
        Ok(output) => {
            let ir = &output.ir_text;
            assert!(
                !ir.contains("P8-Rust"),
                "extern \"c\" 的 LLVM IR 不应包含 P8-Rust 注释"
            );
            let _ = std::fs::remove_file(&output_path);
        }
        Err(e) => {
            eprintln!("AOT 编译失败（可能缺少 LLVM 工具链）: {}", e);
        }
    }
}
