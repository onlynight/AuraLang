// Aura Seed Compiler — CLI entry point
// Rebuilt from library source to include emit.rs fix for CallVirtual bare-name resolution.

use std::env;
use std::process;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: aura <command> [options]");
        eprintln!("Commands: run, compile, check, version");
        process::exit(1);
    }

    match args[1].as_str() {
        "version" => {
            println!("Aura Seed Compiler 0.1.0 (with emit.rs fix)");
        }
        "run" | "compile" | "check" => {
            let entry = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if entry.is_empty() {
                eprintln!("Error: no input file specified");
                process::exit(1);
            }
            match fs::read_to_string(entry) {
                Ok(source) => {
                    match compiler::codegen::compile_source(&source) {
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
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            process::exit(1);
        }
    }
}