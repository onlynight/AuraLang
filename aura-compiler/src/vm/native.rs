//! 原生（内置 / FFI）函数调度器
//!
//! 对应 技术方案 §7.1 的 `CallNative` / `CallC` 与 §9.3 的 FFI 调度。
//!
//! 原生函数签名统一为 `fn(&[Value]) -> Value`：参数已从操作数栈按声明顺序弹出，
//! 返回值压回操作数栈。`println` 等内置函数由 VM 启动时自动注册；`extern "c"`
//! 声明的函数（如 `puts`）若未在运行时链接，则回退为打印其参数的占位实现，
//! 保证字节码可继续执行而不崩溃。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vm::dynamic_ffi::DynamicLoader;
use crate::vm::value::Value;

/// 原生函数指针类型
pub type NativeFn = fn(&[Value]) -> Value;

/// 原生函数注册表
#[derive(Default)]
pub struct NativeRegistry {
    fns: HashMap<String, NativeFn>,
    /// 动态加载器（5.9）：未内置的原生函数从动态库查找
    dynamic: DynamicLoader,
}

impl NativeRegistry {
    /// 创建并注册全部内置原生函数
    pub fn new() -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };
        r.register("println", native_println);
        r.register("print", native_print);
        r.register("puts", native_puts);
        r.register("abs", native_abs);
        r.register("sqrt", native_sqrt);
        r.register("pow", native_pow);
        r.register("toInt", native_to_int);
        r.register("toFloat", native_to_float);
        r.register("toStr", native_to_str);
        r.register("clock", native_clock);
        r.register("strlen", native_strlen);
        r
    }

    pub fn register(&mut self, name: &str, f: NativeFn) {
        self.fns.insert(name.to_string(), f);
    }

    /// 查找原生函数（优先内置表，其次动态加载表）
    pub fn get(&self, name: &str) -> Option<NativeFn> {
        self.fns
            .get(name)
            .copied()
            .or_else(|| self.dynamic.get(name))
    }

    /// 是否存在该名称的原生函数（内置或动态）
    pub fn contains(&self, name: &str) -> bool {
        self.fns.contains_key(name) || self.dynamic.contains(name)
    }

    /// 动态加载库（5.9）
    pub fn load_library(&mut self, path: &str) -> Result<(), String> {
        self.dynamic.load_lib(path)
    }

    /// 从动态加载的库注册函数
    pub fn register_dynamic(&mut self, name: &str, f: NativeFn) {
        self.dynamic.register_func(name, f);
    }

    /// 获取动态加载器的引用
    pub fn dynamic_loader(&self) -> &DynamicLoader {
        &self.dynamic
    }

    /// 获取动态加载器的可变引用
    pub fn dynamic_loader_mut(&mut self) -> &mut DynamicLoader {
        &mut self.dynamic
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 内置实现
// ─────────────────────────────────────────────────────────────────────────────

fn native_println(args: &[Value]) -> Value {
    let mut s = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&a.to_string());
    }
    println!("{}", s);
    Value::Null
}

fn native_print(args: &[Value]) -> Value {
    let mut s = String::new();
    for a in args {
        s.push_str(&a.to_string());
    }
    print!("{}", s);
    // 确保即时刷新（无换行时）
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Value::Null
}

fn native_puts(args: &[Value]) -> Value {
    // C 风格 puts：输出并换行
    if let Some(a) = args.first() {
        println!("{}", a);
    } else {
        println!();
    }
    Value::Int(0)
}

fn native_abs(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Int(i.abs()),
        Some(Value::Float(f)) => Value::Float(f.abs()),
        _ => Value::Int(0),
    }
}

fn native_sqrt(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Float(v.as_float().sqrt()),
        None => Value::Float(0.0),
    }
}

fn native_pow(args: &[Value]) -> Value {
    let base = args.first().map(|v| v.as_float()).unwrap_or(0.0);
    let exp = args.get(1).map(|v| v.as_float()).unwrap_or(0.0);
    Value::Float(base.powf(exp))
}

fn native_to_int(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Bool(b)) => Value::Int(*b as i64),
        Some(v) => Value::Int(v.as_int()),
        None => Value::Int(0),
    }
}

fn native_to_float(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Float(v.as_float()),
        None => Value::Float(0.0),
    }
}

fn native_to_str(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::str_(v.to_string()),
        None => Value::str_(""),
    }
}

fn native_clock(args: &[Value]) -> Value {
    let _ = args;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    Value::Float(now)
}

fn native_strlen(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.chars().count() as i64),
        _ => Value::Int(0),
    }
}
