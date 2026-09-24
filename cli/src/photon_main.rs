//! `photon` - Photon 原生编译器入口

use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    let subcommand = args[1].clone();
    let rest = &args[2..];

    let photon_args: Vec<String> = match subcommand.as_str() {
        "--help" | "-h" | "help" => {
            print_usage();
            return;
        }
        "build" | "check" | "run" => {
            let mut a = Vec::with_capacity(rest.len() + 1);
            a.push(subcommand.clone());
            a.extend_from_slice(rest);
            a
        }
        other => {
            eprintln!("Unknown photon subcommand: {}", other);
            print_usage();
            process::exit(1);
        }
    };

    if let Some(aura_bin) = find_aura_binary() {
        let status = std::process::Command::new(&aura_bin)
            .args(["build", "-b", "photon"])
            .args(&photon_args)
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
        "photon - Photon native compiler\n\
\n\
Usage:\n\
  photon build <file.aura> [--output <out.exe>]         Compile with Photon backend\n\
  photon build <file.aura> [--output <out.exe>] [--debug]\n\
  photon build <file.aura> [--output <out.exe>] [--emit-llvm]\n\
  photon check <file.aura>                              Syntax/semantic check\n\
  photon run <file.aura>                                Build and execute\n\
  photon help | --help"
    );
}

fn find_aura_binary() -> Option<PathBuf> {
    let name = if cfg!(target_os = "windows") { "aura.exe" } else { "aura" };
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.join(name);
    candidate.exists().then_some(candidate)
}
