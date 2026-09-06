//! Aura 调试器交互式 REPL
//!
//! 命令：break / continue / step / next / backtrace / list / print / locals
//!      stack / info / mode / del / help / quit

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use compiler::vm::debugger::{BreakpointTarget, DebugMode, DebugSession, StopReason, format_value};

/// 调试器 REPL 主入口
pub fn run_debugger(session: &mut DebugSession) {
    print_banner(session);

    // 初始化 VM
    if let Err(e) = session.initialize() {
        eprintln!("错误: 调试器初始化失败: {}", e);
        return;
    }

    loop {
        // 显示当前暂停状态
        if session.paused {
            show_paused_state(session);
        }

        // 显示 prompt
        let prompt = if session.paused { "> " } else { "  " };
        eprint!("{}", prompt);
        io::stdout().flush().ok();

        // 读取命令
        let stdin = io::stdin();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF
                println!("\n再见!");
                session.cleanup();
                return;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("读取输入失败: {}", e);
                return;
            }
        }

        let cmd = line.trim().to_lowercase();

        if cmd.is_empty() {
            // 空行：如果暂停则重新显示状态
            if session.paused {
                show_paused_state(session);
            }
            continue;
        }

        // 执行命令
        match execute_command(session, &line) {
            CommandResult::Exit => {
                println!("再见!");
                session.cleanup();
                return;
            }
            CommandResult::Run => {
                // 运行 VM
                match session.run() {
                    Ok(reason) => {
                        match reason {
                            StopReason::Breakpoint {
                                description,
                                ..
                            } => {
                                println!("\n  断点命中: {}", description);
                            }
                            StopReason::Step { mode } => {
                                println!("\n  单步暂停 ({}):", mode.as_str());
                            }
                            StopReason::Completion { result } => {
                                println!("\n  执行完成: {}", format_value(&result));
                                session.step_mode = compiler::vm::debugger::StepMode::Off;
                            }
                            StopReason::Error {
                                message,
                                at_func,
                            } => {
                                if let Some(func) = at_func {
                                    eprintln!("\n  运行时错误 [{}]: {}", func, message);
                                } else {
                                    eprintln!("\n  运行时错误: {}", message);
                                }
                            }
                            StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {
                                // 仅 JIT/AOT 模式
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("执行错误: {}", e);
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
                // 立即运行
                match session.run() {
                    Ok(reason) => match reason {
                        StopReason::Breakpoint {
                            description,
                            ..
                        } => {
                            println!("\n  断点命中: {}", description);
                        }
                        StopReason::Step { mode } => {
                            println!("\n  单步暂停 ({}):", mode.as_str());
                        }
                        StopReason::Completion { result } => {
                            println!("\n  执行完成: {}", format_value(&result));
                            session.step_mode = compiler::vm::debugger::StepMode::Off;
                        }
                        StopReason::Error {
                            message,
                            at_func,
                        } => {
                            if let Some(func) = at_func {
                                eprintln!("\n  运行时错误 [{}]: {}", func, message);
                            } else {
                                eprintln!("\n  运行时错误: {}", message);
                            }
                        }
                        StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {}
                    },
                    Err(e) => {
                        eprintln!("执行错误: {}", e);
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
                // 执行一步
                match session.run() {
                    Ok(reason) => match reason {
                        StopReason::Breakpoint {
                            description,
                            ..
                        } => {
                            println!("\n  断点命中: {}", description);
                        }
                        StopReason::Step { mode } => {
                            println!("\n  单步暂停 ({}):", mode.as_str());
                        }
                        StopReason::Completion { result } => {
                            println!("\n  执行完成: {}", format_value(&result));
                            session.step_mode = compiler::vm::debugger::StepMode::Off;
                        }
                        StopReason::Error {
                            message,
                            at_func,
                        } => {
                            if let Some(func) = at_func {
                                eprintln!("\n  运行时错误 [{}]: {}", func, message);
                            } else {
                                eprintln!("\n  运行时错误: {}", message);
                            }
                        }
                        StopReason::JitCompiled { .. } | StopReason::AotCompiled { .. } => {}
                    },
                    Err(e) => {
                        eprintln!("执行错误: {}", e);
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

/// 命令执行结果
enum CommandResult {
    None,
    Run,
    Continue,
    Step { mode: StepMode },
    Exit,
}

use compiler::vm::debugger::StepMode;

/// 执行命令
fn execute_command(session: &mut DebugSession, raw_line: &str) -> CommandResult {
    let line = raw_line.trim();
    if line.is_empty() {
        return CommandResult::None;
    }

    // 分割命令和参数
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let args = parts.next().unwrap_or("").trim().to_string();

    match cmd.as_str() {
        // ── 断点命令 ──
        "break" | "b" => {
            if args.is_empty() {
                println!("  用法: break <行号|函数名>");
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
                    println!("  断点 #{} 已设置", id);
                }
                Err(e) => {
                    eprintln!("  设置断点失败: {}", e);
                }
            }
            CommandResult::None
        }

        // ── 删除断点 ──
        "del" | "d" => {
            if let Ok(id) = args.parse::<usize>() {
                if session.delete_breakpoint(id) {
                    println!("  断点 #{} 已删除", id);
                } else {
                    println!("  断点 #{} 不存在", id);
                }
            } else {
                println!("  用法: del <断点ID>");
            }
            CommandResult::None
        }

        // ── 继续执行 ──
        "continue" | "c" => CommandResult::Continue,

        // ── 单步命令 ──
        "step" | "s" | "si" => CommandResult::Step {
            mode: StepMode::In,
        },
        "next" | "n" | "so" => CommandResult::Step {
            mode: StepMode::Over,
        },
        "out" => CommandResult::Step {
            mode: StepMode::Out,
        },

        // ── 调用栈 ──
        "backtrace" | "bt" => {
            println!("{}", session.show_backtrace());
            CommandResult::None
        }

        // ── 源码显示 ──
        "list" | "l" => {
            let center = if args.is_empty() {
                session.current_line()
            } else {
                args.parse::<usize>().unwrap_or(session.current_line())
            };
            println!("{}", session.show_source(center, 3));
            CommandResult::None
        }

        // ── 打印值 ──
        "print" | "p" => {
            if args.is_empty() {
                println!("  用法: print <表达式>");
            } else {
                // 简化：在栈顶查找值
                if let Some(vm) = session.vm.as_ref() {
                    if let Some(frame) = vm.frames().last() {
                        if let Some(val) = frame.stack.last() {
                            println!("  栈顶 = {}", format_value(val));
                        } else if let Some(func_idx) = session.current_func_idx() {
                            if let Some(func) = vm.module_ref().funcs.get(func_idx) {
                                let last_slot = func.locals as usize - 1;
                                if last_slot < frame.locals.len() {
                                    println!(
                                        "  最后局部变量 = {}",
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

        // ── 局部变量 ──
        "locals" => {
            let depth = if args.is_empty() { None } else { args.parse::<usize>().ok() };
            println!("{}", session.show_locals(depth));
            CommandResult::None
        }

        // ── 操作数栈 ──
        "stack" => {
            let depth = if args.is_empty() { None } else { args.parse::<usize>().ok() };
            println!("{}", session.show_stack(depth));
            CommandResult::None
        }

        // ── 信息命令 ──
        "info" => {
            if args.is_empty() || args == "all" {
                println!("{}", session.show_info());
            } else if args == "functions" || args == "func" {
                println!("{}", session.show_functions());
            } else if args == "breakpoints" || args == "b" {
                println!("{}", session.list_breakpoints());
            } else if args == "mode" {
                println!("  当前模式: {}", session.mode.as_str());
            } else if args == "line" {
                println!("  当前行: {}", session.current_line());
                if let Some(func) = session.current_function() {
                    println!("  当前函数: {}", func);
                }
            } else {
                println!("  用法: info [functions|breakpoints|mode|line|all]");
            }
            CommandResult::None
        }

        // ── 模式切换 ──
        "mode" | "m" => {
            if args.is_empty() {
                println!("  当前模式: {}", session.mode.as_str());
                println!("  可用模式: vm, jit, aot");
            } else {
                match args.as_str() {
                    "vm" => {
                        session.mode = DebugMode::Vm;
                        println!("  模式切换为: VM");
                    }
                    "jit" => {
                        session.mode = DebugMode::Jit;
                        println!("  模式切换为: JIT");
                    }
                    "aot" => {
                        session.mode = DebugMode::Aot;
                        println!("  模式切换为: AOT");
                    }
                    _ => {
                        println!("  未知模式: {}", args);
                    }
                }
            }
            CommandResult::None
        }

        // ── 函数列表 ──
        "functions" | "funcs" => {
            println!("{}", session.show_functions());
            CommandResult::None
        }

        // ── 断点列表 ──
        "breakpoints" | "bps" => {
            println!("{}", session.list_breakpoints());
            CommandResult::None
        }

        // ── Phase 2: JIT 命令 ──
        "jit" => {
            if args.is_empty() {
                println!("  JIT 子命令: state, fallbacks, compiled");
                println!("    jit state      显示 JIT 编译状态");
                println!("    jit fallbacks  显示 JIT 回退详情");
                println!("    jit compiled   显示已编译函数");
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
                            "  未知 JIT 子命令: {} (可用: state, fallbacks, compiled)",
                            args
                        );
                    }
                }
            }
            CommandResult::None
        }

        // ── Phase 3: AOT 命令 ──
        "aot" => {
            if args.is_empty() {
                println!("  AOT 子命令: compile, dwarf, launch, path");
                println!("    aot compile  执行 AOT 编译");
                println!("    aot dwarf    显示 DWARF 调试信息");
                println!("    aot launch   启动外部调试器 (lldb/gdb)");
                println!("    aot path     显示输出路径");
            } else {
                #[cfg(feature = "llvm")]
                match args.as_str() {
                    "compile" => match session.aot_compile() {
                        Ok(summary) => println!("{}", summary),
                        Err(e) => eprintln!("  AOT 编译失败: {}", e),
                    },
                    _ => {}
                }
                #[cfg(not(feature = "llvm"))]
                {
                    println!("  AOT 功能需要 llvm feature (cargo build --features llvm)");
                }

                if args == "dwarf" {
                    println!("{}", session.show_aot_dwarf());
                } else if args == "path" {
                    println!("{}", session.show_aot_path());
                } else if args == "launch" {
                    match session.aot_launch() {
                        Ok(msg) => println!("{}", msg),
                        Err(e) => eprintln!("  启动失败: {}", e),
                    }
                } else if args != "compile" {
                    println!(
                        "  未知 AOT 子命令: {} (可用: compile, dwarf, launch, path)",
                        args
                    );
                }
            }
            CommandResult::None
        }

        // ── 帮助 ──
        "help" | "h" | "?" => {
            print_help();
            CommandResult::None
        }

        // ── 退出 ──
        "quit" | "q" | "exit" | "e" => CommandResult::Exit,

        // ── 未识别命令 ──
        _ => {
            println!("  未知命令: {} (输入 help 查看帮助)", cmd);
            CommandResult::None
        }
    }
}

/// 显示启动横幅
fn print_banner(session: &DebugSession) {
    println!("═══════════════════════════════════════════════════════════");
    println!("  Aura 调试器 v0.1");
    println!("  源码: {}", session.file_name);
    println!(
        "  模式: {}  |  函数: {}  |  断点: {}",
        session.mode.as_str(),
        session.mapping.functions.len(),
        session.breakpoints.len()
    );
    println!("═══════════════════════════════════════════════════════════");
    println!();
}

/// 显示暂停状态
fn show_paused_state(session: &DebugSession) {
    if let Some(ref reason) = session.stop_reason {
        match reason {
            StopReason::Breakpoint {
                description,
                ..
            } => {
                print!("\n  断点命中: {}\n", description);
            }
            StopReason::Step { mode } => {
                print!("\n  单步暂停 ({})\n", mode.as_str());
            }
            StopReason::Completion { result } => {
                print!("\n  执行完成: {}\n", format_value(result));
            }
            StopReason::Error {
                message,
                at_func,
            } => {
                if let Some(func) = at_func {
                    print!("\n  运行时错误 [{}]: {}\n", func, message);
                } else {
                    print!("\n  运行时错误: {}\n", message);
                }
            }
            StopReason::JitCompiled {
                func_name,
                success,
            } => {
                if *success {
                    print!("\n  JIT 编译完成: {}\n", func_name);
                } else {
                    print!("\n  JIT 编译失败: {}\n", func_name);
                }
            }
            StopReason::AotCompiled { path } => {
                print!("\n  AOT 编译完成: {}\n", path);
            }
        }
    }

    // 显示当前源码上下文
    let line = session.current_line();
    if line > 0 {
        print!("{}", session.show_source(line, 2));
        println!();
    }

    // 显示快速操作提示
    print!("  (b)reak  (c)ontinue  (s)tep  (n)ext  (l)ist  (h)elp  (q)uit\n\n");
}

/// 显示帮助
fn print_help() {
    println!("  ┌──────────────────────────────────────────────┐");
    println!("  │            Aura 调试器命令列表                │");
    println!("  ├──────────────────────────────────────────────┤");
    println!("  │  断点                                        │");
    println!("  │    b/break <行号|函数名>    设置断点          │");
    println!("  │    d/del <ID>               删除断点          │");
    println!("  │    info b                   列出断点          │");
    println!("  │                                              │");
    println!("  │  执行                                        │");
    println!("  │    c/continue             继续到下一个断点    │");
    println!("  │    s/step/si              步入（下一条指令）  │");
    println!("  │    n/next/so              步过（不进入子函数）│");
    println!("  │    out                      步出（到函数返回）│");
    println!("  │                                              │");
    println!("  │  检查                                        │");
    println!("  │    bt/backtrace           显示调用栈          │");
    println!("  │    l/list [行号]           显示源码            │");
    println!("  │    p/print <表达式>         打印值            │");
    println!("  │    locals [深度]            显示局部变量       │");
    println!("  │    stack [深度]             显示操作数栈       │");
    println!("  │    info                     VM 状态摘要        │");
    println!("  │    info func                函数列表           │");
    println!("  │    info line                当前行/函数        │");
    println!("  │    info mode                当前调试模式       │");
    println!("  │                                              │");
    println!("  │  模式                                        │");
    println!("  │    m/mode [vm|jit|aot]      切换调试模式      │");
    println!("  │                                              │");
    println!("  │  JIT (Phase 2)                          │");
    println!("  │    jit state             JIT 编译状态       │");
    println!("  │    jit fallbacks         JIT 回退详情       │");
    println!("  │    jit compiled          已编译函数         │");
    println!("  │                                              │");
    println!("  │  AOT (Phase 3)                          │");
    println!("  │    aot compile           执行 AOT 编译      │");
    println!("  │    aot dwarf             DWARF 调试信息     │");
    println!("  │    aot launch            启动外部调试器     │");
    println!("  │    aot path              显示输出路径       │");
    println!("  │                                              │");
    println!("  │  其他                                        │");
    println!("  │    h/help/?                 显示帮助          │");
    println!("  │    q/quit/exit              退出调试器        │");
    println!("  └──────────────────────────────────────────────┘");
}
