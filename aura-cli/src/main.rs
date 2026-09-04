//! Aura 语言命令行工具
//!
//! 子命令：
//! - `aura build <file.aura> [--output <out.auc>]`  编译为字节码 `.auc`
//! - `aura build <file.aura> --aot [--output <exe>]`  AOT 编译为原生可执行文件
//! - `aura run <file.aura>`                          编译并执行（预留，依赖 VM）
//! - `aura check <file.aura>`                        仅做语法/语义检查
//! - `aura disasm <file.auc> [--source <file.aura>]` 反汇编 `.auc` 为可读汇编
//! - `aura tokens <file.aura>`                       输出词法分析
//! - `aura ast <file.aura>`                          输出 AST（调试）
//! - `aura fmt <file.aura>`                          代码格式化（预留）

use std::process::exit;

use aura_compiler::codegen::{
    SerializeError, compile_source, disassemble, read_auc, to_bytes, write_auc,
};
use aura_compiler::lexer::Lexer;
use aura_compiler::parser::Parser;
use aura_compiler::sema::analyze_source;
use aura_compiler::vm::{Vm, VmOptions};

#[cfg(feature = "llvm")]
use aura_compiler::codegen::aot::{AotOptions, OptimizationLevel, TargetTriple};

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
        "leak-check" => cmd_leak_check(rest),
        "doc" => cmd_doc(rest),
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
  aura build <file.aura> [--output <out>]        编译为字节码 .auc / 原生可执行文件\n\
  aura build <file.aura> --aot [--output <exe>]  AOT 编译为原生可执行文件\n\
    [--target <triple>]   目标三元组（如 aarch64-unknown-linux-gnu）\n\
    [--opt <level>]       优化级别（0/1/2/3/s/z，默认 2）\n\
    [--emit-llvm]         仅生成 LLVM IR（.ll）\n\
    [--debug]             生成 DWARF 调试信息\n\
  aura run <file.aura>                          编译并执行（依赖 VM）\n\
  aura check <file.aura>                        仅做语法/语义检查\n\
  aura disasm <file.auc> [--source <f.aura>]    反汇编 .auc 为可读汇编\n\
  aura tokens <file.aura>                       输出词法分析\n\
  aura ast <file.aura>                          输出 AST\n\
  aura fmt <file.aura>                          代码格式化（预留）\n\
  aura leak-check <file.aura>                    P7: 内存泄漏检测（ARC 分析）\n\
  aura doc [--output <dir>]                       生成标准库 API 文档（Markdown + HTML）\n"
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
    args.iter()
        .find(|a| a.as_str() != skip && !a.starts_with("--"))
}

fn cmd_build(args: &[String]) {
    // AOT 模式（--aot）
    if args.iter().any(|a| a == "--aot") {
        #[cfg(feature = "llvm")]
        {
            cmd_build_aot(args);
            return;
        }
        #[cfg(not(feature = "llvm"))]
        {
            eprintln!("错误: llvm feature 未启用，无法进行 AOT 编译");
            eprintln!("提示: 使用 `cargo build --features llvm` 重新构建 aura-compiler");
            exit(1);
        }
    }

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

/// AOT 编译（LLVM 后端）
#[cfg(feature = "llvm")]
fn cmd_build_aot(args: &[String]) {
    let input = match first_positional(args, "--aot") {
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

    // 目标三元组
    let target_str = extract_opt(args, "--target");
    let target = match target_str {
        Some(t) => match TargetTriple::from_str(&t) {
            Some(tt) => tt,
            None => {
                eprintln!("错误: 不支持的目标三元组: {}", t);
                eprintln!(
                    "支持格式: x86_64-pc-windows-msvc / aarch64-unknown-linux-gnu / armv7-unknown-linux-gnueabihf"
                );
                exit(1);
            }
        },
        None => TargetTriple::default(),
    };
    let is_windows_target = {
        use aura_compiler::codegen::aot::OperatingSystem;
        target.os == OperatingSystem::Windows
    };

    // 优化级别
    let opt_str = extract_opt(args, "--opt");
    let opt_level = match opt_str {
        Some(s) => match OptimizationLevel::from_str(&s) {
            Some(l) => l,
            None => {
                eprintln!("错误: 无效的优化级别: {}（支持 0/1/2/3/s/z）", s);
                exit(1);
            }
        },
        None => OptimizationLevel::default(),
    };

    // 输出格式
    let emit_llvm = args.iter().any(|a| a == "--emit-llvm");

    // 调试信息（DWARF 元数据）
    let debug_enabled = args.iter().any(|a| a == "--debug");

    let mut options = AotOptions {
        target,
        opt_level,
        debug_info: debug_enabled,
        ..Default::default()
    };

    // 默认使用宿主 LLVM 安装（可从环境变量获取）
    if let Ok(home) = std::env::var("AURA_LLVM_HOME") {
        options.llvm_home = Some(home.into());
    }

    let out_path = extract_opt(args, "--output")
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| {
            let exe_name = if emit_llvm {
                format!("{}.ll", default_output_base(input))
            } else {
                format!(
                    "{}{}",
                    default_output_base(input),
                    if is_windows_target { ".exe" } else { "" }
                )
            };
            std::path::PathBuf::from(exe_name)
        });

    // 创建输出目录（如有）
    if let Some(dir) = out_path.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }

    // 解析并生成 LLVM IR
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        eprintln!("错误: [词法] {}", e.message);
        exit(1);
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("错误: [语法] {}", e.message);
        exit(1);
    }

    let hir = aura_compiler::codegen::hir::desugar_program(&program);
    let codegen = aura_compiler::codegen::aot::AotCodeGenerator::new(options.clone());
    let ir = match codegen.generate_ir(&hir) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("错误: AOT IR 生成失败: {}", e);
            exit(1);
        }
    };

    if emit_llvm {
        // 仅输出 LLVM IR
        if let Err(e) = std::fs::write(&out_path, &ir) {
            eprintln!("错误: 无法写入 {}: {}", out_path.display(), e);
            exit(1);
        }
        println!("✓ AOT 编译完成（LLVM IR）: {}", out_path.display());
        return;
    }

    // 完整 AOT：写 .ll → llc → link
    // 在临时目录生成中间产物
    let tmp_dir = std::env::temp_dir().join(format!("aura_aot_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let ll_path = tmp_dir.join("module.ll");
    if let Err(e) = std::fs::write(&ll_path, &ir) {
        eprintln!("错误: 无法写入临时 IR: {}", e);
        exit(1);
    }

    match finish_executable(&ll_path, &out_path, &options) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            println!("✓ AOT 编译完成: {}", out_path.display());
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            eprintln!("错误: AOT 编译失败: {}", e);
            exit(1);
        }
    }
}

/// 完成可执行文件生成（llc + link）
#[cfg(feature = "llvm")]
fn finish_executable(
    ll_path: &std::path::Path,
    exe_path: &std::path::Path,
    options: &AotOptions,
) -> Result<(), String> {
    use aura_compiler::codegen::aot::linker::{link_to_executable, link_to_object};

    // 中间对象文件
    let obj_path = ll_path.with_extension(if cfg!(target_os = "windows") {
        "obj"
    } else {
        "o"
    });

    link_to_object(ll_path, &obj_path, options).map_err(|e| e.to_string())?;

    link_to_executable(&obj_path, exe_path, options).map_err(|e| e.to_string())?;

    Ok(())
}

/// 输出文件基名（去掉扩展名）
#[cfg(feature = "llvm")]
fn default_output_base(input: &str) -> String {
    let base = input.trim_end_matches(".aura").trim_end_matches(".AURA");
    base.to_string()
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

fn cmd_run(args: &[String]) {
    let use_jit = args.iter().any(|a| a == "--jit");
    let input = first_positional(args, "--jit").or_else(|| first_positional(args, "--output"));

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            exit(1);
        }
    };

    // 加载字节码：`.auc` 直接读取，`.aura` 先编译
    let module = if input.ends_with(".auc") {
        match read_auc(input) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("错误: 无法读取字节码 {}: {}", input, e);
                exit(1);
            }
        }
    } else {
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("错误: 无法读取 {}: {}", input, e);
                exit(1);
            }
        };
        match compile_source(&source) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("编译失败:\n{}", e);
                exit(1);
            }
        }
    };

    let opts = VmOptions {
        jit: use_jit,
        ..Default::default()
    };
    let mut vm = match Vm::new(&module, opts) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("VM 初始化失败: {}", e);
            exit(1);
        }
    };

    match vm.run() {
        Ok(result) => {
            if !matches!(result, aura_compiler::vm::Value::Null) {
                println!("{}", result);
            }
        }
        Err(e) => {
            eprintln!("运行时错误: {}", e);
            exit(1);
        }
    }
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

/// P7.9: 内存泄漏检测命令
fn cmd_leak_check(args: &[String]) {
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

    use aura_compiler::codegen::hir::desugar_program;
    use aura_compiler::codegen::mir::lower_program;

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    if !parser.errors().is_empty() {
        for e in parser.errors() {
            eprintln!("[语法] {}", e.message);
        }
        exit(1);
    }

    let hir = desugar_program(&program);
    let (mut mir_funcs, _ctx) = lower_program(&hir);

    // 运行完整 ARC 分析
    let result = aura_compiler::codegen::arc::run_arc_analysis(&mut mir_funcs);

    println!("=== ARC 分析报告 ===");
    println!("{}", result.summary());
    println!();

    if let Some((name, info)) = result.escape_info.iter().next() {
        println!("--- 逃逸分析: {} ---", name);
        println!("  逃逸分配: {}", info.escaping_allocs.len());
        println!("  非逃逸分配: {}", info.non_escaping_allocs.len());
    }

    println!();
    println!("--- ARC 插入统计 ---");
    println!("  Retain 插入: {}", result.insertion_stats.retains);
    println!("  Release 插入: {}", result.insertion_stats.releases);
    println!();
    println!("--- ARC 优化统计 ---");
    println!(
        "  消除 Retain: {}",
        result.optimization_stats.eliminated_retains
    );
    println!(
        "  消除 Release: {}",
        result.optimization_stats.eliminated_releases
    );

    println!();
    if result.leak_report.is_clean() {
        println!("✅ 内存泄漏检测: 无泄漏");
    } else {
        println!(
            "⚠️  内存泄漏检测: 发现 {} 个潜在泄漏",
            result.leak_report.leaked_allocs
        );
        for d in &result.leak_report.details {
            println!("  - [{}] {}", d.function, d.description);
        }
    }
}

/// P9.11: 生成标准库 API 文档
fn cmd_doc(args: &[String]) {
    let output_dir = extract_opt(args, "--output")
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("docs/api"));

    // 可选：仅生成指定模块
    let module_filter = extract_opt(args, "--module");

    if let Some(module) = &module_filter {
        // 仅生成单个模块文档
        let registry = aura_compiler::docgen::DocRegistry::new().load_all();
        let docs = registry.by_module(module);
        if docs.is_empty() {
            eprintln!("错误: 模块 '{}' 不存在或无文档", module);
            eprintln!("可用模块:");
            for m in registry.module_names() {
                println!("  {}", m);
            }
            exit(1);
        }
        let content = aura_compiler::docgen::render_module_markdown(&registry, module);
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| {
                eprintln!("错误: 创建输出目录失败: {}", e);
                exit(1);
            })
            .ok();
        let file_path = output_dir.join(format!("std_{}.md", module));
        std::fs::write(&file_path, &content)
            .map_err(|e| {
                eprintln!("错误: 写入 {} 失败: {}", file_path.display(), e);
                exit(1);
            })
            .ok();
        println!("✓ 已生成模块文档: {}", file_path.display());
        println!("  函数数: {}", docs.len());
        return;
    }

    // 生成完整文档（Markdown + HTML）
    match aura_compiler::docgen::generate_docs(&output_dir) {
        Ok(files) => {
            println!("✓ 已生成 {} 个文档文件:", files.len());
            for f in &files {
                println!("  {}", f.display());
            }
        }
        Err(e) => {
            eprintln!("错误: 文档生成失败: {}", e);
            exit(1);
        }
    }

    // 同时生成 HTML 版本
    let registry = aura_compiler::docgen::DocRegistry::new().load_all();
    let html = aura_compiler::docgen::render_html(&registry);
    let html_path = output_dir.join("index.html");
    if let Err(e) = std::fs::write(&html_path, &html) {
        eprintln!("警告: 无法写入 HTML 文档 {}: {}", html_path.display(), e);
    } else {
        println!("  ✓ HTML 文档: {}", html_path.display());
    }
}
