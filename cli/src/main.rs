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
    BytecodeModule, SerializeError, compile_source, disassemble, read_auc, to_bytes, write_auc,
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
        // Phase 3: 标准库预编译
        "stdlib-compile" => cmd_stdlib_compile(rest),
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
        // P15: 调试器命令
        "debug" => cmd_debug(rest),
        // Phase 4.2: C ABI 头文件生成（Tier 2b）
        "export-header" => cmd_export_header(rest),
        "--help" | "-h" | "help" => print_usage(),
        other => {
            eprintln!("Unknown subcommand: {}", other);
            print_usage();
            exit(1);
        }
    }
}

fn print_usage() {
    println!(
        "Aura language toolchain\n\
\n\
Usage:\n\
  aura build <file.aura> [--output <out>]        Compile to bytecode .auc / native executable\n\
  aura build <file.aura> --aot-embed             Compile .auc v4 (embed AOT machine code, mmap at VM load)\n\
  aura build <file.aura> --lib [--output <out>]   Package as .auz library artifact (same as aura package)\n\
  aura build <file.aura> --aot [--output <exe>]  AOT compile to native executable\n\
    [--target <triple>]   Target triple (e.g. aarch64-unknown-linux-gnu)\n\
    [--opt <level>]       Optimization level (0/1/2/3/s/z, default 2)\n\
    [--emit-llvm]         Only generate LLVM IR (.ll)\n\
    [--debug]             Generate DWARF debug information\n\
    [--shared]            Generate shared library (.so / .dylib / .dll), export JitValue ABI wrapper\n\
  aura run <file.aura> [--stdlib-dir <dir>]      Compile and run (optionally load stdlib .auc)\n\
  aura check <file.aura>                        Syntax/semantic check only\n\
  aura disasm <file.auc> [--source <f.aura>]    Disassemble .auc to readable assembly\n\
  aura tokens <file.aura>                       Print lexical analysis\n\
  aura ast <file.aura>                          Print AST\n\
  aura fmt <file.aura>                          Code formatting (reserved)\n\
  aura leak-check <file.aura>                    P7: Memory leak detection (ARC analysis)\n\
  aura doc [--output <dir>]                       Generate stdlib API docs (Markdown + HTML)\n\
  aura eval [--expr <code>]                      Execute code snippet (like node -e)\n\
  aura repl                                     Interactive REPL (like python -i)\n\
  aura install [--offline]                        P11: Install dependencies (aura.toml + // @depends)\n\
  aura update [--all]                             P11: Update dependencies to latest compatible version\n\
  aura publish [--dir <path>]                     P11: Publish package to Git repo\n\
  aura deps [--dir <path>]                        P11: Show dependency tree\n\
  aura new <name> [--dir <path>]                  P11: Create new package project\n\
  aura package <file.aura> [--output <out>]         Phase 1: Package as .auz artifact\n\
  aura inspect <file.auz>                         Phase 1: Inspect .auz content\n\
  aura verify <file.auz>                          Phase 1: Verify .auz checksum\n\
  aura lsp                                        P13: Start LSP server (stdio communication)\n\
  aura debug <file.aura>                          P15: Start debugger (forward to aura-debug)\n\
  aura fmt <file.aura> [--check]                  P13: Code formatting\n\
  aura stdlib-compile <core-dir> [--output <dir>] Phase 3: Pre-compile stdlib .aura → .auc\n"
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
            eprintln!("Error: llvm feature is not enabled, AOT compilation is unavailable");
            eprintln!("Hint: rebuild compiler with `cargo build --features llvm`");
            exit(1);
        }
    }

    let output = extract_opt(args, "--output");
    let input = match first_positional(args, "--output") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };

    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };
    // 预处理：解析 import "xxx.aura" 语句
    let source = compiler::codegen::resolve_aura_imports(&source, Some(input));

    let module = match compile_source(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation failed:\n{}", e);
            exit(1);
        }
    };

    // Phase 1 AOT: --aot-embed → 编译为 .auc v4 并嵌入 AOT 机器码
    let embed = args.iter().any(|a| a == "--aot-embed");
    #[cfg(feature = "llvm")]
    let module = if embed { embed_into_auc(&source, module) } else { module };
    #[cfg(not(feature = "llvm"))]
    let module = {
        if embed {
            eprintln!("Error: llvm feature is not enabled, --aot-embed is unavailable");
            exit(1);
        }
        module
    };

    let out_path = output.unwrap_or_else(|| default_output(input));
    if let Err(e) = write_auc(&out_path, &module) {
        eprintln!("Error: failed to write {}: {}", out_path, e);
        exit(1);
    }
    println!(
        "Generated {} ({} bytes, {} functions, {} constants)",
        out_path,
        to_bytes(&module).len(),
        module.functions.len(),
        module.consts.len()
    );
}

/// `--aot-embed`：编译为 `.auc` v4 并嵌入 AOT 机器码（Phase 1）
///
/// 复用 LLVM AOT 后端：HIR → LLVM IR → 目标文件 → `.text` 机器码 blob，
/// 由 [`embed_aot`] 组装为段表（SEG_MACHINE + SEG_DESC_TABLE）写回字节码模块。
/// 失败时回退纯字节码并告警，保证构建不中断。
#[cfg(feature = "llvm")]
fn embed_into_auc(source: &str, module: BytecodeModule) -> BytecodeModule {
    use compiler::codegen::aot_embed::embed_aot;
    use compiler::codegen::hir::desugar_program;

    // 重新解析得到同源 HIR（AOT IR 生成需要）
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        eprintln!("Error: [lex] {}", e.message);
        exit(1);
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("Error: [syntax] {}", e.message);
        exit(1);
    }
    let mut hir = desugar_program(&program);
    compiler::codegen::hir::synthesize_main_if_missing(&mut hir);

    let options = AotOptions {
        opt_level: OptimizationLevel::default(),
        ..Default::default()
    };
    let tmp_dir = std::env::temp_dir().join(format!("aura_embed_{}", std::process::id()));
    match embed_aot(module.clone(), &hir, options, &tmp_dir) {
        Ok(result) => {
            println!(
                "✓ AOT embed: machine code {} bytes, {} function descriptors",
                result.machine_size, result.desc_count
            );
            result.module
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            eprintln!(
                "Warning: AOT embed failed ({}), falling back to pure bytecode .auc",
                e
            );
            module
        }
    }
}

/// AOT 编译（LLVM 后端）
#[cfg(feature = "llvm")]
fn cmd_build_aot(args: &[String]) {
    let input = match first_positional(args, "--aot") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };

    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };
    // 预处理：解析 import "xxx.aura" 语句
    let source = compiler::codegen::resolve_aura_imports(&source, Some(input));

    // 目标三元组
    let target_str = extract_opt(args, "--target");
    let target = match target_str {
        Some(t) => match TargetTriple::from_str(&t) {
            Some(tt) => tt,
            None => {
                eprintln!("Error: unsupported target triple: {}", t);
                eprintln!(
                    "Supported formats: x86_64-pc-windows-msvc / aarch64-unknown-linux-gnu / armv7-unknown-linux-gnueabihf"
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
    let is_macos_target = {
        use compiler::codegen::aot::OperatingSystem;
        target.os == OperatingSystem::MacOS
    };

    // 优化级别
    let opt_str = extract_opt(args, "--opt");
    let opt_level = match opt_str {
        Some(s) => match OptimizationLevel::from_str(&s) {
            Some(l) => l,
            None => {
                eprintln!(
                    "Error: invalid optimization level: {} (supported: 0/1/2/3/s/z)",
                    s
                );
                exit(1);
            }
        },
        None => OptimizationLevel::default(),
    };

    // 输出格式
    let emit_llvm = args.iter().any(|a| a == "--emit-llvm");

    // 调试信息（DWARF 元数据）
    let debug_enabled = args.iter().any(|a| a == "--debug");

    // Phase 4.1: 动态库模式（Tier 2：.so / .dylib / .dll）
    let shared_flag = args.iter().any(|a| a == "--shared" || a == "--dylib");

    // Phase 4.2: C ABI 包装函数模式（Tier 2b：供外部 C/Python 消费者调用）
    let c_abi_flag = args.iter().any(|a| a == "--cabi");

    let mut options = AotOptions {
        target,
        opt_level,
        debug_info: debug_enabled,
        c_abi: c_abi_flag,
        ..Default::default()
    };

    // 默认使用宿主 LLVM 安装（可从环境变量获取）
    if let Ok(home) = std::env::var("AURA_LLVM_HOME") {
        options.llvm_home = Some(home.into());
    }

    let out_path =
        extract_opt(args, "--output").map(|s| std::path::PathBuf::from(s)).unwrap_or_else(|| {
            let base = default_output_base(input);
            let name = if emit_llvm {
                format!("{}.ll", base)
            } else if shared_flag {
                format!(
                    "{}.{}",
                    base,
                    if is_windows_target {
                        "dll"
                    } else if is_macos_target {
                        "dylib"
                    } else {
                        "so"
                    }
                )
            } else {
                format!("{}{}", base, if is_windows_target { ".exe" } else { "" })
            };
            std::path::PathBuf::from(name)
        });

    // 推断输出格式：--shared 标志或输出扩展名（.so/.dylib/.dll）
    let is_shared = shared_flag
        || out_path
            .extension()
            .map(|e| {
                let s = e.to_string_lossy().to_lowercase();
                s == "so" || s == "dylib" || s == "dll"
            })
            .unwrap_or(false);

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
        eprintln!("Error: [lex] {}", e.message);
        exit(1);
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("Error: [syntax] {}", e.message);
        exit(1);
    }

    // 语义检查（获取表达式类型信息，供 HIR 隐式 toString 降级使用）
    //
    // 与字节码路径 `compile_source` 保持一致：P3 类型检查存在已知局限
    // （泛型实例化未展开、动态 `Any` 传播等），因此语义诊断**只作告警输出，
    // 不阻断 AOT 代码生成**。否则「Aura 编译器自身」这类大量使用动态类型的
    // 程序将无法 AOT 编译，而同样的源码在字节码路径下是可以通过的。
    let (_ast, sema) = compiler::sema::analyze_source(&source);
    let mut hard_count: usize = 0;
    for d in sema.errors.iter() {
        if d.severity == compiler::errors::ErrorSeverity::Error {
            hard_count += 1;
        } else {
            eprintln!("Warning: [sema] {}", d.message);
        }
    }
    if hard_count > 0 {
        eprintln!(
            "Warning: [sema] ignoring {} type diagnostics (P3 type-check limitations, consistent with bytecode path)",
            hard_count
        );
    }

    let mut hir = compiler::codegen::hir::desugar_program_with(&program, Some(&sema.info));
    compiler::codegen::hir::synthesize_main_if_missing(&mut hir);
    let codegen = compiler::codegen::aot::AotCodeGenerator::new(options.clone());
    // Phase 4.1: 动态库模式下生成 JitValue ABI 包装函数（blob_mode = true），
    // 且包装函数以 external linkage 导出（wrapper_exported = true），供 dlsym 查找。
    // Phase 4.2: --cabi 模式下额外生成裸 C ABI 包装函数。
    let ir = match codegen.generate_ir_with_mode(&hir, is_shared, is_shared, options.c_abi) {
        Ok(ir) => ir,
        Err(e) => {
            eprintln!("Error: AOT IR generation failed: {}", e);
            exit(1);
        }
    };

    if emit_llvm {
        // 仅输出 LLVM IR
        if let Err(e) = std::fs::write(&out_path, &ir) {
            eprintln!("Error: failed to write {}: {}", out_path.display(), e);
            exit(1);
        }
        println!(
            "✓ AOT compilation complete (LLVM IR): {}",
            out_path.display()
        );
        return;
    }

    // 完整 AOT：写 .ll → llc → link
    // 在临时目录生成中间产物
    let tmp_dir = std::env::temp_dir().join(format!("aura_aot_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let ll_path = tmp_dir.join("module.ll");
    if let Err(e) = std::fs::write(&ll_path, &ir) {
        eprintln!("Error: failed to write temporary IR: {}", e);
        exit(1);
    }

    match if is_shared {
        finish_shared_library(&ll_path, &out_path, &options)
    } else {
        finish_executable(&ll_path, &out_path, &options)
    } {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let kind = if is_shared { "shared library" } else { "executable" };
            println!(
                "✓ AOT compilation complete ({}): {}",
                kind,
                out_path.display()
            );
        }
        Err(e) => {
            // 调试：设置 AURA_KEEP_AOT_TMP=1 保留临时 IR 便于定位 llc 错误
            if std::env::var("AURA_KEEP_AOT_TMP").is_err() {
                let _ = std::fs::remove_dir_all(&tmp_dir);
            }
            eprintln!("Error: AOT compilation failed: {}", e);
            exit(1);
        }
    }
}

/// 完成动态库生成（llc + link -shared）
///
/// Phase 4.1（Tier 2）：生成 `.so` / `.dylib` / `.dll`，
/// 包装函数以 external linkage 导出，供 VM 或外部宿主通过 `dlsym` 查找调用。
#[cfg(feature = "llvm")]
fn finish_shared_library(
    ll_path: &std::path::Path,
    lib_path: &std::path::Path,
    options: &AotOptions,
) -> Result<(), String> {
    use compiler::codegen::aot::linker::{link_to_object, link_to_shared_library};

    // 中间对象文件
    let obj_path = ll_path.with_extension(if cfg!(target_os = "windows") { "obj" } else { "o" });

    link_to_object(ll_path, &obj_path, options).map_err(|e| e.to_string())?;
    link_to_shared_library(&obj_path, lib_path, options).map_err(|e| e.to_string())?;

    Ok(())
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
    let obj_path = ll_path.with_extension(if cfg!(target_os = "windows") { "obj" } else { "o" });

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
            eprintln!("Error: missing input file");
            exit(1);
        }
    };
    match read_auc(input) {
        Ok(module) => {
            println!("{}", disassemble(&module));
        }
        Err(SerializeError::Format(m)) => {
            eprintln!("Disassembly failed (format error): {}", m);
            exit(1);
        }
        Err(SerializeError::Io(m)) => {
            eprintln!("Disassembly failed (IO error): {}", m);
            exit(1);
        }
    }
}

fn cmd_run(args: &[String]) {
    let use_jit = args.iter().any(|a| a == "--jit");
    let stdlib_dir = extract_opt(args, "--stdlib-dir");

    // 查找第一个非标志参数作为输入文件（跳过 --stdlib-dir 的值）
    let input = {
        let mut iter = args.iter();
        let mut found: Option<&String> = None;
        while let Some(arg) = iter.next() {
            if arg.starts_with("--") {
                // 跳过 --stdlib-dir <value>
                if arg == "--stdlib-dir" {
                    iter.next(); // 跳过值
                }
            } else {
                found = Some(arg);
                break;
            }
        }
        found
    };

    let input = match input {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };

    // 加载字节码：`.auc` 直接读取，`.aura` 先编译
    let module = if input.ends_with(".auc") {
        match read_auc(input) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error: failed to read bytecode {}: {}", input, e);
                exit(1);
            }
        }
    } else {
        let source = match std::fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: failed to read {}: {}", input, e);
                exit(1);
            }
        };
        // 预处理：解析 import "xxx.aura" 语句
        let source = compiler::codegen::resolve_aura_imports(&source, Some(input));
        match compile_source(&source) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Compilation failed:\n{}", e);
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
            eprintln!("VM initialization failed: {}", e);
            exit(1);
        }
    };

    // Phase 3: 加载标准库 .auc 文件（aura-compiled stdlib fallback）
    if let Some(ref stdlib_path) = stdlib_dir {
        let stdlib_p = std::path::Path::new(stdlib_path);
        match vm.load_stdlib_dir(stdlib_p) {
            Ok(count) => {
                eprintln!(
                    "[run] stdlib loaded: {} Aura-compiled functions ready",
                    count
                );
            }
            Err(e) => {
                eprintln!(
                    "[run] stdlib load failed (continuing without stdlib): {}",
                    e
                );
            }
        }
    }

    match vm.run() {
        Ok(result) => {
            if !matches!(result, compiler::vm::Value::Null) {
                println!("{}", result);
            }
            // `Process.exit(code)` 请求的退出码（未被显式请求时保持 0）
            if let Some(code) = vm.requested_exit_code() {
                if code != 0 {
                    exit(code);
                }
            }
        }
        Err(e) => {
            eprintln!("Runtime error: {}", e);
            exit(1);
        }
    }
}

/// Phase 3: 预编译标准库 .aura 文件为 .auc 字节码
///
/// 扫描 `core/aura/lang/std/` 目录下的所有 `.aura` 文件，
/// 编译为 `.auc` 字节码文件，输出到指定目录。
///
/// 用法：
///   aura stdlib-compile <core-dir> [--output <out-dir>]
fn cmd_stdlib_compile(args: &[String]) {
    use compiler::codegen::write_auc;

    let output = extract_opt(args, "--output");
    let input = match first_positional(args, "--output") {
        Some(p) => p,
        None => {
            eprintln!("Usage: aura stdlib-compile <core-dir> [--output <out-dir>]");
            exit(1);
        }
    };

    let core_dir = std::path::Path::new(input);
    if !core_dir.exists() {
        eprintln!(
            "Error: stdlib directory does not exist: {}",
            core_dir.display()
        );
        exit(1);
    }

    let out_dir = output.unwrap_or_else(|| "std-auc".to_string());
    let out_path = std::path::Path::new(&out_dir);

    if let Err(e) = std::fs::create_dir_all(out_path) {
        eprintln!(
            "Error: failed to create output directory {}: {}",
            out_path.display(),
            e
        );
        exit(1);
    }

    let mut success = 0;
    let mut failed = 0;

    // 递归扫描 .aura 文件
    let aura_files = scan_aura_files_recursive(core_dir, core_dir).unwrap_or_default();
    eprintln!("[stdlib-compile] found {} .aura files", aura_files.len());

    for (rel_path, abs_path) in &aura_files {
        let source = match std::fs::read_to_string(abs_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[stdlib-compile] failed to read {}: {}",
                    rel_path.display(),
                    e
                );
                failed += 1;
                continue;
            }
        };

        // 预处理 import 语句
        let source =
            compiler::codegen::resolve_aura_imports(&source, Some(abs_path.to_str().unwrap_or("")));

        match compiler::codegen::compile_source(&source) {
            Ok(module) => {
                let rel_str = rel_path.to_string_lossy();
                let out_name = rel_str.strip_suffix(".aura").unwrap_or(&rel_str);
                let out_file = out_path.join(format!("{}.auc", out_name));
                let out_dir_for_file = out_file.parent().unwrap_or(out_path).to_path_buf();
                if let Err(e) = std::fs::create_dir_all(&out_dir_for_file) {
                    eprintln!(
                        "[stdlib-compile] failed to create directory {}: {}",
                        out_dir_for_file.display(),
                        e
                    );
                    failed += 1;
                    continue;
                }
                match write_auc(&out_file.to_string_lossy(), &module) {
                    Ok(_) => {
                        eprintln!(
                            "[stdlib-compile] ✓ {} → {}",
                            rel_path.display(),
                            out_file.display()
                        );
                        success += 1;
                    }
                    Err(e) => {
                        eprintln!(
                            "[stdlib-compile] write failed {}: {}",
                            out_file.display(),
                            e
                        );
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "[stdlib-compile] ✗ {} compilation failed: {}",
                    rel_path.display(),
                    e
                );
                failed += 1;
            }
        }
    }

    eprintln!(
        "[stdlib-compile] done: {} succeeded, {} failed → {}",
        success,
        failed,
        out_path.display()
    );

    if success == 0 {
        eprintln!("[stdlib-compile] Warning: no files were successfully compiled");
    }
}

/// 递归扫描目录下的 .aura 文件
fn scan_aura_files_recursive(
    root: &std::path::Path,
    dir: &std::path::Path,
) -> std::io::Result<Vec<(std::path::PathBuf, std::path::PathBuf)>> {
    let mut results = Vec::new();
    let entries = std::fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            results.extend(scan_aura_files_recursive(root, &path)?);
        } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
            let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            results.push((rel, path));
        }
    }
    Ok(results)
}

fn cmd_check(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };

    let mut errs = 0;
    // 词法
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    for e in lexer.errors() {
        eprintln!("[lex] {}", e.message);
        errs += 1;
    }
    // 语法
    let mut parser = Parser::new(tokens);
    let _program = parser.parse_program();
    for e in parser.errors() {
        eprintln!("[syntax] {}", e.message);
        errs += 1;
    }
    // 语义
    let (_ast, sema) = analyze_source(&source);
    for e in &sema.errors {
        eprintln!("[sema] {}", e.message);
        errs += 1;
    }

    if errs == 0 {
        println!("✓ {} check passed", input);
    } else {
        println!("✗ {} has {} errors", input, errs);
        exit(1);
    }
}

fn cmd_tokens(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
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
            eprintln!("[lex] {}", e.message);
        }
        exit(1);
    }
}

fn cmd_ast(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
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
            eprintln!("[syntax] {}", e.message);
        }
        exit(1);
    }
}

fn cmd_fmt(args: &[String]) {
    let check_only = args.iter().any(|a| a == "--check");
    let input = match first_positional(args, "--check") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            eprintln!("Usage: aura fmt <file.aura> [--check]");
            exit(1);
        }
    };

    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };

    let formatted = compiler::lsp::format_source(&source);

    if check_only {
        if formatted != source {
            println!("{} needs formatting", input);
            exit(1);
        } else {
            println!("✓ {} already formatted", input);
        }
    } else {
        std::fs::write(input, &formatted)
            .map_err(|e| {
                eprintln!("Error: failed to write {}: {}", input, e);
                exit(1);
            })
            .ok();
        println!("✓ Formatted {}", input);
    }
}

/// P13.3: `aura lsp` — 启动 LSP 服务器
///
/// Phase 2: 优先启动独立的 `aura-lsp` 子进程（不加载 VM/AOT/JIT），
/// 若二进制不存在则回退到进程内运行（向后兼容）。
fn cmd_lsp(_args: &[String]) {
    // 尝试找到 aura-lsp 可执行文件
    let lsp_bin = find_lsp_binary();

    if let Some(bin_path) = lsp_bin {
        eprintln!(
            "[aura] Starting standalone LSP process: {}",
            bin_path.display()
        );
        // 使用子进程方式启动 aura-lsp，stdio 透传
        let status = std::process::Command::new(&bin_path)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|e| {
                eprintln!("Error: failed to start {}: {}", bin_path.display(), e);
                exit(1);
            })
            .unwrap();

        if !status.success() {
            eprintln!(
                "[aura] LSP process exited abnormally (code: {:?})",
                status.code()
            );
            exit(1);
        }
    } else {
        // 回退：进程内运行 LSP（兼容旧行为）
        eprintln!("[aura] aura-lsp not found, falling back to in-process LSP mode");
        compiler::lsp::run_lsp_server();
    }
}

/// 查找 `aura-lsp` 可执行文件
///
/// 搜索顺序：
/// 1. 当前可执行文件所在目录（`aura-lsp.exe` / `aura-lsp`）
/// 2. PATH 环境变量
fn find_lsp_binary() -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};

    let lsp_name = if cfg!(target_os = "windows") { "aura-lsp.exe" } else { "aura-lsp" };

    // 1. 当前可执行文件所在目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(lsp_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 2. PATH 搜索
    if let Ok(paths) = std::env::var("PATH") {
        let path_sep = if cfg!(target_os = "windows") { ";" } else { ":" };
        for dir in paths.split(path_sep) {
            let dir_path = Path::new(dir);
            if !dir_path.is_dir() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(lsp_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// P15: `aura debug` — 转发到独立 `aura-debug` 进程
///
/// 搜索顺序：
/// 1. 当前可执行文件所在目录（`aura-debug.exe` / `aura-debug`）
/// 2. PATH 环境变量
/// 3. 回退：显示提示信息
fn cmd_debug(args: &[String]) {
    let debug_bin = find_aura_debug_binary();

    match debug_bin {
        Some(bin_path) => {
            eprintln!("[aura] Starting debugger: {}", bin_path.display());
            let status = std::process::Command::new(&bin_path)
                .args(args)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status()
                .map_err(|e| {
                    eprintln!("Error: failed to start {}: {}", bin_path.display(), e);
                    exit(1);
                })
                .unwrap();

            if !status.success() {
                eprintln!(
                    "[aura] Debugger exited abnormally (code: {:?})",
                    status.code()
                );
                exit(1);
            }
        }
        None => {
            eprintln!("[aura] aura-debug not found, ensure it is in PATH");
            eprintln!("[aura] Or run directly: aura-debug <file.aura>");
            exit(1);
        }
    }
}

/// 搜索 `aura-debug` 可执行文件
fn find_aura_debug_binary() -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};

    let debug_name = if cfg!(target_os = "windows") { "aura-debug.exe" } else { "aura-debug" };

    // 1. 当前可执行文件所在目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(debug_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 2. PATH 搜索
    if let Ok(paths) = std::env::var("PATH") {
        let path_sep = if cfg!(target_os = "windows") { ";" } else { ":" };
        for dir in paths.split(path_sep) {
            let dir_path = Path::new(dir);
            if !dir_path.is_dir() {
                continue;
            }
            let candidate = PathBuf::from(dir).join(debug_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// P7.9: 内存泄漏检测命令
fn cmd_leak_check(args: &[String]) {
    let input = match first_positional(args, "--source") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            exit(1);
        }
    };
    let source = match std::fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
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
            eprintln!("[syntax] {}", e.message);
        }
        exit(1);
    }

    let hir = desugar_program(&program);
    let (mut mir_funcs, _ctx) = lower_program(&hir);

    // 运行完整 ARC 分析
    let result = compiler::codegen::arc::run_arc_analysis(&mut mir_funcs);

    println!("=== ARC Analysis Report ===");
    println!("{}", result.summary());
    println!();

    if let Some((name, info)) = result.escape_info.iter().next() {
        println!("--- Escape Analysis: {} ---", name);
        println!("  Escaping allocations: {}", info.escaping_allocs.len());
        println!(
            "  Non-escaping allocations: {}",
            info.non_escaping_allocs.len()
        );
    }

    println!();
    println!("--- ARC Insertion Statistics ---");
    println!("  Retain insertions: {}", result.insertion_stats.retains);
    println!("  Release insertions: {}", result.insertion_stats.releases);
    println!();
    println!("--- ARC Optimization Statistics ---");
    println!(
        "  Eliminated retains: {}",
        result.optimization_stats.eliminated_retains
    );
    println!(
        "  Eliminated releases: {}",
        result.optimization_stats.eliminated_releases
    );

    println!();
    if result.leak_report.is_clean() {
        println!("✅ Memory leak detection: no leaks");
    } else {
        println!(
            "⚠️  Memory leak detection: {} potential leaks found",
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
            eprintln!(
                "Error: module '{}' does not exist or has no documentation",
                module
            );
            eprintln!("Available modules:");
            for m in registry.module_names() {
                println!("  {}", m);
            }
            exit(1);
        }
        let content = compiler::docgen::render_module_markdown(&registry, module);
        std::fs::create_dir_all(&output_dir)
            .map_err(|e| {
                eprintln!("Error: failed to create output directory: {}", e);
                exit(1);
            })
            .ok();
        let file_path = output_dir.join(format!("std_{}.md", module));
        std::fs::write(&file_path, &content)
            .map_err(|e| {
                eprintln!("Error: failed to write {}: {}", file_path.display(), e);
                exit(1);
            })
            .ok();
        println!("✓ Module documentation generated: {}", file_path.display());
        println!("  Function count: {}", docs.len());
        return;
    }

    // 生成完整文档（Markdown + HTML）
    match compiler::docgen::generate_docs(&output_dir) {
        Ok(files) => {
            println!("✓ Generated {} documentation files:", files.len());
            for f in &files {
                println!("  {}", f.display());
            }
        }
        Err(e) => {
            eprintln!("Error: documentation generation failed: {}", e);
            exit(1);
        }
    }

    // 同时生成 HTML 版本
    let registry = compiler::docgen::DocRegistry::new().load_all();
    let html = compiler::docgen::render_html(&registry);
    let html_path = output_dir.join("index.html");
    if let Err(e) = std::fs::write(&html_path, &html) {
        eprintln!(
            "Warning: failed to write HTML documentation {}: {}",
            html_path.display(),
            e
        );
    } else {
        println!("  ✓ HTML documentation: {}", html_path.display());
    }
}

/// `aura eval` — 执行代码片段（类 node -e / python -c）
///
/// 用法：
///   aura eval --expr "println('hello')"
///   echo "println('hello')" | aura eval
fn cmd_eval(args: &[String]) {
    use std::io::{self, Read};

    let code =
        extract_opt(args, "--expr").or_else(|| first_positional(args, "--expr").map(|s| s.clone()));

    let code = match code {
        Some(c) => c,
        None => {
            // 从 stdin 读取
            let mut input = String::new();
            match io::stdin().read_to_string(&mut input) {
                Ok(_) => input,
                Err(e) => {
                    eprintln!("Error: failed to read from stdin: {}", e);
                    exit(1);
                }
            }
        }
    };

    if code.trim().is_empty() {
        eprintln!("Error: empty code");
        exit(1);
    }

    let module = match compile_source(&code) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation failed:\n{}", e);
            exit(1);
        }
    };

    let opts = VmOptions::default();
    let mut vm = match Vm::new(&module, opts) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("VM initialization failed: {}", e);
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
            eprintln!("Runtime error: {}", e);
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

    println!(
        "Aura REPL — Enter code and press Enter to execute. Multi-line continues with {{ or (. Exit: exit or Ctrl+D"
    );

    loop {
        let prompt = if buffer.is_empty() { ">>> " } else { "... " };
        eprint!("{}", prompt);
        io::stdout().flush().ok();

        let line = match lines.next() {
            Some(Ok(l)) => l,
            Some(Err(e)) => {
                eprintln!("Failed to read input: {}", e);
                break;
            }
            None => {
                // EOF (Ctrl+D)
                println!("\nGoodbye!");
                break;
            }
        };

        let trimmed = line.trim();

        // 退出命令
        if trimmed == "exit" || trimmed == "quit" {
            println!("Goodbye!");
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

    use compiler::package::{PackageManager, PackageManifest};

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
                eprintln!("Warning: failed to parse aura.toml: {}", e);
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
        println!("✓ No dependencies to install");
        return;
    }

    let config = compiler::package::PackageManagerConfig {
        offline,
        ..Default::default()
    };
    let mut pm = PackageManager::with_config(config);

    println!("Installing {} dependencies...", deps.len());
    match pm.install(&project_dir, &deps) {
        Ok(lock) => {
            println!("✓ Installed {} dependencies", lock.dependencies.len());
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
            eprintln!("Installation failed: {}", e);
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
        eprintln!("Failed to load project: {}", e);
        exit(1);
    }

    match pm.update(&project_dir, all) {
        Ok(updated) => {
            if updated.is_empty() {
                println!("✓ All dependencies are up to date");
            } else {
                println!("✓ Updated {} dependencies:", updated.len());
                for u in &updated {
                    println!("  {}", u);
                }
            }
        }
        Err(e) => {
            eprintln!("Update failed: {}", e);
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
            eprintln!("Publish failed: {}", e);
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
        eprintln!("Failed to load project: {}", e);
        exit(1);
    }

    match pm.show_deps(&project_dir) {
        Ok(tree) => {
            if outdated {
                println!("=== Outdated Dependencies ===");
                // 简化：标记所有依赖
                println!("Run 'aura update --all' to update all dependencies");
            }
            println!("{}", tree);
        }
        Err(e) => {
            eprintln!("Failed to display dependencies: {}", e);
            exit(1);
        }
    }
}

/// P11: `aura new` — 创建新包项目
fn cmd_new(args: &[String]) {
    let name = match first_positional(args, "--dir") {
        Some(n) => n.clone(),
        None => {
            eprintln!("Error: missing package name");
            eprintln!("Usage: aura new <name> [--dir <path>]");
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
            println!(
                "✓ Created new package project: {}",
                parent_dir.join(&name).display()
            );
            println!("  Next steps:");
            println!("    cd {}", name);
            println!("    aura run main.aura");
            println!("    aura publish");
        }
        Err(e) => {
            eprintln!("Creation failed: {}", e);
            exit(1);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1: .auz 制品格式命令
// ─────────────────────────────────────────────────────────────────────────────

/// Phase 1: `aura package` — 打包为 .auz 制品
fn cmd_package(args: &[String]) {
    use compiler::auz::{PackageBuildOptions, PackageBuilder};
    use compiler::package::PackageManifest;

    // 解析参数
    let output = extract_opt(args, "--output");
    let input = match first_positional(args, "--output") {
        Some(p) => p.clone(),
        None => {
            eprintln!("Error: missing input file");
            eprintln!("Usage: aura package <file.aura> [--output <out.auz>] [--sources]");
            exit(1);
        }
    };

    let include_sources = args.iter().any(|a| a == "--sources");

    // 读取并编译源码
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };

    let module = match compile_source(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation failed:\n{}", e);
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
                eprintln!(
                    "Warning: failed to parse aura.toml: {}, using default manifest",
                    e
                );
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
    let out_path = output.map(|s| std::path::PathBuf::from(s)).unwrap_or_else(|| {
        let base = input.trim_end_matches(".aura").trim_end_matches(".AURA");
        std::path::PathBuf::from(format!("{}.auz", base))
    });

    // 执行打包
    match builder.build(&out_path) {
        Ok(result) => {
            println!("{}", result.summary());
            println!("  Package type: {}", manifest.kind);
            if manifest.library {
                println!("  Library package: true");
            }
            println!("  File list:");
            for entry in &result.checksum_entries {
                println!("    {}", entry.path);
            }
        }
        Err(e) => {
            eprintln!("Packaging failed: {}", e);
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
            eprintln!("Error: missing input file");
            eprintln!("Usage: aura inspect <file.auz>");
            exit(1);
        }
    };

    let verbose = args.iter().any(|a| a == "--verbose");

    match PackageReader::from_file(&std::path::PathBuf::from(&input)) {
        Ok(content) => {
            println!("=== .auz package info ===");
            println!("Name:     {}", content.manifest.name);
            println!("Version:  {}", content.manifest.version);
            println!("Type:     {}", content.manifest.kind);
            println!("Library:  {}", content.manifest.library);
            if let Some(desc) = &content.manifest.description {
                println!("Desc:     {}", desc);
            }
            if let Some(license) = &content.manifest.license {
                println!("License:  {}", license);
            }
            if let Some(min_ver) = &content.manifest.compiler_min_version {
                println!("Min compiler: >= {}", min_ver);
            }
            if !content.manifest.exports.is_empty() {
                println!("Exports:  {}", content.manifest.exports.join(", "));
            }
            println!();
            println!("=== File list ({} files) ===", content.files.len());
            for (path, data) in &content.files {
                let size = data.len();
                println!("  {:6}  {}", size, path);
            }

            // 字节码模块信息
            if let Some(module) = &content.module {
                println!();
                println!("=== Bytecode module ===");
                println!("  Functions:  {}", module.functions.len());
                println!("  Constants:  {}", module.consts.len());
                println!("  Natives:    {}", module.natives.len());
                if !module.enabled_modules.is_empty() {
                    println!("  Enabled modules: {}", module.enabled_modules.join(", "));
                }
            }

            if verbose {
                println!();
                println!("=== Checksum entries ===");
                for entry in &content.checksum_entries {
                    println!("  {}  {}", entry.hash, entry.path);
                }
            }
        }
        Err(e) => {
            eprintln!("Inspection failed: {}", e);
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
            eprintln!("Error: missing input file");
            eprintln!("Usage: aura verify <file.auz>");
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
            eprintln!("Verification failed: {}", e);
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
                        eprintln!("Runtime error: {}", e);
                    }
                },
                Err(e) => {
                    eprintln!("VM initialization failed: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Compilation failed:\n{}", e);
        }
    }
}

/// 生成 C ABI 头文件（Tier 2b: 供外部 C/Python 消费者调用）
///
/// 用法: `aura export-header <file.aura> --out <name>.h`
///
/// 从 HIR 提取所有函数签名，生成对应的 C 头文件声明。
/// 生成的头文件包含 `aura_c_<name>` 函数原型，与 `--cabi` 生成的包装函数匹配。
fn cmd_export_header(args: &[String]) {
    use compiler::codegen::hir::desugar_program;

    let input = match first_positional(args, "--out") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing input file");
            eprintln!("Usage: aura export-header <file.aura> --out <name>.h");
            exit(1);
        }
    };

    let out_path = match extract_opt(args, "--out") {
        Some(p) => p,
        None => {
            eprintln!("Error: missing --out argument");
            eprintln!("Usage: aura export-header <file.aura> --out <name>.h");
            exit(1);
        }
    };

    // 读取源文件
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read {}: {}", input, e);
            exit(1);
        }
    };

    // 编译到 HIR
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        eprintln!("Lex error: {}", e.message);
        exit(1);
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        eprintln!("Syntax error: {}", e.message);
        exit(1);
    }
    let hir = desugar_program(&program);

    // 生成 C 头文件
    let header_content = generate_c_header(&hir, &input);

    // 写入文件
    if let Err(e) = std::fs::write(&out_path, &header_content) {
        eprintln!("Error: failed to write {}: {}", out_path, e);
        exit(1);
    }

    println!("✓ C header file generation complete: {}", out_path);
    println!(
        "  Contains {} function declarations",
        count_functions(&header_content)
    );
}

/// 从 HIR 生成 C ABI 头文件内容
fn generate_c_header(hir: &compiler::codegen::hir::HirProgram, source_path: &str) -> String {
    use compiler::codegen::hir::HirType;

    let guard = sanitize_header_name(source_path);
    let mut s = String::new();

    // 文件头保护
    s.push_str(&format!("#ifndef {}\n", guard));
    s.push_str(&format!("#define {}\n\n", guard));

    s.push_str("// Auto-generated by `aura export-header`\n");
    s.push_str(&format!("// Source: {}\n\n", source_path));

    // 函数声明
    for func in &hir.functions {
        if func.is_native {
            continue;
        }

        let c_func_name = format!("aura_c_{}", sanitize_c_name(&func.name));
        let params_str = func
            .params
            .iter()
            .map(|p| {
                let default_ty = compiler::codegen::hir::HirType::Named("Int".into());
                let ty = p.ty.as_ref().unwrap_or(&default_ty);
                format!("{} {}", aura_type_to_c(ty), sanitize_c_name(&p.name))
            })
            .collect::<Vec<_>>()
            .join(", ");

        let ret_type =
            func.ret.as_ref().map(|t| aura_type_to_c(t)).unwrap_or_else(|| "void".to_string());

        s.push_str(&format!(
            "extern \"C\" {} {}({});\n",
            ret_type, c_func_name, params_str
        ));
    }

    s.push_str(&format!("\n#endif /* {} */\n", guard));

    s
}

/// 将 Aura HIR 类型映射为 C 类型
fn aura_type_to_c(ty: &compiler::codegen::hir::HirType) -> String {
    use compiler::codegen::hir::HirType;

    match ty {
        HirType::Named(name) => match name.as_str() {
            "Int" | "Long" | "Short" | "Byte" | "U8" | "Char" => "int".to_string(),
            "Float" | "Double" => "double".to_string(),
            "Boolean" | "Bool" => "int".to_string(),
            "Unit" | "Void" | "Nothing" => "void".to_string(),
            "String" | "Str" => "const char *".to_string(),
            "CString" | "CStr" => "const char *".to_string(),
            _ => "void *".to_string(),
        },
        HirType::Pointer(_) => "void *".to_string(),
        _ => "void *".to_string(),
    }
}

/// 将文件路径转换为 C 头文件保护宏名
fn sanitize_header_name(path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "aura_export".to_string());
    format!(
        "AURA_EXPORT_{}_H",
        stem.to_uppercase().replace('.', "_").replace('-', "_")
    )
}

/// 将 Aura 函数名转换为 C 函数名
fn sanitize_c_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

/// 统计头文件中的函数声明数量
fn count_functions(header: &str) -> usize {
    header.lines().filter(|l| l.contains("extern \"C\"")).count()
}
