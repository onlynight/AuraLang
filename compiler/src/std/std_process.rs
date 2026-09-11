//! std.process — 进程管理
//!
//! 提供进程退出、参数获取、退出码等基础功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.Process.exit", nat_exit);
    reg.register("aura.lang.std.Process.exitCode", nat_exit_code);
    reg.register("aura.lang.std.Process.args", nat_args);
    reg.register("aura.lang.std.Process.arg", nat_arg);
    reg.register("aura.lang.std.Process.argCount", nat_arg_count);
    reg.register("aura.lang.std.Process.pid", nat_pid);
    reg.register("aura.lang.std.Process.spawn", nat_spawn);
    reg.register("aura.lang.std.Process.kill", nat_kill);
    reg.register("aura.lang.std.Process.wait", nat_wait);
    reg.register("aura.lang.std.Process.exitProcess", nat_exit_process);
}

/// `Process.exit(code)` → 退出进程。
///
/// 注意：`Process` 是 object 单例，方法调用会**注入 self 作为第 0 个参数**
/// （实际 `argc = 1 + 形参个数`）。因此退出码取自**最后一个**数值参数，
/// 而不是 `args[0]`（此前误取 args[0] 导致退出码恒为 0）。
fn nat_exit(args: &[Value]) -> Value {
    let code = last_int_arg(args).unwrap_or(0) as i32;
    std::process::exit(code);
}

/// `nat_exit` 的公开包装（供 prelude 短名别名注册使用）。
pub fn nat_exit_pub(args: &[Value]) -> Value {
    nat_exit(args)
}

/// 取参数列表中最后一个整数（兼容 `Value::Float` 表示的整数）。
pub(crate) fn last_int_arg(args: &[Value]) -> Option<i64> {
    args.iter().rev().find_map(|v| match v {
        Value::Int(n) => Some(*n),
        Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
        _ => None,
    })
}

/// process.exitCode() → Int (default 0)
fn nat_exit_code(_args: &[Value]) -> Value {
    Value::Int(0)
}

/// process.args() → List of command-line arguments
fn nat_args(_args: &[Value]) -> Value {
    let args: Vec<Value> = std::env::args().map(|a| Value::str_(a)).collect();
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
    let cmd_args: Vec<String> = args.iter().skip(1).map(|v| v.as_string()).collect();
    match std::process::Command::new(&cmd).args(&cmd_args).spawn() {
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
            if libc::kill(pid as i32, 9) == 0 { Value::Bool(true) } else { Value::Bool(false) }
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
