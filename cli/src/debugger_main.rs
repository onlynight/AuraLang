//! `aura-debug` - Aura language debugger
//
// Modeled after GDB / LLDB, providing interactive source-level debugging: breakpoints,
// stepping, variable inspection, and call stacks.
// Supports three debug modes: VM (interpretation), JIT (compiled tracing), AOT (DWARF + external debugger).
//
// Usage:
//   aura-debug <file.aura>                          VM mode debugging (default)
//   aura-debug --mode jit <file.aura>            JIT mode debugging
//   aura-debug --mode aot <file.aura>            AOT mode debugging
//   aura-debug --mode aot --launch <file.aura>   AOT + launch external debugger
//   aura-debug --help                            Show help

use std::process;

use compiler::codegen::compile_source;
use compiler::vm::debugger::{DebugMode, DebugSession};

mod debugger;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        print_usage();
        return;
    }

    if args[1] == "--version" || args[1] == "-V" {
        println!("aura-debug 0.1.0");
        return;
    }

    let mut mode = DebugMode::Vm;
    let mut file_name: Option<String> = None;
    let mut _launch_external = false;

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--mode" && i + 1 < args.len() {
            match args[i + 1].as_str() {
                "vm" => mode = DebugMode::Vm,
                "jit" => mode = DebugMode::Jit,
                "aot" => mode = DebugMode::Aot,
                other => {
                    eprintln!("Unknown mode: {} (support: vm/jit/aot)", other);
                    process::exit(1);
                }
            }
            i += 2;
        } else if args[i] == "--launch" {
            _launch_external = true;
            i += 1;
        } else if !args[i].starts_with("--") {
            file_name = Some(args[i].clone());
            i += 1;
        } else {
            eprintln!("Unknown argument: {}", args[i]);
            print_usage();
            process::exit(1);
        }
    }

    let file_name = match file_name {
        Some(f) => f,
        None => {
            eprintln!("Error: missing source file path");
            print_usage();
            process::exit(1);
        }
    };

    let source = match std::fs::read_to_string(&file_name) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: cannot read {}: {}", file_name, e);
            process::exit(1);
        }
    };

    let module = match compile_source(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation failed:\n{}", e);
            process::exit(1);
        }
    };

    let mut session = match mode {
        DebugMode::Vm => DebugSession::new(module, &source, &file_name),
        DebugMode::Jit => DebugSession::new_jit(module, &source, &file_name),
        DebugMode::Aot => {
            let mut session = DebugSession::new(module, &source, &file_name);
            session.mode = DebugMode::Aot;
            // Phase 3: Automatically attempt AOT compilation
            #[cfg(feature = "llvm")]
            match session.aot_compile() {
                Ok(summary) => eprintln!("{}", summary),
                Err(e) => eprintln!(
                    "AOT compilation failed: {}\n  Hint: Ensure LLVM is installed (llc/clang)",
                    e
                ),
            }
            #[cfg(not(feature = "llvm"))]
            eprintln!("AOT mode requires the llvm feature (cargo build --features llvm)");
            session
        }
    };

    debugger::run_debugger(&mut session);
}

fn print_usage() {
    println!(
        "aura-debug - Aura language debugger

![P15] Usage:
  aura-debug <file.aura>                          VM mode (default)
  aura-debug --mode jit <file.aura>            JIT mode
  aura-debug --mode aot <file.aura>            AOT mode
  aura-debug --mode aot --launch <file.aura>   AOT + external debugger

![P15] Options:
  --mode <vm|jit|aot>     Debug mode (default: vm)
  --launch                AOT: launch external debugger (lldb/gdb)
  --help, -h              Show this help
  --version, -V           Show version

![P15] Interactive commands:
  break <line|function>    Set breakpoint
  continue                 Continue to next breakpoint
  step / next / out      Step execution
  backtrace                Show call stack
  list [line]              Show source code
  print <expr>             Print value
  locals / stack           Show locals / operand stack
  info                     Debugger status summary
  help                     Show help
  quit                     Exit debugger"
    );
}
