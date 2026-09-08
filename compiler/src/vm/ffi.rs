//! FFI 回调蹦床与全局回调注册表（P8.7）
//!
//! 对应 技术方案 §3.9 FFI 与系统集成。
//!
//! 设计：
//! - Aura 函数可通过 `MakeCallback` 指令注册为 C 回调
//! - 注册后返回一个回调 ID（正整数），编码为 `Value::Ptr(callback_id)`
//! - C 代码将此 Ptr 作为函数指针调用（context/userdata 参数）
//! - 蹦床函数 `aura_callback_trampoline` 从 context 参数读取回调 ID，
//!   查全局注册表，通过 `CallbackDispatcher` 回调闭包派发回 Aura VM
//! - 蹦床使用固定参数列表（最多 4 个 i64 参数），通过 C ABI 传递

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

/// 回调注册表条目：回调 ID → Aura 函数索引
#[derive(Debug, Clone)]
pub struct CallbackEntry {
    pub func_idx: usize,
}

/// 全局回调注册表（单例，线程安全）
pub struct CallbackRegistry {
    entries: std::sync::Mutex<Vec<CallbackEntry>>,
}

impl CallbackRegistry {
    pub fn new() -> Self {
        CallbackRegistry {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 注册一个 Aura 函数为 C 回调，返回回调 ID
    pub fn register(&self, func_idx: usize) -> i64 {
        let entries = self.entries.lock().unwrap();
        let id = entries.len() + 1; // 1-based，0 = 无效
        drop(entries);
        self.entries.lock().unwrap().push(CallbackEntry { func_idx });
        id as i64
    }

    /// 查找回调 ID 对应的函数索引
    pub fn lookup(&self, callback_id: i64) -> Option<usize> {
        let entries = self.entries.lock().unwrap();
        if callback_id <= 0 || callback_id as usize > entries.len() {
            return None;
        }
        Some(entries[(callback_id as usize) - 1].func_idx)
    }

    /// 注册表中的回调数量
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

impl Default for CallbackRegistry {
    fn default() -> Self {
        CallbackRegistry::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 回调派发器（Closure）
// ─────────────────────────────────────────────────────────────────────────────

/// 回调派发闭包：接收回调 ID 和参数，返回结果
///
/// 由 VM 在调用 C 函数前设置，蹦床函数通过此闭包派发回 Aura VM。
pub type CallbackDispatcher = Arc<dyn Fn(i64, &[i64]) -> i64 + Send + Sync>;

thread_local! {
    /// 当前活跃的回调派发闭包（Phase 1: 保留 thread_local 作为 fast path）
    pub static CURRENT_DISPATCHER: RefCell<Option<CallbackDispatcher>> =
        RefCell::new(None);
}

/// 全局回调派发器栈（Phase 1: thread_local 的跨线程后备）
///
/// 使用栈式结构支持嵌套回调派发：
/// - `set_dispatcher` 同时压栈到 thread_local 和全局栈
/// - `clear_dispatcher` 同时弹栈
/// - 蹦床函数 `aura_callback_trampoline` 先查 thread_local，后备查全局栈
///
/// 此方案解决了 thread_local 的跨线程限制：
/// C 回调从非 VM 线程调用时，可通过全局栈找到 dispatcher。
static DISPATCHER_STACK: Mutex<Vec<CallbackDispatcher>> = Mutex::new(Vec::new());

/// 设置当前回调派发闭包（由 VM 在调用 C 函数前调用）
pub fn set_dispatcher(dispatcher: Arc<dyn Fn(i64, &[i64]) -> i64 + Send + Sync>) {
    CURRENT_DISPATCHER.with(|d| {
        *d.borrow_mut() = Some(dispatcher.clone());
    });
    DISPATCHER_STACK.lock().unwrap().push(dispatcher);
}

/// 清除当前回调派发闭包
pub fn clear_dispatcher() {
    CURRENT_DISPATCHER.with(|d| {
        *d.borrow_mut() = None;
    });
    DISPATCHER_STACK.lock().unwrap().pop();
}

/// 获取当前活跃的回调派发闭包：先查 thread_local（fast path），后备查全局栈
fn current_dispatcher() -> Option<CallbackDispatcher> {
    // Fast path: thread_local
    let local = CURRENT_DISPATCHER.with(|d| d.borrow().clone());
    if local.is_some() {
        return local;
    }
    // Fallback: global stack (cross-thread C callback)
    DISPATCHER_STACK.lock().unwrap().last().cloned()
}

// ─────────────────────────────────────────────────────────────────────────────
// C ABI 蹦床函数
// ─────────────────────────────────────────────────────────────────────────────

/// C 回调蹦床（最多 8 个参数版本）
///
/// 签名：`i64 callback(void* context, i64 a1, i64 a2, ..., i64 a8) -> i64`
///
/// `context` 参数编码回调 ID（由 `Value::Ptr(callback_id)` 提供）。
/// 蹦床读取回调 ID，查全局注册表，通过派发闭包回调回 Aura VM。
///
/// # Safety
///
/// 此函数通过 C ABI 暴露，由 C 代码调用。调用者必须保证：
/// - `context` 参数指向有效的回调 ID
/// - 参数数量不超过 8 个
#[unsafe(no_mangle)]
pub unsafe extern "C" fn aura_callback_trampoline(
    context: *mut std::ffi::c_void,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
    a8: i64,
) -> i64 {
    let callback_id = context as usize as i64;
    let args = [
        a1, a2, a3, a4, a5, a6, a7, a8,
    ];

    if let Some(dispatcher) = current_dispatcher() {
        dispatcher(callback_id, &args)
    } else {
        // 无活跃派发器：返回 0（未链接）
        eprintln!("[ffi] 回调 #{} 被调用但无活跃派发器，已忽略", callback_id);
        0
    }
}

/// 获取蹦床函数指针（用于传递给 C 代码）
pub fn trampoline_ptr() -> i64 {
    aura_callback_trampoline as *const () as usize as i64
}

// ─────────────────────────────────────────────────────────────────────────────
// 静态链接：C 函数解析（P8.4）
// ─────────────────────────────────────────────────────────────────────────────

/// C 类型枚举（Fix 7）：用于类型安全的 C 函数调用
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CType {
    /// 32 位有符号整数
    Int32,
    /// 64 位有符号整数
    Int64,
    /// 32 位浮点数
    Float32,
    /// 64 位浮点数
    Float64,
    /// 布尔值（1 字节）
    Bool,
    /// 字符（1 字节）
    Char,
    /// C 字符串（char*）
    CString,
    /// 指针（void*）
    Ptr,
    /// 无返回值
    Void,
}

impl CType {
    /// 从字符串解析 C 类型
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "int32" | "int32_t" | "i32" => Some(CType::Int32),
            "int64" | "int64_t" | "i64" => Some(CType::Int64),
            "float" | "f32" => Some(CType::Float32),
            "double" | "f64" => Some(CType::Float64),
            "bool" | "boolean" => Some(CType::Bool),
            "char" | "u8" => Some(CType::Char),
            "cstring" | "string" | "char*" => Some(CType::CString),
            "ptr" | "pointer" | "void*" => Some(CType::Ptr),
            "void" | "unit" => Some(CType::Void),
            _ => None,
        }
    }

    /// 将 Aura Value 转换为 i64（C ABI 传递格式）
    pub fn pack(&self, value: &crate::vm::Value) -> i64 {
        match (*self, value) {
            (CType::Int32 | CType::Int64, crate::vm::Value::Int(i)) => *i,
            (CType::Float32 | CType::Float64, crate::vm::Value::Float(f)) => f.to_bits() as i64,
            (CType::Bool, crate::vm::Value::Bool(b)) => *b as i64,
            (CType::Char, crate::vm::Value::Int(c)) => *c,
            // P9: 指针类型 — 支持 Ptr 和 Int（handle 作为 Long 存储）
            (CType::CString | CType::Ptr, crate::vm::Value::Ptr(p)) => *p,
            (CType::CString | CType::Ptr, crate::vm::Value::Int(i)) => *i, // handle 作为 Int 传递
            (CType::CString, crate::vm::Value::Str(s)) => s.as_ptr() as i64,
            _ => 0,
        }
    }

    /// 将 i64 结果转换回 Aura Value
    pub fn unpack(&self, result: i64) -> crate::vm::Value {
        match self {
            // Int32: 只取低 32 位（x86_64 调用约定中 i32 返回值在 RAX 低 32 位）
            CType::Int32 => crate::vm::Value::Int(result as i32 as i64),
            CType::Int64 => crate::vm::Value::Int(result),
            CType::Float32 => crate::vm::Value::Float(f64::from_bits((result as u32) as u64)),
            CType::Float64 => crate::vm::Value::Float(f64::from_bits(result as u64)),
            CType::Bool => crate::vm::Value::Bool(result != 0),
            CType::Char => crate::vm::Value::Int(result as i32 as i64),
            // P9: CString 返回值 — 从指针读取字符串内容
            CType::CString => {
                if result == 0 {
                    crate::vm::Value::Null
                } else {
                    // 安全地从 C 字符串指针读取
                    let ptr = result as *const std::os::raw::c_char;
                    unsafe {
                        if let Ok(c_str) = std::ffi::CStr::from_ptr(ptr).to_str() {
                            crate::vm::Value::Str(std::rc::Rc::from(c_str))
                        } else {
                            crate::vm::Value::Null
                        }
                    }
                }
            }
            CType::Ptr => crate::vm::Value::Int(result), // P9: 指针作为 Long 返回
            CType::Void => crate::vm::Value::Null,
        }
    }
}

/// C 函数信息（Fix 7）：存储参数类型和返回类型
#[derive(Debug, Clone)]
pub struct CFuncInfo {
    /// 函数名
    pub name: String,
    /// 参数类型列表
    pub param_types: Vec<CType>,
    /// 返回类型
    pub return_type: CType,
}

/// 通用 C 函数签名（最多 8 个参数，返回 i64）
///
/// 通过 dlsym/GetProcAddress 解析 C 函数符号后，包装为此签名调用。
/// 参数类型由 `CFuncInfo` 中的类型信息在调用前转换。
pub type CFuncPtr = unsafe extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64;

/// 存储解析后的 C 函数指针
#[cfg(feature = "dynamic-ffi")]
pub struct ResolvedCFunc {
    pub name: String,
    /// 原始符号地址
    pub symbol: usize,
    /// 通用调用指针（4 参数版本）
    pub call_ptr: CFuncPtr,
}

#[cfg(not(feature = "dynamic-ffi"))]
pub struct ResolvedCFunc {
    pub name: String,
    pub symbol: usize,
}

/// 从当前进程中解析 C 函数符号（P8.4 静态链接）
///
/// 使用 `dlsym(NULL, name)`（Unix）或 `GetProcAddress(GetModuleHandle(NULL), name)`（Windows）。
/// 成功时返回函数指针地址，失败时返回 None。
pub fn resolve_static_symbol(name: &str) -> Option<usize> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::raw::c_void;

        // 动态链接器需要句柄，NULL 表示当前进程
        let handle: *mut c_void = dlopen_null_handle();
        if handle.is_null() {
            return None;
        }
        let c_name = CString::new(name).ok()?;
        let ptr = unsafe { dlsym(handle, c_name.as_ptr()) };
        if ptr.is_null() { None } else { Some(ptr as usize) }
    }

    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let handle = get_module_handle_null();
        if handle == 0 {
            return None;
        }
        let wide: Vec<u16> = OsStr::new(name).encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            let ptr = get_proc_address(handle, wide.as_ptr());
            if ptr == 0 { None } else { Some(ptr as usize) }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = name;
        None
    }
}

#[cfg(unix)]
fn dlopen_null_handle() -> *mut std::os::raw::c_void {
    // 使用 RTLD_DEFAULT 等价：直接返回 null（dlsym 接受 null 表示当前进程）
    std::ptr::null_mut()
}

#[cfg(windows)]
fn get_module_handle_null() -> usize {
    // GetModuleHandleW(NULL) 返回当前进程模块句柄
    unsafe { windows_get_module_handle() }
}

#[cfg(windows)]
unsafe fn windows_get_module_handle() -> usize {
    // 使用 GetModuleHandleW(NULL)
    unsafe {
        unsafe extern "system" {
            fn GetModuleHandleW(module_name: *const u16) -> usize;
        }
        GetModuleHandleW(std::ptr::null())
    }
}

#[cfg(windows)]
unsafe fn get_proc_address(handle: usize, name: *const u16) -> usize {
    unsafe {
        unsafe extern "system" {
            fn GetProcAddress(module: usize, proc_name: *const u16) -> usize;
        }
        GetProcAddress(handle, name)
    }
}

#[cfg(unix)]
unsafe fn dlsym(
    handle: *mut std::os::raw::c_void,
    name: *const std::os::raw::c_char,
) -> *mut std::os::raw::c_void {
    unsafe {
        unsafe extern "C" {
            fn dlsym(
                handle: *mut std::os::raw::c_void,
                symbol: *const std::os::raw::c_char,
            ) -> *mut std::os::raw::c_void;
        }
        dlsym(handle, name)
    }
}
