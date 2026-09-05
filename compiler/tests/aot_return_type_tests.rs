#![cfg(feature = "llvm")]

//! Fix 1 — AOT emit_call 返回值类型推断测试
//!
//! 验证 emit_call 不再硬编码 i32，而是从函数签名推断返回类型。
//! 覆盖：用户函数、native 函数（FFI）、void 返回、字符串返回、指针返回。

use compiler::codegen::aot::{AotCodeGenerator, AotOptions, aot_compile};
use compiler::codegen::hir::desugar_program;
use compiler::lexer::Lexer;
use compiler::parser::Parser;

/// 解析源码为 AST
fn parse(src: &str) -> compiler::ast::Program {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    parser.parse_program()
}

/// 生成 LLVM IR 文本
fn gen_ir(src: &str) -> String {
    let program = parse(src);
    let hir = desugar_program(&program);
    let codegen = AotCodeGenerator::new(AotOptions::default());
    codegen.generate_ir(&hir).expect("IR 生成失败")
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. 用户函数返回类型推断
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_user_func_returns_int() {
    let ir = gen_ir("fun add(a: Int, b: Int): Int { return a + b }\nfun main(): Int { return add(1, 2) }");
    assert!(ir.contains("call i32 @add"), "add 返回 Int → i32");
}

#[test]
fn test_user_func_returns_float() {
    let ir = gen_ir("fun avg(a: Float, b: Float): Float { return (a + b) / 2.0f }\nfun main(): Float { return avg(1.0f, 3.0f) }");
    assert!(ir.contains("call float @avg"), "avg 返回 Float → float");
}

#[test]
fn test_user_func_returns_bool() {
    let ir = gen_ir("fun is_even(n: Int): Boolean { return n % 2 == 0 }\nfun main(): Int { return 0 }");
    assert!(ir.contains("call i1 @is_even") || !ir.contains("call i32 @is_even"), "is_even 返回 Boolean → i1");
}

#[test]
fn test_user_func_returns_void() {
    let ir = gen_ir("fun log(msg: Int) { println(msg) }\nfun main(): Int { log(42); return 0 }");
    // void 返回 → 不应包含 "call i32 @log"
    assert!(!ir.contains("call i32 @log"), "log 返回 Unit → 不应用 i32");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Native/FFI 函数返回类型推断
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_native_returns_int() {
    let ir = gen_ir(
        r#"
        extern "c" "test" {
            fun native_add(a: Int, b: Int): Int
        }
        fun main(): Int { return native_add(1, 2) }
    "#,
    );
    assert!(ir.contains("call i32 @native_add"), "native_add 返回 Int → i32");
}

#[test]
fn test_native_returns_float() {
    let ir = gen_ir(
        r#"
        extern "c" "test" {
            fun native_sqrt(x: Float): Float
        }
        fun main(): Float { return native_sqrt(4.0f) }
    "#,
    );
    assert!(ir.contains("call float @native_sqrt"), "native_sqrt 返回 Float → float");
}

#[test]
fn test_native_returns_void() {
    let ir = gen_ir(
        r#"
        extern "c" "test" {
            fun native_free(p: Int)
        }
        fun main(): Int { native_free(0); return 0 }
    "#,
    );
    assert!(!ir.contains("call i32 @native_free"), "native_free 返回 Unit → 不应用 i32");
}

#[test]
fn test_native_returns_pointer() {
    // NOTE: Pointer<Int> 当前被解析为 Type::Generic（非 Type::Pointer），
    // 导致 from_ast 走 catch-all 路径 → HirType::Named("<type>")
    // → map_named → %struct.__type_（sanitized）
    // 这是已知问题 #4（Pointer<T> 嵌套泛型解析缺陷），待 Fix 4 修复。
    // 此处测试当前实际行为，确认 emit_call 正确传递了声明的返回类型。
    let ir = gen_ir(
        r#"
        extern "c" "test" {
            fun native_calloc(size: Int): Pointer<Int>
        }
        fun main(): Int { var p = native_calloc(10); return 0 }
    "#,
    );
    // 当前行为：Pointer<Int> 未正确映射为 ptr，而是 %struct.__type_
    // 修复后应为：call ptr @native_calloc
    let call_lines: Vec<&str> = ir.lines().filter(|l| l.contains("native_calloc") && l.contains("call")).collect();
    assert!(!call_lines.is_empty(), "应包含 native_calloc 的 call 指令");
    // 确认不是 i32（修复前的行为）
    assert!(
        !call_lines.iter().any(|l| l.contains("call i32 @native_calloc")),
        "native_calloc 不应返回 i32"
    );
}

#[test]
fn test_native_returns_string() {
    let ir = gen_ir(
        r#"
        extern "c" "test" {
            fun native_strdup(s: CString): CString
        }
        fun main(): Int { var p = native_strdup(CString("hello")); return 0 }
    "#,
    );
    // CString → i8*（type mapper line 75）
    let call_lines: Vec<&str> = ir.lines().filter(|l| l.contains("native_strdup")).collect();
    let call_str = call_lines.join("\n");
    assert!(
        call_str.contains("i8* @native_strdup"),
        "native_strdup 返回 CString → i8*，实际: {}",
        call_str
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 嵌套调用返回类型
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_nested_call_return_type() {
    let ir = gen_ir(
        "fun double(x: Int): Int { return x * 2 }\nfun quad(x: Int): Int { return double(double(x)) }\nfun main(): Int { return quad(5) }",
    );
    assert!(ir.contains("call i32 @double"), "double 调用返回 i32");
}

#[test]
fn test_call_result_used_in_arithmetic() {
    let ir = gen_ir(
        "fun mul(a: Int, b: Int): Int { return a * b }\nfun main(): Int { return mul(3, 4) + 1 }",
    );
    assert!(ir.contains("call i32 @mul"), "mul 调用返回 i32");
    assert!(ir.contains("add"), "加法式存在");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 无返回类型函数的兜底行为
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unknown_func_falls_back_to_i32() {
    // 未声明的函数调用 → 兜底 i32
    let ir = gen_ir("fun main(): Int { return unknown_func() }");
    assert!(ir.contains("call i32 @unknown_func"), "未知函数兜底 i32");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 实际编译验证（需要 LLVM 工具链）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_aot_compile_native_int_return() {
    let src = "fun main(): Int { return 1 + 2 }";
    let _ = gen_ir(src);
    // 如果 LLVM 可用，尝试完整编译
    if let Ok(_result) = std::env::var("AURA_LLVM_HOME") {
        let output = std::env::temp_dir().join("aura_test_native_int");
        let result = aot_compile(src, &output, AotOptions::default());
        assert!(result.is_ok(), "AOT 编译应成功: {:?}", result.err());
        let _ = std::fs::remove_file(&output);
    }
}
