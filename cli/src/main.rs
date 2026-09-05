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

use compiler::codegen::{
    SerializeError, compile_source, disassemble, read_auc, to_bytes, write_auc,
};
use compiler::lexer::Lexer;
use compiler::parser::Parser;
use compiler::sema::analyze_source;
use compiler::vm::{Vm, VmOptions};

#[cfg(feature = "llvm")]
use compiler::codegen::aot::{AotOptions, OptimizationLevel, TargetTriple};

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
        "eval" => cmd_eval(rest),
        "repl" => cmd_repl(rest),
        // P11: 包管理器命令
        "install" => cmd_install(rest),
        "update" => cmd_update(rest),
        "publish" => cmd_publish(rest),
        "deps" => cmd_deps(rest),
        "new" => cmd_new(rest),
        // Phase 1: .auz 制品格式命令
        "package" => cmd_package(rest),
        "inspect" => cmd_inspect(rest),
        "verify" => cmd_verify(rest),
        // P13: 工具链命令
        "lsp" => cmd_lsp(rest),
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
  aura build <file.aura> --lib [--output <out>]   打包为 .auz 库制品（等价于 aura package）\n\
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
  aura doc [--output <dir>]                       生成标准库 API 文档（Markdown + HTML）\n\
  aura eval [--expr <code>]                      执行代码片段（类 node -e）\n\
  aura repl                                     交互式 REPL（类 python -i）\n\
  aura install [--offline]                        P11: 安装依赖（aura.toml + // @depends）\n\
  aura update [--all]                             P11: 更新依赖到最新兼容版本\n\
  aura publish [--dir <path>]                     P11: 发布包到 Git 仓库\n\
  aura deps [--dir <path>]                        P11: 显示依赖树\n\
  aura new <name> [--dir <path>]                  P11: 创建新包项目\n\
  aura package <file.aura> [--output <out>]         Phase 1: 打包为 .auz 制品\n\
  aura inspect <file.auz>                         Phase 1: 检查 .auz 内容\n\
  aura verify <file.auz>                          Phase 1: 验证 .auz 校验和\n\
  aura lsp                                        P13: 启动 LSP 服务器（stdio 通信）\n\
  aura fmt <file.aura> [--check]                  P13: 代码格式化\n"
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
    // Phase 1: --lib 标志 → 打包为 .auz 库制品（等价于 aura package）
    if args.iter().any(|a| a == "--lib") {
        cmd_package(args);
        return;
    }

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
            eprintln!("提示: 使用 `cargo build --features llvm` 重新构建 compiler");
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
        use compiler::codegen::aot::OperatingSystem;
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

    let hir = compiler::codegen::hir::desugar_program(&program);
    let codegen = compiler::codegen::aot::AotCodeGenerator::new(options.clone());
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
    use compiler::codegen::aot::linker::{link_to_executable, link_to_object};

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
            if !matches!(result, compiler::vm::Value::Null) {
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

fn cmd_fmt(args: &[String]) {
    let check_only = args.iter().any(|a| a == "--check");
    let input = match first_positional(args, "--check") {
        Some(p) => p,
        None => {
            eprintln!("错误: 缺少输入文件");
            eprintln!("用法: aura fmt <file.aura> [--check]");
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

    let formatted = compiler::lsp::format_source(&source);

    if check_only {
        if formatted != source {
            println!("{} 需要格式化", input);
            exit(1);
        } else {
            println!("✓ {} 已格式化", input);
        }
    } else {
        std::fs::write(input, &formatted).map_err(|e| {
            eprintln!("错误: 写入 {} 失败: {}", input, e);
            exit(1);
        }).ok();
        println!("✓ 已格式化 {}", input);
    }
}

/// P13.3: `aura lsp` — 启动 LSP 服务器
fn cmd_lsp(_args: &[String]) {
    eprintln!("Aura LSP 服务器启动（stdio 模式）");
    compiler::lsp::run_lsp_server();
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

    use compiler::codegen::hir::desugar_program;
    use compiler::codegen::mir::lower_program;

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
    let result = compiler::codegen::arc::run_arc_analysis(&mut mir_funcs);

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
        let registry = compiler::docgen::DocRegistry::new().load_all();
        let docs = registry.by_module(module);
        if docs.is_empty() {
            eprintln!("错误: 模块 '{}' 不存在或无文档", module);
            eprintln!("可用模块:");
            for m in registry.module_names() {
                println!("  {}", m);
            }
            exit(1);
        }
        let content = compiler::docgen::render_module_markdown(&registry, module);
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
    match compiler::docgen::generate_docs(&output_dir) {
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
    let registry = compiler::docgen::DocRegistry::new().load_all();
    let html = compiler::docgen::render_html(&registry);
    let html_path = output_dir.join("index.html");
    if let Err(e) = std::fs::write(&html_path, &html) {
        eprintln!("警告: 无法写入 HTML 文档 {}: {}", html_path.display(), e);
    } else {
        println!("  ✓ HTML 文档: {}", html_path.display());
    }
}

/// `aura eval` — 执行代码片段（类 node -e / python -c）
///
/// 用法：
///   aura eval --expr "println('hello')"
///   echo "println('hello')" | aura eval
fn cmd_eval(args: &[String]) {
    use std::io::{self, Read};

    let code = extract_opt(args, "--expr")
        .or_else(|| first_positional(args, "--expr").map(|s| s.clone()));

    let code = match code {
        Some(c) => c,
        None => {
            // 从 stdin 读取
            let mut input = String::new();
            match io::stdin().read_to_string(&mut input) {
                Ok(_) => input,
                Err(e) => {
                    eprintln!("错误: 无法读取 stdin: {}", e);
                    exit(1);
                }
            }
        }
    };

    if code.trim().is_empty() {
        eprintln!("错误: 代码为空");
        exit(1);
    }

    let module = match compile_source(&code) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("编译失败:\n{}", e);
            exit(1);
        }
    };

    let opts = VmOptions::default();
    let mut vm = match Vm::new(&module, opts) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("VM 初始化失败: {}", e);
            exit(1);
        }
    };

    match vm.run() {
        Ok(result) => {
            if !matches!(result, compiler::vm::Value::Null) {
                println!("{}", result);
            }
        }
        Err(e) => {
            eprintln!("运行时错误: {}", e);
            exit(1);
        }
    }
}

/// `aura repl` — 交互式 REPL（类 python -i / node -i）
///
/// 多行输入支持：当行末为 `{`、`,`、`(` 等时自动续行。
fn cmd_repl(_args: &[String]) {
    use std::io::{self, BufRead, Write};

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    let mut buffer = String::new();
    let mut depth: i32 = 0;

    println!("Aura REPL — 输入代码按回车执行，多行以 {{ 或 ( 续行。退出：exit 或 Ctrl+D");

    loop {
        let prompt = if buffer.is_empty() { ">>> " } else { "... " };
        eprint!("{}", prompt);
        io::stdout().flush().ok();

        let line = match lines.next() {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                eprintln!("读取输入失败: {}", e);
                break;
            }
            None => {
                // EOF (Ctrl+D)
                println!("\n再见!");
                break;
            }
        };

        let trimmed = line.trim();

        // 退出命令
        if trimmed == "exit" || trimmed == "quit" {
            println!("再见!");
            break;
        }

        // 空白输入
        if trimmed.is_empty() {
            if !buffer.is_empty() {
                // 提交之前累积的代码
                let code = std::mem::take(&mut buffer);
                depth = 0;
                repl_eval(&code);
            }
            continue;
        }

        // 跟踪括号深度
        for ch in trimmed.chars() {
            match ch {
                '{' | '(' | '[' => depth += 1,
                '}' | ')' | ']' => depth -= 1,
                _ => {}
            }
        }

        // 续行（括号未闭合或行尾为续行符）
        if depth > 0 || trimmed.ends_with(',') || trimmed.ends_with('.') {
            buffer.push_str(&line);
            buffer.push('\n');
            continue;
        }

        // 单行：直接执行
        buffer.push_str(&line);
        let code = std::mem::take(&mut buffer);
        depth = 0;
        repl_eval(&code);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// P11: 包管理器命令
// ─────────────────────────────────────────────────────────────────────────────

/// P11.4: `aura install` — 安装依赖
fn cmd_install(args: &[String]) {
    let offline = args.iter().any(|a| a == "--offline");
    let dir_opt = extract_opt(args, "--dir");
    let project_dir = dir_opt
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    use compiler::package::{
        PackageManager, PackageManifest,
    };

    // 收集所有依赖
    let mut deps = Vec::new();

    // 从 aura.toml 加载
    let manifest_path = project_dir.join("aura.toml");
    if manifest_path.exists() {
        match PackageManifest::from_toml_file(&manifest_path) {
            Ok(manifest) => {
                deps.extend(manifest.dependencies);
            }
            Err(e) => {
                eprintln!("警告: 无法解析 aura.toml: {}", e);
            }
        }
    }

    // 从 .aura 文件中的 // @depends 收集
    if let Ok(entries) = std::fs::read_dir(&project_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "aura").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    deps.extend(compiler::package::parse_depends(&content));
                }
            }
        }
    }

    if deps.is_empty() {
        println!("✓ 无依赖需要安装");
        return;
    }

    let config = compiler::package::PackageManagerConfig {
        offline,
        ..Default::default()
    };
    let mut pm = PackageManager::with_config(config);

    println!("正在安装 {} 个依赖...", deps.len());
    match pm.install(&project_dir, &deps) {
        Ok(lock) => {
            println!("✓ 已安装 {} 个依赖", lock.dependencies.len());
            for entry in &lock.dependencies {
                println!(
                    "  ✓ {} v{} (rev: {})",
                    entry.name,
                    entry.version,
                    entry.rev.as_deref().unwrap_or("-")
                );
            }
        }
        Err(e) => {
            eprintln!("安装失败: {}", e);
            exit(1);
        }
    }
}

/// P11.5: `aura update` — 更新依赖
fn cmd_update(args: &[String]) {
    let all = args.iter().any(|a| a == "--all");
    let dir_opt = extract_opt(args, "--dir");
    let project_dir = dir_opt
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    use compiler::package::PackageManager;

    let mut pm = PackageManager::new();
    if let Err(e) = pm.load_project(&project_dir) {
        eprintln!("加载项目失败: {}", e);
        exit(1);
    }

    match pm.update(&project_dir, all) {
        Ok(updated) => {
            if updated.is_empty() {
                println!("✓ 所有依赖已是最新");
            } else {
                println!("✓ 已更新 {} 个依赖:", updated.len());
                for u in &updated {
                    println!("  {}", u);
                }
            }
        }
        Err(e) => {
            eprintln!("更新失败: {}", e);
            exit(1);
        }
    }
}

/// P11.6: `aura publish` — 发布包
fn cmd_publish(args: &[String]) {
    let dir_opt = extract_opt(args, "--dir");
    let package_dir = dir_opt
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    use compiler::package::PackageManager;

    let pm = PackageManager::new();
    match pm.publish(&package_dir) {
        Ok(msg) => println!("✓ {}", msg),
        Err(e) => {
            eprintln!("发布失败: {}", e);
            exit(1);
        }
    }
}

/// P11.7: `aura deps` — 显示依赖树
fn cmd_deps(args: &[String]) {
    let dir_opt = extract_opt(args, "--dir");
    let project_dir = dir_opt
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let outdated = args.iter().any(|a| a == "--outdated");

    use compiler::package::PackageManager;

    let mut pm = PackageManager::new();
    if let Err(e) = pm.load_project(&project_dir) {
        eprintln!("加载项目失败: {}", e);
        exit(1);
    }

    match pm.show_deps(&project_dir) {
        Ok(tree) => {
            if outdated {
                println!("=== 过时依赖 ===");
                // 简化：标记所有依赖
                println!("运行 aura update --all 来更新所有依赖");
            }
            println!("{}", tree);
        }
        Err(e) => {
            eprintln!("显示依赖失败: {}", e);
            exit(1);
        }
    }
}

/// P11: `aura new` — 创建新包项目
fn cmd_new(args: &[String]) {
    let name = match first_positional(args, "--dir") {
        Some(n) => n.clone(),
        None => {
            eprintln!("错误: 缺少包名");
            eprintln!("用法: aura new <name> [--dir <path>]");
            exit(1);
        }
    };

    let dir_opt = extract_opt(args, "--dir");
    let parent_dir = dir_opt
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    use compiler::package::PackageManager;

    match PackageManager::create_new_package(&name, &parent_dir) {
        Ok(()) => {
            println!("✓ 已创建新包项目: {}", parent_dir.join(&name).display());
            println!("  下一步:");
            println!("    cd {}", name);
            println!("    aura run main.aura");
            println!("    aura publish");
        }
        Err(e) => {
            eprintln!("创建失败: {}", e);
            exit(1);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1: .auz 制品格式命令
// ─────────────────────────────────────────────────────────────────────────────

/// Phase 1: `aura package` — 打包为 .auz 制品
fn cmd_package(args: &[String]) {
    use compiler::auz::{PackageBuilder, PackageBuildOptions};
    use compiler::package::PackageManifest;

    // 解析参数
    let output = extract_opt(args, "--output");
    let input = match first_positional(args, "--output") {
        Some(p) => p.clone(),
        None => {
            eprintln!("错误: 缺少输入文件");
            eprintln!("用法: aura package <file.aura> [--output <out.auz>] [--sources]");
            exit(1);
        }
    };

    let include_sources = args.iter().any(|a| a == "--sources");

    // 读取并编译源码
    let source = match std::fs::read_to_string(&input) {
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

    // 尝试从项目目录加载 aura.toml，否则生成默认清单
    let source_path = std::path::Path::new(&input);
    let project_dir = source_path.parent().unwrap_or(std::path::Path::new("."));
    let manifest_path = project_dir.join("aura.toml");

    let manifest = if manifest_path.exists() {
        match PackageManifest::from_toml_file(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("警告: 无法解析 aura.toml: {}，使用默认清单", e);
                default_manifest(&input, project_dir)
            }
        }
    } else {
        default_manifest(&input, project_dir)
    };

    // 构建打包选项
    let options = PackageBuildOptions {
        include_sources,
        include_ref_index: true,
        compression_level: compiler::auz::DEFAULT_COMPRESSION_LEVEL,
        ..Default::default()
    };

    // 创建构建器
    let mut builder = PackageBuilder::new(&manifest, &module).with_options(options);

    // 如果有源码目录，包含源码
    if include_sources {
        builder = builder.with_source_dir(project_dir);
    }

    // 确定输出路径
    let out_path = output
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| {
            let base = input.trim_end_matches(".aura").trim_end_matches(".AURA");
            std::path::PathBuf::from(format!("{}.auz", base))
        });

    // 执行打包
    match builder.build(&out_path) {
        Ok(result) => {
            println!("{}", result.summary());
            println!("  包类型: {}", manifest.kind);
            if manifest.library {
                println!("  库包: true");
            }
            println!("  文件列表:");
            for entry in &result.checksum_entries {
                println!("    {}", entry.path);
            }
        }
        Err(e) => {
            eprintln!("打包失败: {}", e);
            exit(1);
        }
    }
}

/// Phase 1: `aura inspect` — 检查 .auz 内容
fn cmd_inspect(args: &[String]) {
    use compiler::auz::PackageReader;

    let input = match first_positional(args, "--verbose") {
        Some(p) => p.clone(),
        None => {
            eprintln!("错误: 缺少输入文件");
            eprintln!("用法: aura inspect <file.auz>");
            exit(1);
        }
    };

    let verbose = args.iter().any(|a| a == "--verbose");

    match PackageReader::from_file(&std::path::PathBuf::from(&input)) {
        Ok(content) => {
            println!("=== .auz 包信息 ===");
            println!("名称:     {}", content.manifest.name);
            println!("版本:     {}", content.manifest.version);
            println!("类型:     {}", content.manifest.kind);
            println!("库包:     {}", content.manifest.library);
            if let Some(desc) = &content.manifest.description {
                println!("描述:     {}", desc);
            }
            if let Some(license) = &content.manifest.license {
                println!("许可证:   {}", license);
            }
            if let Some(min_ver) = &content.manifest.compiler_min_version {
                println!("最低编译器: >= {}", min_ver);
            }
            if !content.manifest.exports.is_empty() {
                println!("导出:     {}", content.manifest.exports.join(", "));
            }
            println!();
            println!("=== 文件列表 ({} 个) ===", content.files.len());
            for (path, data) in &content.files {
                let size = data.len();
                println!("  {:6}  {}", size, path);
            }

            // 字节码模块信息
            if let Some(module) = &content.module {
                println!();
                println!("=== 字节码模块 ===");
                println!("  函数数:   {}", module.functions.len());
                println!("  常量数:   {}", module.consts.len());
                println!("  原生函数: {}", module.natives.len());
                if !module.enabled_modules.is_empty() {
                    println!("  启用模块: {}", module.enabled_modules.join(", "));
                }
            }

            if verbose {
                println!();
                println!("=== 校验和条目 ===");
                for entry in &content.checksum_entries {
                    println!("  {}  {}", entry.hash, entry.path);
                }
            }
        }
        Err(e) => {
            eprintln!("检查失败: {}", e);
            exit(1);
        }
    }
}

/// Phase 1: `aura verify` — 验证 .auz 校验和
fn cmd_verify(args: &[String]) {
    use compiler::auz::PackageReader;

    let input = match first_positional(args, "") {
        Some(p) => p.clone(),
        None => {
            eprintln!("错误: 缺少输入文件");
            eprintln!("用法: aura verify <file.auz>");
            exit(1);
        }
    };

    match PackageReader::verify(&std::path::PathBuf::from(&input)) {
        Ok(result) => {
            println!("{}", result.report());
            if !result.is_valid() {
                exit(1);
            }
        }
        Err(e) => {
            eprintln!("验证失败: {}", e);
            exit(1);
        }
    }
}

/// 生成默认包清单（当 aura.toml 不存在时）
fn default_manifest(
    input: &str,
    _project_dir: &std::path::Path,
) -> compiler::package::PackageManifest {
    use compiler::package::{PackageKind, PackageManifest, PackageOptions, ResourceConfig};

    // 从输入文件名推导包名
    let file_name = std::path::Path::new(input)
        .file_name()
        .map(|s| s.to_string_lossy().replace(".aura", "").replace(".AURA", ""))
        .unwrap_or_else(|| "app".to_string());

    PackageManifest {
        schema_version: "1.0".to_string(),
        name: file_name,
        version: "0.1.0".to_string(),
        description: None,
        authors: vec![],
        license: None,
        repository: None,
        entry: "main.aura".to_string(),
        dependencies: vec![],
        dev_dependencies: vec![],
        exports: vec![],
        platforms: vec![],
        library: false,
        kind: PackageKind::Bytecode,
        compiler_min_version: Some("0.3.0".to_string()),
        compiler_max_version: None,
        package: PackageOptions::default(),
        resources: ResourceConfig::default(),
    }
}

/// REPL 辅助：编译并执行一段代码，打印结果
fn repl_eval(code: &str) {
    let code = code.trim();
    if code.is_empty() {
        return;
    }

    match compile_source(code) {
        Ok(module) => {
            let opts = VmOptions::default();
            match Vm::new(&module, opts) {
                Ok(mut vm) => match vm.run() {
                    Ok(result) => {
                        if !matches!(result, compiler::vm::Value::Null) {
                            println!("{}", result);
                        }
                    }
                    Err(e) => {
                        eprintln!("运行时错误: {}", e);
                    }
                },
                Err(e) => {
                    eprintln!("VM 初始化失败: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("编译失败:\n{}", e);
        }
    }
}
