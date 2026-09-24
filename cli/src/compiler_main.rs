//! `aurac` - Aura 编译器入口

use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let subcommand = args[1].clone();
    let rest = &args[1..];

    match subcommand.as_str() {
        "build" | "check" | "disasm" | "tokens" | "ast" | "fmt" | "leak-check" | "stdlib-compile" | "export-header" => {}
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        other => {
            eprintln!("Unknown aurac subcommand: {}", other);
            print_usage();
            process::exit(1);
        }
    }

    if let Some(aura_bin) = find_aura_binary() {
        let status = std::process::Command::new(&aura_bin)
            .args(rest)
            .stdin(process::Stdio::inherit())
            .stdout(process::Stdio::inherit())
            .stderr(process::Stdio::inherit())
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Error: failed to start {}: {}", aura_bin.display(), e);
                process::exit(1);
            });
        process::exit(status.code().unwrap_or(1));
    } else {
        eprintln!("Error: aura.exe not found");
        eprintln!("Hint: build with `cargo build --features llvm` and run from the same directory");
        process::exit(1);
    }
}

fn print_usage() {
    println!(
        "aurac - Aura compiler\n\
\n\
Usage:\n\
  aurac build <file.aura> [--output <out>]        Compile to bytecode / native executable\n\
  aurac check <file.aura>                          Syntax/semantic check only\n\
  aurac disasm <file.auc> [--source <f.aura>]      Disassemble .auc\n\
  aurac tokens <file.aura>                         Print lexical analysis\n\
  aurac ast <file.aura>                            Print AST\n\
  aurac fmt <file.aura> [--check]                  Code formatting\n\
  aurac leak-check <file.aura>                     ARC leak analysis\n\
  aurac stdlib-compile <core-dir> [--output <dir>] Pre-compile stdlib\n\
  aurac export-header <file.aura> --out <name>.h   Export C ABI header\n\
  aurac help | --help"
    );
}

fn find_aura_binary() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") { "aura.exe" } else { "aura" };
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(name);
    candidate.exists().then_some(candidate)
}
