//! insta 快照测试（P0.4 / P2.18）
//!
//! 覆盖三层输出的快照：
//! - 词法：Token 流（`Kind(literal) @line:col`）
//! - 语法：AST（Debug 格式化）
//! - 语义：诊断信息列表
//!
//! 更新快照：`INSTA_UPDATE=always cargo test`
//! 查看差异：`cargo insta review`（需安装 cargo-insta）

use compiler::sema::analyze_source;
use compiler::{Lexer, Parser, TokenKind};

fn token_dump(src: &str) -> String {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();

    let mut out = String::new();
    for t in &tokens {
        if t.kind == TokenKind::EOF {
            out.push_str("EOF\n");
            break;
        }
        if t.literal.is_empty() {
            out.push_str(&format!(
                "{:?} @{}:{}\n",
                t.kind, t.span.start_line, t.span.start_col
            ));
        } else {
            out.push_str(&format!(
                "{:?}({:?}) @{}:{}\n",
                t.kind, t.literal, t.span.start_line, t.span.start_col
            ));
        }
    }
    for e in lexer.errors() {
        out.push_str(&format!("error: {} @{}\n", e.message, e.span));
    }
    out
}

fn ast_dump(src: &str) -> String {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    let mut out = format!("{:#?}\n", program);
    for e in parser.errors() {
        out.push_str(&format!("error: {} @{}\n", e.message, e.span));
    }
    out
}

fn sema_dump(src: &str) -> String {
    let (_program, result) = analyze_source(src);
    let mut out = String::new();
    for e in &result.errors {
        out.push_str(&format!("{:?}: {} @{}\n", e.severity, e.message, e.span));
    }
    if out.is_empty() {
        out.push_str("no diagnostics\n");
    }
    out
}

// ───────────────────────── 词法快照 ─────────────────────────

#[test]
fn snapshot_lexer_declaration() {
    insta::assert_snapshot!(token_dump("val x: Int = 42"));
}

#[test]
fn snapshot_lexer_function() {
    insta::assert_snapshot!(token_dump("fun add(a: Int, b: Int): Int { return a + b }"));
}

#[test]
fn snapshot_lexer_control_flow() {
    insta::assert_snapshot!(token_dump("for (i in 0..10) { if (i % 2 == 0) continue }"));
}

#[test]
fn snapshot_lexer_null_safety_operators() {
    insta::assert_snapshot!(token_dump("val len = name?.length ?: 0"));
}

#[test]
fn snapshot_lexer_string_interpolation() {
    insta::assert_snapshot!(token_dump("\"hello $name, sum = ${a + b}\""));
}

#[test]
fn snapshot_lexer_raw_string() {
    insta::assert_snapshot!(token_dump("\"\"\"raw\nmulti-line\"\"\""));
}

#[test]
fn snapshot_lexer_doc_comment() {
    insta::assert_snapshot!(token_dump("/// Adds two numbers\nfun add() {}"));
}

#[test]
fn snapshot_lexer_errors() {
    insta::assert_snapshot!(token_dump("val x = @"));
}

// ───────────────────────── 语法快照 ─────────────────────────

#[test]
fn snapshot_parser_function() {
    insta::assert_snapshot!(ast_dump(
        "fun greet(name: String): String { return \"hi $name\" }"
    ));
}

#[test]
fn snapshot_parser_data_struct() {
    insta::assert_snapshot!(ast_dump("struct Player(val id: Int, var name: String)"));
}

#[test]
fn snapshot_parser_when_expression() {
    insta::assert_snapshot!(ast_dump(
        "fun grade(score: Int): String { return when (score) { in 90..100 -> \"A\" else -> \"F\" } }"
    ));
}

#[test]
fn snapshot_parser_enum_and_class() {
    insta::assert_snapshot!(ast_dump(
        "enum Color { RED, GREEN, CUSTOM(val r: Int) }\nsealed class Shape { fun area(): Float }"
    ));
}

#[test]
fn snapshot_parser_generic_function() {
    insta::assert_snapshot!(ast_dump("fun <T : Comparable<T>> max(a: T, b: T): T = a"));
}

#[test]
fn snapshot_parser_doc_comment() {
    insta::assert_snapshot!(ast_dump(
        "/// A point in 2D space\nstruct Point(val x: Int)"
    ));
}

// ───────────────────────── 语义快照 ─────────────────────────

#[test]
fn snapshot_sema_clean_program() {
    insta::assert_snapshot!(sema_dump("fun add(a: Int, b: Int): Int { return a + b }"));
}

#[test]
fn snapshot_sema_type_errors() {
    insta::assert_snapshot!(sema_dump(
        "fun main() {\n    val x: Int = \"oops\"\n    val y = missingVar\n}"
    ));
}

#[test]
fn snapshot_sema_null_safety_violations() {
    insta::assert_snapshot!(sema_dump(
        "fun main() {\n    val s: String? = null\n    val n = s.length\n    val m = s + 1\n}"
    ));
}
