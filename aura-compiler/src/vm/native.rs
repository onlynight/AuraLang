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
        // P8.5: CString / CStr
        r.register("CString", native_cstring);
        r.register("CStr", native_cstr);
        // P8.6: Pointer / nullptr
        r.register("ptrIsNull", native_ptr_is_null);
        r.register("ptrToInt", native_ptr_to_int);
        r.register("intToPtr", native_int_to_ptr);
        // P8.7: 回调
        r.register("makeCallback", native_make_callback);
        // P9: 标准库
        crate::std::register_all(&mut r);
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

    /// 静态链接：从当前进程中解析 C 函数符号并注册（P8.4）
    ///
    /// 使用 `dlsym(NULL, name)`（Unix）或 `GetProcAddress`（Windows）查找函数。
    /// 返回 C 函数地址，由调用方直接调用。
    pub fn try_static_link(&self, name: &str) -> Option<usize> {
        use crate::vm::ffi::resolve_static_symbol;
        resolve_static_symbol(name)
    }

    /// 尝试解析 C 函数：先查内置表，再查动态表
    pub fn resolve_c_function(&self, name: &str) -> Option<NativeFn> {
        if let Some(f) = self.fns.get(name).copied() {
            return Some(f);
        }
        if let Some(f) = self.dynamic.get(name) {
            return Some(f);
        }
        None
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

// ─────────────────────────────────────────────────────────────────────────────
// P8 FFI 内置函数
// ─────────────────────────────────────────────────────────────────────────────

/// CString(str) → Ptr：将 Aura 字符串转换为 C 字符串指针（P8.5）
///
/// 实际实现：通过 `CString` 指令完成转换，此处作为占位返回 Ptr(0)。
/// 完整实现需 VM 端支持（见 interp.rs CString 指令）。
fn native_cstring(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Str(_s)) => {
            // 返回一个非空指针占位（实际 C 字符串由 CString 指令分配）
            Value::Ptr(1)
        }
        _ => Value::Ptr(0),
    }
}

/// CStr(str) → Ptr：CString 的别名（P8.5）
fn native_cstr(args: &[Value]) -> Value {
    native_cstring(args)
}

/// ptrIsNull(ptr) → Bool：检查指针是否为 nullptr（P8.6）
fn native_ptr_is_null(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Bool(v.is_null_ptr()),
        _ => Value::Bool(true),
    }
}

/// ptrToInt(ptr) → Int：将指针转换为整数地址（P8.6）
fn native_ptr_to_int(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Int(v.as_ptr()),
        _ => Value::Int(0),
    }
}

/// intToPtr(n) → Ptr：将整数地址转换为指针（P8.6）
fn native_int_to_ptr(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Ptr(v.as_int()),
        _ => Value::Ptr(0),
    }
}

/// makeCallback(funcName) → Ptr：创建 C 回调蹦床（P8.7）
///
/// 返回的 Ptr 包含回调 ID，C 代码将其作为函数指针调用时，
/// 蹦床通过 thread-local 派发回 Aura VM 执行对应函数。
fn native_make_callback(_args: &[Value]) -> Value {
    // makeCallback 由 MakeCallback 指令处理（见 mir.rs / emit.rs）
    // 此处作为占位：如果通过 CallNative 调用，返回无效回调 ID
    Value::Ptr(0)
}
