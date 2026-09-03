//! Aura 语言命令行工具
//!
//! 子命令：
//! - `aura build <file.aura> [--output <out.auc>]`  编译为字节码 `.auc`
//! - `aura run <file.aura>`                          编译并执行（预留，依赖 VM）
//! - `aura check <file.aura>`                        仅做语法/语义检查
//! - `aura disasm <file.auc> [--source <file.aura>]` 反汇编 `.auc` 为可读汇编
//! - `aura tokens <file.aura>`                       输出词法分析
//! - `aura ast <file.aura>`                          输出 AST（调试）
//! - `aura fmt <file.aura>`                          代码格式化（预留）

use std::process::exit;

use aura_compiler::codegen::{
    compile_source, disassemble, read_auc, to_bytes, write_auc, SerializeError,
};
use aura_compiler::lexer::Lexer;
use aura_compiler::parser::Parser;
use aura_compiler::sema::analyze_source;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        exit(1);
    }
    let cmd = args[1].as_str();
    let rest = &args[2..];

    match cmd {
        "build" => cmd_build(rest),
        "run" => cmd_run(rest),
        "check" => cmd_check(rest),
        "disasm" => cmd_disasm(rest),
        "tokens" => cmd_tokens(rest),
        "ast" => cmd_ast(rest),
        "fmt" => cmd_fmt(rest),
        "--help" | "-h" | "help" => print_usage(),
        other => {
            eprintln!("未知子命令: {}", other);
            print_usage();
            exit(1);
        }
    }
}

fn print_usage() {
    println!(
        "Aura 语言工具链\n\
\n\
用法:\n\
  aura build <file.aura> [--output <out.auc>]   编译为字节码 .auc\n\
  aura run <file.aura>                          编译并执行（依赖 VM）\n\
  aura check <file.aura>                        仅做语法/语义检查\n\
  aura disasm <file.auc> [--source <f.aura>]    反汇编 .auc 为可读汇编\n\
  aura tokens <file.aura>                       输出词法分析\n\
  aura ast <file.aura>                          输出 AST\n\
  aura fmt <file.aura>                          代码格式化（预留）\n"
    );
}

/// 解析 `--output <path>` / `--source <path>` 选项
fn extract_opt(args: &[String], name: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == name && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

fn first_positional<'a>(args: &'a [String], skip: &'a str) -> Option<&'a String> {
    args.iter().find(|a| a.as_str() != skip && !a.starts_with("--"))
}

fn cmd_build(args: &[String]) {
    let output = extract_opt(args, "--output");
    let input = match first_positional(args, "--output") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };

    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误: 无法读取 {}: {}", input, e);
            exit(1);
        }
    };

    let module = match compile_source(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("编译失败:\n{}", e);
            exit(1);
        }
    };

    let out_path = output.unwrap_or_else(|| default_output(input));
    if let Err(e) = write_auc(&out_path, &module) {
        eprintln!("错误: 写入 {} 失败: {}", out_path, e);
        exit(1);
    }
    println!(
        "已生成 {} ({} 字节, {} 函数, {} 常量)",
        out_path,
        to_bytes(&module).len(),
        module.functions.len(),
        module.consts.len()
    );
}

fn default_output(input: &str) -> String {
    let base = input.trim_end_matches(".aura").trim_end_matches(".AURA");
    format!("{}.auc", base)
}

fn cmd_disasm(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };
    match read_auc(input) {
        Ok(module) => {
            println!("{}", disassemble(&module));
        }
        Err(SerializeError::Format(m)) => {
            eprintln!("反汇编失败（格式错误）: {}", m);
            exit(1);
        }
        Err(SerializeError::Io(m)) => {
            eprintln!("反汇编失败（IO 错误）: {}", m);
            exit(1);
        }
    }
}

fn cmd_run(_args: &[String]) {
    eprintln!("`aura run` 尚未实现（依赖 P5 虚拟机）");
    exit(1);
}

fn cmd_check(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误: 无法读取 {}: {}", input, e);
            exit(1);
        }
    };

    let mut errs = 0;
    // 词法
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    for e in lexer.errors() {
        eprintln!("[词法] {}", e.message);
        errs += 1;
    }
    // 语法
    let mut parser = Parser::new(tokens);
    let _program = parser.parse_program();
    for e in parser.errors() {
        eprintln!("[语法] {}", e.message);
        errs += 1;
    }
    // 语义
    let (_ast, sema) = analyze_source(&source);
    for e in &sema.errors {
        eprintln!("[语义] {}", e.message);
        errs += 1;
    }

    if errs == 0 {
        println!("✓ {} 检查通过", input);
    } else {
        println!("✗ {} 存在 {} 个错误", input, errs);
        exit(1);
    }
}

fn cmd_tokens(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误: 无法读取 {}: {}", input, e);
            exit(1);
        }
    };
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    for t in &tokens {
        println!("{:?}", t.kind);
    }
    if !lexer.errors().is_empty() {
        for e in lexer.errors() {
            eprintln!("[词法] {}", e.message);
        }
        exit(1);
    }
}

fn cmd_ast(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误: 无法读取 {}: {}", input, e);
            exit(1);
        }
    };
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    println!("{:#?}", program);
    if !parser.errors().is_empty() {
        for e in parser.errors() {
            eprintln!("[语法] {}", e.message);
        }
        exit(1);
    }
}

fn cmd_fmt(_args: &[String]) {
    eprintln!("`aura fmt` 尚未实现");
    exit(1);
}
