// Aura Seed Compiler — CLI entry point
// Rebuilt from library source to include emit.rs fix for CallVirtual bare-name resolution.

use std::env;
use std::process;
use std::fs;

fn main() {
    // 增加线程栈大小（导入解析递归深度大）
    let stack_size = 64 * 1024 * 1024; // 64MB
    let handle = std::thread::Builder::new()
        .stack_size(stack_size)
        .spawn(|| {
            real_main();
        })
        .expect("failed to spawn thread");
    handle.join().expect("thread panicked");
}

fn real_main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: aura <command> [options]");
        eprintln!("Commands: run, compile, check, version, build");
        process::exit(1);
    }

    match args[1].as_str() {
        "version" => {
            println!("Aura Seed Compiler 0.1.0 (with emit.rs fix)");
        }
        "run" | "compile" | "check" | "build" => {
            let entry = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if entry.is_empty() {
                eprintln!("Error: no input file specified");
                process::exit(1);
            }
            if args[1] == "build" {
                // build [-b photon] [--aot] <entry> --output <path>
                // 跳过 -b 及其参数（photon 后端标记）、--aot 等 flag
                let mut i = 2;
                let backend = if i < args.len() && args[i] == "-b" && i + 1 < args.len() {
                    let be = args[i + 1].clone();
                    i += 2;
                    Some(be)
                } else {
                    None
                };
                // 跳过 --aot flag（不消费参数）
                while i < args.len() && args[i].starts_with("--") {
                    i += 1;
                }
                let entry = args.get(i).map(|s| s.as_str()).unwrap_or("");
                if entry.is_empty() {
                    eprintln!("Error: no input file specified");
                    process::exit(1);
                }
                let output = args.iter()
                    .position(|a| a == "--output")
                    .and_then(|idx| args.get(idx + 1))
                    .map(|s| s.as_str())
                    .unwrap_or("");
                if output.is_empty() {
                    eprintln!("Error: --output <path> required for build");
                    process::exit(1);
                }
                let use_aot = args.iter().any(|a| a == "--aot");
                let use_photon = backend.as_deref() == Some("photon");
                match fs::read_to_string(entry) {
                    Ok(source) => {
                        // 先解析 import，内联所有引用的模块
                        let expanded = compiler::codegen::resolve_aura_imports(
                            &source,
                            Some(entry),
                        );
                        let output_path = std::path::Path::new(output);
                        if use_aot {
                            // AOT 编译：生成原生可执行文件
                            let options = compiler::codegen::aot::AotOptions::default();
                            match compiler::codegen::aot::aot_compile(&expanded, output_path, options) {
                                Ok(_) => {
                                    println!("[ok] {} built", output);
                                }
                                Err(e) => {
                                    eprintln!("Error: {}", e);
                                    process::exit(1);
                                }
                            }
                        } else {
                            // Bytecode 编译：生成 .auc 文件
                            match compiler::codegen::compile_source(&expanded) {
                                Ok(module) => {
                                    // 序列化模块到 .auc 文件
                                    match compiler::codegen::serialize::write_auc(output, &module) {
                                        Ok(_) => {
                                            println!("[ok] {} built", output);
                                        }
                                        Err(e) => {
                                            eprintln!("Error writing {}: {}", output, e);
                                            process::exit(1);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Error: {}", e);
                                    process::exit(1);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading file: {}", e);
                        process::exit(1);
                    }
                }
            } else {
                match fs::read_to_string(entry) {
                    Ok(source) => {
                        // 先解析 import，内联所有引用的模块
                        let expanded = compiler::codegen::resolve_aura_imports(
                            &source,
                            Some(entry),
                        );
                        match compiler::codegen::compile_source(&expanded) {
                            Ok(module) => {
                                println!("[ok] {} compiled ({} functions)", entry, module.functions.len());
                                if args[1] == "run" {
                                    // Execute the compiled module
                                    let opts = compiler::vm::VmOptions::default();
                                    match compiler::vm::Vm::new(&module, opts) {
                                        Ok(mut vm) => {
                                            match vm.run() {
                                                Ok(val) => {
                                                    if let compiler::vm::Value::Str(s) = val {
                                                        println!("{}", s);
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("Runtime error: {}", e);
                                                    process::exit(1);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("VM init error: {}", e);
                                            process::exit(1);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading file: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}