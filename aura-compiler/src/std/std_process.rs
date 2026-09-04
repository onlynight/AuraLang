//! std.process — 进程管理
//!
//! 提供进程退出、参数获取、退出码等基础功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.process.exit", nat_exit);
    reg.register("aura.process.exitCode", nat_exit_code);
    reg.register("aura.process.args", nat_args);
    reg.register("aura.process.arg", nat_arg);
    reg.register("aura.process.argCount", nat_arg_count);
    reg.register("aura.process.pid", nat_pid);
    reg.register("aura.process.spawn", nat_spawn);
    reg.register("aura.process.kill", nat_kill);
    reg.register("aura.process.wait", nat_wait);
    reg.register("aura.process.exitProcess", nat_exit_process);
}

/// process.exit(code) → exits the process
fn nat_exit(args: &[Value]) -> Value {
    let code = args.first().map(|v| v.as_int()).unwrap_or(0) as i32;
    std::process::exit(code);
}

/// process.exitCode() → Int (default 0)
fn nat_exit_code(_args: &[Value]) -> Value {
    Value::Int(0)
}

/// process.args() → List of command-line arguments
fn nat_args(_args: &[Value]) -> Value {
    let args: Vec<Value> = std::env::args()
        .map(|a| Value::str_(a))
        .collect();
    Value::List(args)
}

/// process.arg(index) → String or null
fn nat_arg(args: &[Value]) -> Value {
    let idx = args.first().map(|v| v.as_int() as usize).unwrap_or(0);
    match std::env::args().nth(idx) {
        Some(a) => Value::str_(a),
        None => Value::Null,
    }
}

/// process.argCount() → Int
fn nat_arg_count(_args: &[Value]) -> Value {
    Value::Int(std::env::args().count() as i64)
}

/// process.pid() → Int (process ID)
fn nat_pid(_args: &[Value]) -> Value {
    Value::Int(std::process::id() as i64)
}

/// process.spawn(command, args) → handle or error string
fn nat_spawn(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::str_("spawn: no command");
    }
    let cmd = args[0].as_string();
    let cmd_args: Vec<String> = args
        .iter()
        .skip(1)
        .map(|v| v.as_string())
        .collect();
    match std::process::Command::new(&cmd)
        .args(&cmd_args)
        .spawn()
    {
        Ok(child) => {
            let pid = child.id() as i64;
            // Store child process ID
            Value::Int(pid)
        }
        Err(e) => Value::str_(format!("spawn error: {}", e)),
    }
}

/// process.kill(pid) → Bool
fn nat_kill(args: &[Value]) -> Value {
    let _pid = args.first().map(|v| v.as_int()).unwrap_or(0) as u32;
    #[cfg(unix)]
    {
        unsafe {
            if libc::kill(pid as i32, 9) == 0 {
                Value::Bool(true)
            } else {
                Value::Bool(false)
            }
        }
    }
    #[cfg(windows)]
    {
        Value::Bool(false) // Not implemented on Windows
    }
}

/// process.wait() → Unit (waits for child process)
fn nat_wait(args: &[Value]) -> Value {
    // Simplified: wait for a short duration
    let pid = args.first().map(|v| v.as_int()).unwrap_or(0);
    if pid > 0 {
        #[cfg(unix)]
        unsafe {
            let mut status = 0i32;
            libc::waitpid(pid as i32, &mut status, 0);
        }
    }
    Value::Null
}

/// process.exitProcess(code) → Unit (alias for exit)
fn nat_exit_process(args: &[Value]) -> Value {
    nat_exit(args)
}
