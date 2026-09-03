//! Aura 命令行工具（P1-P2 阶段：词法/语法前端）
//!
//! 用法：
//!   aura tokens <file>    # 词法分析，打印 Token 流
//!   aura parse <file>     # 语法分析，打印 AST
//!   aura check <file>     # 检查语法错误（解析 + 报告）
//!   aura --help

use aura_compiler::errors::ErrorSeverity;
use aura_compiler::{FileId, Lexer, Parser, SourceMap, TokenKind};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let (cmd, file) = match parse_args(&args) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{}", msg);
            print_usage();
            return ExitCode::from(2);
        }
    };

    let source = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {}", file, e);
            return ExitCode::from(1);
        }
    };

    // 注册源文件，供诊断输出源码片段（SourceMap）
    let mut sm = SourceMap::new();
    let file_id = sm.add_file(file.clone(), source.clone());

    match cmd.as_str() {
        "tokens" => run_tokens(&source, &sm, file_id),
        "parse" => run_parse(&source, &sm, file_id),
        "check" => run_check(&source, &sm, file_id),
        _ => {
            eprintln!("error: unknown command '{}'", cmd);
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn parse_args(args: &[String]) -> Result<(String, String), String> {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        return Err("no command".to_string());
    }
    if args.len() < 2 {
        return Err(format!("missing file argument for '{}'", args[0]));
    }
    Ok((args[0].clone(), args[1].clone()))
}

fn print_usage() {
    eprintln!("\nUsage:\n  aura tokens <file>\n  aura parse <file>\n  aura check <file>");
}

fn run_tokens(source: &str, sm: &SourceMap, file: FileId) -> ExitCode {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();

    for err in lexer.errors() {
        eprintln!(
            "lex error [{}]: {}\n{}",
            err.span,
            err.message,
            sm.snippet(file, &err.span)
        );
    }

    let mut has_error = false;
    for tok in &tokens {
        println!(
            "{:>6}  {:<24} {:?}",
            tok.span.start_line,
            if tok.literal.is_empty() {
                tok.kind.display_name()
            } else {
                tok.literal.as_str()
            },
            tok.kind
        );
        if tok.kind == TokenKind::Error {
            has_error = true;
        }
    }
    println!("{} tokens", tokens.len());
    if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_parse(source: &str, sm: &SourceMap, file: FileId) -> ExitCode {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    for err in lexer.errors() {
        eprintln!(
            "lex error [{}]: {}\n{}",
            err.span,
            err.message,
            sm.snippet(file, &err.span)
        );
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    for err in parser.errors() {
        let sev = match err.severity {
            ErrorSeverity::Error => "error",
            ErrorSeverity::Warning => "warning",
            ErrorSeverity::Info => "info",
        };
        eprintln!(
            "parse {:<7} {}\n{}",
            sev,
            err.message,
            sm.snippet(file, &err.span)
        );
    }

    println!("{:#?}", program);
    println!("\n{} declaration(s)", program.declarations.len());

    if !parser.errors().is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn run_check(source: &str, sm: &SourceMap, file: FileId) -> ExitCode {
    use aura_compiler::sema::analyze_source;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut any_err = false;

    for err in lexer.errors() {
        any_err = true;
        eprintln!(
            "lex     error [{}]: {}\n{}",
            err.span,
            err.message,
            sm.snippet(file, &err.span)
        );
    }

    let mut parser = Parser::new(tokens);
    parser.parse_program();
    for err in parser.errors() {
        any_err = true;
        eprintln!(
            "parse   error [{}]: {}\n{}",
            err.span,
            err.message,
            sm.snippet(file, &err.span)
        );
    }

    // 语义分析（P3）
    let (_program, sema_result) = analyze_source(source);
    for err in &sema_result.errors {
        any_err |= err.severity == ErrorSeverity::Error;
        eprintln!("semantic {}", err.render(sm, file));
    }

    if any_err {
        eprintln!("FAIL");
        ExitCode::from(1)
    } else {
        println!("OK");
        ExitCode::SUCCESS
    }
}
