//! Aura debugger interactive REPL
//!
//! Commands: break / continue / step / next / backtrace / list / print / locals
//!           stack / info / mode / del / help / quit

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use compiler::vm::debugger::{BreakpointTarget, DebugMode, DebugSession, StopReason, format_value};

/// Debugger REPL main entry
pub fn run_debugger(session: &mut DebugSession) {
    print_banner(session);

    // Initialize VM
    if let Err(e) = session.initialize() {
        eprintln!("Error: debugger initialization failed: {}", e);
        return;
    }

    loop {
        // Display current paused state
        if session.paused {
            show_paused_state(session);
        }

        // Display prompt
        let prompt = if session.paused { "> " } else { "  " };
        eprint!("{}", prompt);
        io::stdout().flush().ok();

        // Read command
        let stdin = io::stdin();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF
                println!("\nGoodbye!");
                session.cleanup();
                return;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Failed to read input: {}", e);
                return;
            }
        }

        let cmd = line.trim().to_lowercase();

        if cmd.is_empty() {
            // Empty line: if paused, re-display state
            if session.paused {
                show_paused_state(session);
            }
            continue;
        }

        // Execute command
        match execute_command(session, &line) {
            CommandResult::Exit => {
                println!("Goodbye!");
                session.cleanup();
                return;
            }
            CommandResult::Run => {
                // Run VM
                match session.run() {
                    Ok(reason) => {
                        match reason {
                            StopReason::Breakpoint {
                                description,
                                ..
                            } => {
                                println!("\n  Breakpoint hit: {}", description);
                            }
                            StopReason::Step { mode } => {
                                println!("\n  Stepped ({}):", mode.as_str());
                            }
                            StopReason::Completion { result } => {
                                println!("\n  Execution complete: {}", format_value(&result));
                                session.step_mode = compiler::vm::debugger::StepMode::Off;
                            }
                            StopReason::Error {
                                message,
                                at_func,
                            } => {
                                if let Some(func) = at_func {
                                    eprintln!("\n  Runtime error [{}]: {}", func, message);
                                } else {
                                    eprintln!("\n  Runtime error: {}", message);
                                }
                            }
                            StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {
                                // JIT/AOT mode only
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Execution error: {}", e);
                        session.paused = true;
                        session.stop_reason = Some(StopReason::Error {
                            message: e.to_string(),
                            at_func: session.current_function().map(|s| s.to_string()),
                        });
                    }
                }
            }
            CommandResult::Continue => {
                session.continue_execution();
                // Run immediately
                match session.run() {
                    Ok(reason) => match reason {
                        StopReason::Breakpoint {
                            description,
                            ..
                        } => {
                            println!("\n  Breakpoint hit: {}", description);
                        }
                        StopReason::Step { mode } => {
                            println!("\n  Stepped ({}):", mode.as_str());
                        }
                        StopReason::Completion { result } => {
                            println!("\n  Execution complete: {}", format_value(&result));
                            session.step_mode = compiler::vm::debugger::StepMode::Off;
                        }
                        StopReason::Error {
                            message,
                            at_func,
                        } => {
                            if let Some(func) = at_func {
                                eprintln!("\n  Runtime error [{}]: {}", func, message);
                            } else {
                                eprintln!("\n  Runtime error: {}", message);
                            }
                        }
                        StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {}
                    },
                    Err(e) => {
                        eprintln!("Execution error: {}", e);
                        session.paused = true;
                        session.stop_reason = Some(StopReason::Error {
                            message: e.to_string(),
                            at_func: session.current_function().map(|s| s.to_string()),
                        });
                    }
                }
            }
            CommandResult::Step { mode } => {
                match mode {
                    StepMode::In => session.step_in(),
                    StepMode::Over => session.step_over(),
                    StepMode::Out => session.step_out(),
                    StepMode::Off => {}
                }
                // Execute one step
                match session.run() {
                    Ok(reason) => match reason {
                        StopReason::Breakpoint {
                            description,
                            ..
                        } => {
                            println!("\n  Breakpoint hit: {}", description);
                        }
                        StopReason::Step { mode } => {
                            println!("\n  Stepped ({}):", mode.as_str());
                        }
                        StopReason::Completion { result } => {
                            println!("\n  Execution complete: {}", format_value(&result));
                            session.step_mode = compiler::vm::debugger::StepMode::Off;
                        }
                        StopReason::Error {
                            message,
                            at_func,
                        } => {
                            if let Some(func) = at_func {
                                eprintln!("\n  Runtime error [{}]: {}", func, message);
                            } else {
                                eprintln!("\n  Runtime error: {}", message);
                            }
                        }
                        StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {}
                    },
                    Err(e) => {
                        eprintln!("Execution error: {}", e);
                        session.paused = true;
                        session.stop_reason = Some(StopReason::Error {
                            message: e.to_string(),
                            at_func: session.current_function().map(|s| s.to_string()),
                        });
                    }
                }
            }
            CommandResult::None => {}
        }
    }
}

/// Command execution result
enum CommandResult {
    None,
    Run,
    Continue,
    Step { mode: StepMode },
    Exit,
}

use compiler::vm::debugger::StepMode;

/// Execute command
fn execute_command(session: &mut DebugSession, raw_line: &str) -> CommandResult {
    let line = raw_line.trim();
    if line.is_empty() {
        return CommandResult::None;
    }

    // Split command and arguments
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let args = parts.next().unwrap_or("").trim().to_string();

    match cmd.as_str() {
        // ── Breakpoint commands ──
        "break" | "b" => {
            if args.is_empty() {
                println!("  Usage: break <line|function>");
                return CommandResult::None;
            }

            let target = if let Ok(line_num) = args.parse::<usize>() {
                BreakpointTarget::Line {
                    line: line_num,
                }
            } else {
                BreakpointTarget::Function { name: args }
            };

            match session.set_breakpoint(target) {
                Ok(id) => {
                    println!("  Breakpoint #{} set", id);
                }
                Err(e) => {
                    eprintln!("  Failed to set breakpoint: {}", e);
                }
            }
            CommandResult::None
        }

        // ── Delete breakpoint ──
        "del" | "d" => {
            if let Ok(id) = args.parse::<usize>() {
                if session.delete_breakpoint(id) {
                    println!("  Breakpoint #{} deleted", id);
                } else {
                    println!("  Breakpoint #{} does not exist", id);
                }
            } else {
                println!("  Usage: del <breakpoint-id>");
            }
            CommandResult::None
        }

        // ── Continue execution ──
        "continue" | "c" => CommandResult::Continue,

        // ── Step commands ──
        "step" | "s" | "si" => CommandResult::Step {
            mode: StepMode::In,
        },
        "next" | "n" | "so" => CommandResult::Step {
            mode: StepMode::Over,
        },
        "out" => CommandResult::Step {
            mode: StepMode::Out,
        },

        // ── Call stack ──
        "backtrace" | "bt" => {
            println!("{}", session.show_backtrace());
            CommandResult::None
        }

        // ── Source display ──
        "list" | "l" => {
            let center = if args.is_empty() {
                session.current_line()
            } else {
                args.parse::<usize>().unwrap_or(session.current_line())
            };
            println!("{}", session.show_source(center, 3));
            CommandResult::None
        }

        // ── Print value ──
        "print" | "p" => {
            if args.is_empty() {
                println!("  Usage: print <expression>");
            } else {
                // Simplified: search for value at top of stack
                if let Some(vm) = session.vm.as_ref() {
                    if let Some(frame) = vm.frames().last() {
                        if let Some(val) = frame.stack.last() {
                            println!("  Stack top = {}", format_value(val));
                        } else if let Some(func_idx) = session.current_func_idx() {
                            if let Some(func) = vm.module_ref().funcs.get(func_idx) {
                                let last_slot = func.locals as usize - 1;
                                if last_slot < frame.locals.len() {
                                    println!(
                                        "  Last local variable = {}",
                                        format_value(&frame.locals[last_slot])
                                    );
                                }
                            }
                        }
                    }
                }
            }
            CommandResult::None
        }

        // ── Local variables ──
        "locals" => {
            let depth = if args.is_empty() { None } else { args.parse::<usize>().ok() };
            println!("{}", session.show_locals(depth));
            CommandResult::None
        }

        // ── Operand stack ──
        "stack" => {
            let depth = if args.is_empty() { None } else { args.parse::<usize>().ok() };
            println!("{}", session.show_stack(depth));
            CommandResult::None
        }

        // ── Info commands ──
        "info" => {
            if args.is_empty() || args == "all" {
                println!("{}", session.show_info());
            } else if args == "functions" || args == "func" {
                println!("{}", session.show_functions());
            } else if args == "breakpoints" || args == "b" {
                println!("{}", session.list_breakpoints());
            } else if args == "mode" {
                println!("  Current mode: {}", session.mode.as_str());
            } else if args == "line" {
                println!("  Current line: {}", session.current_line());
                if let Some(func) = session.current_function() {
                    println!("  Current function: {}", func);
                }
            } else {
                println!("  Usage: info [functions|breakpoints|mode|line|all]");
            }
            CommandResult::None
        }

        // ── Mode switch ──
        "mode" | "m" => {
            if args.is_empty() {
                println!("  Current mode: {}", session.mode.as_str());
                println!("  Available modes: vm, jit, aot");
            } else {
                match args.as_str() {
                    "vm" => {
                        session.mode = DebugMode::Vm;
                        println!("  Mode switched to: VM");
                    }
                    "jit" => {
                        session.mode = DebugMode::Jit;
                        println!("  Mode switched to: JIT");
                    }
                    "aot" => {
                        session.mode = DebugMode::Aot;
                        println!("  Mode switched to: AOT");
                    }
                    _ => {
                        println!("  Unknown mode: {}", args);
                    }
                }
            }
            CommandResult::None
        }

        // ── Function list ──
        "functions" | "funcs" => {
            println!("{}", session.show_functions());
            CommandResult::None
        }

        // ── Breakpoint list ──
        "breakpoints" | "bps" => {
            println!("{}", session.list_breakpoints());
            CommandResult::None
        }

        // ── Phase 2: JIT commands ──
        "jit" => {
            if args.is_empty() {
                println!("  JIT subcommands: state, fallbacks, compiled");
                println!("    jit state      Show JIT compilation state");
                println!("    jit fallbacks  Show JIT fallback details");
                println!("    jit compiled   Show compiled functions");
            } else {
                match args.as_str() {
                    "state" => {
                        session.refresh_jit_info();
                        println!("{}", session.show_jit_state());
                    }
                    "fallbacks" => {
                        session.refresh_jit_info();
                        println!("{}", session.show_jit_fallbacks());
                    }
                    "compiled" => {
                        session.refresh_jit_info();
                        println!("{}", session.show_jit_compiled());
                    }
                    _ => {
                        println!(
                            "  Unknown JIT subcommand: {} (available: state, fallbacks, compiled)",
                            args
                        );
                    }
                }
            }
            CommandResult::None
        }

        // ── Phase 3: AOT commands ──
        "aot" => {
            if args.is_empty() {
                println!("  AOT subcommands: compile, dwarf, launch, path");
                println!("    aot compile  Run AOT compilation");
                println!("    aot dwarf    Show DWARF debug information");
                println!("    aot launch   Launch external debugger (lldb/gdb)");
                println!("    aot path     Show output path");
            } else {
                #[cfg(feature = "llvm")]
                match args.as_str() {
                    "compile" => match session.aot_compile() {
                        Ok(summary) => println!("{}", summary),
                        Err(e) => eprintln!("  AOT compilation failed: {}", e),
                    },
                    _ => {}
                }
                #[cfg(not(feature = "llvm"))]
                {
                    println!(
                        "  AOT feature requires the llvm feature (cargo build --features llvm)"
                    );
                }

                if args == "dwarf" {
                    println!("{}", session.show_aot_dwarf());
                } else if args == "path" {
                    println!("{}", session.show_aot_path());
                } else if args == "launch" {
                    match session.aot_launch() {
                        Ok(msg) => println!("{}", msg),
                        Err(e) => eprintln!("  Launch failed: {}", e),
                    }
                } else if args != "compile" {
                    println!(
                        "  Unknown AOT subcommand: {} (available: compile, dwarf, launch, path)",
                        args
                    );
                }
            }
            CommandResult::None
        }

        // ── Help ──
        "help" | "h" | "?" => {
            print_help();
            CommandResult::None
        }

        // ── Exit ──
        "quit" | "q" | "exit" | "e" => CommandResult::Exit,

        // ── Unrecognized command ──
        _ => {
            println!("  Unknown command: {} (type 'help' for help)", cmd);
            CommandResult::None
        }
    }
}

/// Display startup banner
fn print_banner(session: &DebugSession) {
    println!("═══════════════════════════════════════════════════════════");
    println!("  Aura Debugger v0.1");
    println!("  Source: {}", session.file_name);
    println!(
        "  Mode: {}  |  Functions: {}  |  Breakpoints: {}",
        session.mode.as_str(),
        session.mapping.functions.len(),
        session.breakpoints.len()
    );
    println!("═══════════════════════════════════════════════════════════");
    println!();
}

/// Display paused state
fn show_paused_state(session: &DebugSession) {
    if let Some(ref reason) = session.stop_reason {
        match reason {
            StopReason::Breakpoint {
                description,
                ..
            } => {
                print!("\n  Breakpoint hit: {}\n", description);
            }
            StopReason::Step { mode } => {
                print!("\n  Stepped ({})\n", mode.as_str());
            }
            StopReason::Completion { result } => {
                print!("\n  Execution complete: {}\n", format_value(result));
            }
            StopReason::Error {
                message,
                at_func,
            } => {
                if let Some(func) = at_func {
                    print!("\n  Runtime error [{}]: {}\n", func, message);
                } else {
                    print!("\n  Runtime error: {}\n", message);
                }
            }
            StopReason::JitCompiled {
                func_name,
                success,
            } => {
                if *success {
                    print!("\n  JIT compilation complete: {}\n", func_name);
                } else {
                    print!("\n  JIT compilation failed: {}\n", func_name);
                }
            }
            StopReason::AotCompiled { path } => {
                print!("\n  AOT compilation complete: {}\n", path);
            }
        }
    }

    // Display current source context
    let line = session.current_line();
    if line > 0 {
        print!("{}", session.show_source(line, 2));
        println!();
    }

    // Display quick action hints
    print!("  (b)reak  (c)ontinue  (s)tep  (n)ext  (l)ist  (h)elp  (q)uit\n\n");
}

/// Display help
fn print_help() {
    println!("  ┌──────────────────────────────────────────────┐");
    println!("  │          Aura Debugger Commands               │");
    println!("  ├──────────────────────────────────────────────┤");
    println!("  │  Breakpoints                                 │");
    println!("  │    b/break <line|function>    Set breakpoint  │");
    println!("  │    d/del <ID>               Delete breakpoint │");
    println!("  │    info b                   List breakpoints  │");
    println!("  │                                              │");
    println!("  │  Execution                                   │");
    println!("  │    c/continue             Continue to next bp │");
    println!("  │    s/step/si              Step in             │");
    println!("  │    n/next/so              Step over           │");
    println!("  │    out                      Step out          │");
    println!("  │                                              │");
    println!("  │  Inspection                                  │");
    println!("  │    bt/backtrace           Show call stack     │");
    println!("  │    l/list [line]          Show source code    │");
    println!("  │    p/print <expr>         Print value         │");
    println!("  │    locals [depth]         Show local variables│");
    println!("  │    stack [depth]          Show operand stack  │");
    println!("  │    info                     VM state summary   │");
    println!("  │    info func                Function list      │");
    println!("  │    info line                Current line/func  │");
    println!("  │    info mode                Current debug mode │");
    println!("  │                                              │");
    println!("  │  Mode                                        │");
    println!("  │    m/mode [vm|jit|aot]      Switch debug mode │");
    println!("  │                                              │");
    println!("  │  JIT (Phase 2)                          │");
    println!("  │    jit state             JIT compilation state│");
    println!("  │    jit fallbacks         JIT fallback details │");
    println!("  │    jit compiled          Compiled functions   │");
    println!("  │                                              │");
    println!("  │  AOT (Phase 3)                          │");
    println!("  │    aot compile           Run AOT compilation │");
    println!("  │    aot dwarf             DWARF debug info    │");
    println!("  │    aot launch            Launch ext. debugger │");
    println!("  │    aot path              Show output path    │");
    println!("  │                                              │");
    println!("  │  Other                                     │");
    println!("  │    h/help/?                 Show help          │");
    println!("  │    q/quit/exit              Exit debugger      │");
    println!("  └──────────────────────────────────────────────┘");
}
