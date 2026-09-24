//! `aurap` - Aura 包管理器入口

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
        "install" | "update" | "publish" | "deps" | "new" | "package" | "inspect" | "verify" => {}
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        other => {
            eprintln!("Unknown aurap subcommand: {}", other);
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
        eprintln!("Hint: build with `cargo build` and run from the same directory");
        process::exit(1);
    }
}

fn print_usage() {
    println!(
        "aurap - Aura package manager\n\
\n\
Usage:\n\
  aurap install [--offline]                      Install dependencies\n\
  aurap update [--all]                           Update dependencies\n\
  aurap publish [--dir <path>]                   Publish package\n\
  aurap deps [--dir <path>] [--outdated]         Show dependency tree\n\
  aurap new <name> [--dir <path>]                Create new package project\n\
  aurap package <file.aura> [--output <out>]     Package as .auz\n\
  aurap inspect <file.auz> [--verbose]           Inspect .auz content\n\
  aurap verify <file.auz>                        Verify .auz checksum\n\
  aurap help | --help"
    );
}

fn find_aura_binary() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") { "aura.exe" } else { "aura" };
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(name);
    candidate.exists().then_some(candidate)
}
