//! std.env — 环境变量
//!
//! 提供环境变量的读取、设置、删除等功能。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.Env.get", nat_get);
    reg.register("aura.lang.std.Env.set", nat_set);
    reg.register("aura.lang.std.Env.remove", nat_remove);
    reg.register("aura.lang.std.Env.has", nat_has);
    reg.register("aura.lang.std.Env.keys", nat_keys);
    reg.register("aura.lang.std.Env.values", nat_values);
    reg.register("aura.lang.std.Env.all", nat_all);
    reg.register("aura.lang.std.Env.home", nat_home);
    reg.register("aura.lang.std.Env.tmp", nat_tmp);
    reg.register("aura.lang.std.Env.pwd", nat_pwd);
    reg.register("aura.lang.std.Env.platform", nat_platform);
    reg.register("aura.lang.std.Env.os", nat_os);
    reg.register("aura.lang.std.Env.arch", nat_arch);
}

/// env.get(name, default) → String
fn nat_get(args: &[Value]) -> Value {
    let name = args.first().map(|v| v.as_string()).unwrap_or_default();
    let default = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    match std::env::var(&name) {
        Ok(val) => Value::str_(val),
        Err(_) => Value::str_(default),
    }
}

/// env.set(name, value) → Unit
fn nat_set(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    unsafe {
        std::env::set_var(args[0].as_string(), args[1].as_string());
    }
    Value::Null
}

/// env.remove(name) → Bool
fn nat_remove(args: &[Value]) -> Value {
    let name = args.first().map(|v| v.as_string()).unwrap_or_default();
    unsafe {
        std::env::remove_var(&name);
    }
    Value::Bool(true)
}

/// env.has(name) → Bool
fn nat_has(args: &[Value]) -> Value {
    let name = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::Bool(std::env::var(&name).is_ok())
}

/// env.keys() → List of variable names
fn nat_keys(_args: &[Value]) -> Value {
    let keys: Vec<Value> = std::env::vars().map(|(k, _)| Value::str_(k)).collect();
    Value::List(keys)
}

/// env.values() → List of values
fn nat_values(_args: &[Value]) -> Value {
    let vals: Vec<Value> = std::env::vars().map(|(_, v)| Value::str_(v)).collect();
    Value::List(vals)
}

/// env.all() → Map of all env vars
fn nat_all(_args: &[Value]) -> Value {
    let mut map = std::collections::HashMap::new();
    for (k, v) in std::env::vars() {
        map.insert(Value::str_(k), Value::str_(v));
    }
    Value::Map(map)
}

/// env.home() → String (home directory)
fn nat_home(_args: &[Value]) -> Value {
    match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(h) => Value::str_(h),
        Err(_) => Value::str_("."),
    }
}

/// env.tmp() → String (temp directory)
fn nat_tmp(_args: &[Value]) -> Value {
    Value::str_(std::env::temp_dir().to_string_lossy())
}

/// env.pwd() → String (current working directory)
fn nat_pwd(_args: &[Value]) -> Value {
    match std::env::current_dir() {
        Ok(p) => Value::str_(p.to_string_lossy()),
        Err(_) => Value::str_("."),
    }
}

/// env.platform() → String ("windows" / "linux" / "macos" / etc.)
fn nat_platform(_args: &[Value]) -> Value {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };
    Value::str_(platform)
}

/// env.os() → String (alias for platform)
fn nat_os(args: &[Value]) -> Value {
    nat_platform(args)
}

/// env.arch() → String (architecture)
fn nat_arch(_args: &[Value]) -> Value {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else {
        "unknown"
    };
    Value::str_(arch)
}
