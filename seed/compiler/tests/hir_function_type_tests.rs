#![cfg(feature = "llvm")]

//! Fix 3 — HirType Function 变体测试
//!
//! 验证函数类型 (A, B) -> R 不再被丢弃，而是正确降级为 HirType::Function。
//! 覆盖：基本函数类型、嵌套函数类型、函数类型在参数/返回值中的使用。

use compiler::codegen::aot::types::TypeMapper;
use compiler::codegen::hir::{HirType, desugar_program};
use compiler::lexer::Lexer;
use compiler::parser::Parser;

/// 解析源码为 AST → HIR
fn parse_to_hir(src: &str) -> compiler::codegen::hir::HirProgram {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    desugar_program(&program)
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. 函数类型解析
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_function_type_basic() {
    let hir = parse_to_hir("fun apply(f: (Int) -> Int, x: Int): Int { return f(x) }");
    // apply 函数的第一个参数类型应为 Function
    let apply = hir.functions.iter().find(|f| f.name == "apply").expect("apply function exists");
    let param_type = apply.params[0].ty.as_ref().expect("parameter type exists");
    match param_type {
        HirType::Function {
            params,
            return_type,
        } => {
            assert_eq!(params.len(), 1, "should have 1 parameter");
            assert!(
                matches!(return_type.as_ref(), HirType::Named(n) if n == "Int"),
                "return type should be Int"
            );
        }
        other => panic!("expected Function type, got: {:?}", other),
    }
}

#[test]
fn test_hir_function_type_multi_param() {
    let hir = parse_to_hir(
        "fun apply2(f: (Int, String) -> Boolean, x: Int, y: String): Boolean { return f(x, y) }",
    );
    let apply2 = hir.functions.iter().find(|f| f.name == "apply2").expect("apply2 function exists");
    let param_type = apply2.params[0].ty.as_ref().expect("parameter type exists");
    match param_type {
        HirType::Function {
            params,
            return_type,
        } => {
            assert_eq!(params.len(), 2, "should have 2 parameters");
            assert!(
                matches!(return_type.as_ref(), HirType::Named(n) if n == "Boolean"),
                "return type should be Boolean"
            );
        }
        other => panic!("expected Function type, got: {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 函数类型在返回值中的使用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_function_type_as_return() {
    let hir = parse_to_hir("fun make_adder(x: Int): (Int) -> Int { return fun(y: Int) => x + y }");
    let make_adder =
        hir.functions.iter().find(|f| f.name == "make_adder").expect("make_adder exists");
    let ret_type = make_adder.ret.as_ref().expect("return type exists");
    match ret_type {
        HirType::Function {
            params,
            return_type,
        } => {
            assert_eq!(params.len(), 1);
            assert!(matches!(return_type.as_ref(), HirType::Named(n) if n == "Int"));
        }
        other => panic!("expected Function return type, got: {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 嵌套函数类型
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_nested_function_type() {
    let hir = parse_to_hir(
        "fun compose(f: (Int) -> Int, g: (Int) -> Int): (Int) -> Int { return fun(x) => f(g(x)) }",
    );
    let compose = hir.functions.iter().find(|f| f.name == "compose").expect("compose exists");
    let ret_type = compose.ret.as_ref().expect("return type exists");
    match ret_type {
        HirType::Function {
            params,
            return_type,
        } => {
            assert_eq!(params.len(), 1);
            assert!(matches!(return_type.as_ref(), HirType::Named(n) if n == "Int"));
        }
        other => panic!("expected Function return type, got: {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. LLVM 类型映射
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_llvm_function_type_maps_to_ptr() {
    let tm = TypeMapper::new(true);
    let func_type = HirType::Function {
        params: Box::new(vec![HirType::Named("Int".into())]),
        return_type: Box::new(HirType::Named("Int".into())),
    };
    let llvm = tm.map(&func_type);
    assert_eq!(
        llvm, "ptr",
        "function type should map to ptr (opaque pointer)"
    );
}

#[test]
fn test_llvm_fn_type_signature() {
    let tm = TypeMapper::new(true);
    let params = vec![
        HirType::Named("Int".into()),
        HirType::Named("Float".into()),
    ];
    let ret = HirType::Named("Boolean".into());
    let sig = tm.fn_type(&ret, &params, false);
    assert!(sig.contains("i1"), "return type should be i1");
    assert!(sig.contains("i32"), "params should contain i32");
    assert!(sig.contains("float"), "params should contain float");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 无函数类型时不影响其他类型
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_no_function_type() {
    let hir = parse_to_hir("fun main(): Int { return 42 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    let ret_type = main.ret.as_ref().expect("return type exists");
    assert!(matches!(ret_type, HirType::Named(n) if n == "Int"));
}
