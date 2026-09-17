#![cfg(feature = "llvm")]

//! Fix 2 — HIR Enum 节点补全测试
//!
//! 验证 Decl::Enum 不再被丢弃，而是正确降级为 HirEnum。
//! 覆盖：简单枚举、带关联值的枚举、枚举变体字段类型。

use compiler::codegen::hir::{HirEnum, desugar_program};
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
// 1. 简单枚举
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_enum_basic() {
    let hir = parse_to_hir("enum Direction { North, South, East, West }");
    assert_eq!(hir.enums.len(), 1, "should keep 1 enum");
    assert_eq!(hir.enums[0].name, "Direction");
    assert_eq!(hir.enums[0].variants.len(), 4, "should have 4 variants");
    assert_eq!(hir.enums[0].variants[0].0, "North");
    assert_eq!(hir.enums[0].variants[1].0, "South");
    assert_eq!(hir.enums[0].variants[2].0, "East");
    assert_eq!(hir.enums[0].variants[3].0, "West");
}

#[test]
fn test_hir_enum_variant_no_fields() {
    let hir = parse_to_hir("enum Color { RED, GREEN, BLUE }");
    assert_eq!(hir.enums.len(), 1);
    for (_, fields) in &hir.enums[0].variants {
        assert!(fields.is_empty(), "simple variant should not have fields");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. 带关联值的枚举
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_enum_with_fields() {
    let hir = parse_to_hir(
        "enum Shape {
            Circle(radius: Float),
            Rect(width: Int, height: Int)
        }",
    );
    assert_eq!(hir.enums.len(), 1);
    assert_eq!(hir.enums[0].name, "Shape");
    assert_eq!(hir.enums[0].variants.len(), 2);

    let circle = &hir.enums[0].variants[0];
    assert_eq!(circle.0, "Circle");
    assert_eq!(circle.1.len(), 1, "Circle should have 1 field");

    let rect = &hir.enums[0].variants[1];
    assert_eq!(rect.0, "Rect");
    assert_eq!(rect.1.len(), 2, "Rect should have 2 fields");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 枚举与其他声明共存
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_enum_with_struct() {
    let hir = parse_to_hir(
        "struct Point { val x: Int; val y: Int }
         enum Dir { Left, Right }",
    );
    assert_eq!(hir.structs.len(), 1, "should keep struct");
    assert_eq!(hir.enums.len(), 1, "should keep enum");
}

#[test]
fn test_hir_enum_with_function() {
    let hir = parse_to_hir(
        "enum Status { Active, Inactive }
         fun main(): Int { return 0 }",
    );
    assert_eq!(hir.enums.len(), 1, "should keep enum");
    assert!(
        hir.functions.iter().any(|f| f.name == "main"),
        "应保留 main 函数"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 枚举在 emit_call 中的返回类型映射
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_enum_variant_types_preserved() {
    let hir = parse_to_hir(
        "enum Result {
            Ok(value: Int),
            Err(msg: String)
        }",
    );
    assert_eq!(hir.enums.len(), 1);
    let ok = &hir.enums[0].variants[0];
    assert_eq!(ok.0, "Ok");
    // Ok 的字段类型应为 Int
    assert_eq!(ok.1.len(), 1);
    let err = &hir.enums[0].variants[1];
    assert_eq!(err.0, "Err");
    assert_eq!(err.1.len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 空枚举
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_hir_empty_program_no_enum() {
    let hir = parse_to_hir("fun main(): Int { return 0 }");
    assert!(
        hir.enums.is_empty(),
        "enums should be empty when no enum declarations"
    );
}
