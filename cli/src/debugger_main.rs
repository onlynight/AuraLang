//! `aura-debug` - Aura language debugger
//
// 对标 GDB / LLDB，提供交互式源码级调试：断点、单步、变量检查、调用栈。
// 支持三种调试模式：VM（解释执行）、JIT（编译跟踪）、AOT（DWARF + 外部调试器）。
//
// 用法：
//   aura-debug <file.aura>                          VM 模式调试（默认）
//   aura-debug --mode jit <file.aura>            JIT 模式调试
//   aura-debug --mode aot <file.aura>            AOT 模式调试
//   aura-debug --mode aot --launch <file.aura>   AOT + 启动外部调试器
//   aura-debug --help                            显示帮助

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
            // Phase 3: 自动尝试 AOT 编译
            #[cfg(feature = "llvm")]
            match session.aot_compile() {
                Ok(summary) => eprintln!("{}", summary),
                Err(e) => eprintln!("AOT 编译失败: {}\n  提示: 确保安装了 LLVM (llc/clang)", e),
            }
            #[cfg(not(feature = "llvm"))]
            eprintln!("AOT 模式需要 llvm feature (cargo build --features llvm)");
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
