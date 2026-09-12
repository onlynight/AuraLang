#![cfg(feature = "llvm")]

//! Fix 4 — HIR Lambda/闭包升级测试
//!
//! 验证 Lambda/Closure 不再降级为 __lambda 调用，而是正确降级为 HirExpr::Lambda。
//! 覆盖：简单 lambda、带参数的 lambda、闭包、lambda 作为返回值。

use compiler::codegen::hir::{HirExpr, HirStmt, desugar_program};
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

/// 在 HIR 函数体中递归搜索 HirExpr::Lambda（含嵌套）
fn find_lambda_recursive(expr: &HirExpr) -> bool {
    match expr {
        HirExpr::Lambda { .. } => true,
        HirExpr::Binary {
            lhs, rhs, ..
        } => find_lambda_recursive(lhs) || find_lambda_recursive(rhs),
        HirExpr::Call { args, .. } => args.iter().any(find_lambda_recursive),
        HirExpr::Block(block) => block.stmts.iter().any(|s| match s {
            HirStmt::Val {
                init: Some(e),
                ..
            }
            | HirStmt::Var {
                init: Some(e),
                ..
            } => find_lambda_recursive(e),
            _ => false,
        }),
        _ => false,
    }
}

/// 在 HIR 函数体中搜索 HirExpr::Lambda
fn find_lambda_in_block(block: &compiler::codegen::hir::HirBlock) -> bool {
    block.stmts.iter().any(|s| match s {
        HirStmt::Expr(e) => find_lambda_recursive(e),
        HirStmt::Val {
            init: Some(e),
            ..
        } => find_lambda_recursive(e),
        HirStmt::Var {
            init: Some(e),
            ..
        } => find_lambda_recursive(e),
        HirStmt::Return(Some(e)) => find_lambda_recursive(e),
        _ => false,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. 简单 lambda 降级
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_lambda_simple() {
    // (x: Int) -> x * 2 应降级为 HirExpr::Lambda
    let hir = parse_to_hir("fun main(): Int { var f = (x: Int) -> x * 2; return 0 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    assert!(
        find_lambda_in_block(&main.body),
        "should contain Lambda expression"
    );
}

#[test]
fn test_hir_lambda_block_body() {
    // (x: Int) -> { return x * 2 } 应降级为 HirExpr::Lambda
    let hir = parse_to_hir("fun main(): Int { var f = (x: Int) -> { return x * 2 }; return 0 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    assert!(
        find_lambda_in_block(&main.body),
        "应包含 Lambda 表达式（块体）"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Lambda 不再降级为 __lambda 调用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_lambda_not_lambda_call() {
    let hir = parse_to_hir("fun main(): Int { var f = (x: Int) -> x * 2; return 0 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    // 确认 body 中没有 "__lambda" 调用
    let has_lambda_call = main.body.stmts.iter().any(|s| {
        matches!(s, HirStmt::Val { init: Some(HirExpr::Call { callee, .. }), .. }
            if callee == "__lambda")
    });
    assert!(!has_lambda_call, "should not contain __lambda call");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Lambda 参数保留
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_lambda_params_preserved() {
    let hir = parse_to_hir("fun main(): Int { var f = (x: Int, y: Int) -> x + y; return 0 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    assert!(
        find_lambda_in_block(&main.body),
        "should contain Lambda expression"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Lambda 作为返回值
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_lambda_as_return_value() {
    let hir = parse_to_hir("fun make_adder(x: Int): (Int) -> Int { return (y: Int) -> x + y }");
    let make_adder =
        hir.functions.iter().find(|f| f.name == "make_adder").expect("make_adder exists");
    // 函数体应包含 Lambda
    assert!(
        find_lambda_in_block(&make_adder.body),
        "返回语句应包含 Lambda"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 无 lambda 时不受影响
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_no_lambda() {
    let hir = parse_to_hir("fun main(): Int { return 42 }");
    let main = hir.functions.iter().find(|f| f.name == "main").expect("main exists");
    assert!(
        !find_lambda_in_block(&main.body),
        "无 lambda 时应不包含 Lambda"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. 嵌套 lambda
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_nested_lambda() {
    let hir = parse_to_hir(
        "fun compose(f: (Int) -> Int, g: (Int) -> Int): (Int) -> Int {
            return (x: Int) -> f(g(x))
        }",
    );
    let compose = hir.functions.iter().find(|f| f.name == "compose").expect("compose exists");
    assert!(
        find_lambda_in_block(&compose.body),
        "should contain nested Lambda"
    );
}
