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
    /// 创建并注册全部内置原生函数（向后兼容）
    pub fn new() -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };

        // 注册 prelude（17 个全局内置，始终存在）
        r.register("println", native_println);
        r.register("print", native_print);
        r.register("puts", native_puts);
        r.register("abs", native_abs);
        r.register("sqrt", native_sqrt);
        r.register("pow", native_pow);
        r.register("toInt", native_to_int);
        r.register("toFloat", native_to_float);
        r.register("toStr", native_to_str);
        r.register("toString", native_to_str); // alias for toStr, used as method call
        r.register("clock", native_clock);
        r.register("strlen", native_strlen);
        r.register("CString", native_cstring);
        r.register("CStr", native_cstr);
        r.register("ptrIsNull", native_ptr_is_null);
        r.register("ptrToInt", native_ptr_to_int);
        r.register("intToPtr", native_int_to_ptr);
        r.register("makeCallback", native_make_callback);

        // 注册全部 std 模块（向后兼容）
        crate::std::register_all(&mut r);

        // P10: 并发运行时（需 std-concurrent feature）
        #[cfg(feature = "std-concurrent")]
        {
            r.register("aura.concurrent.spawn", native_spawn);
            r.register("aura.concurrent.send", native_send);
            r.register("aura.concurrent.ask", native_ask);
            r.register("aura.concurrent.newChannel", native_new_channel);
            r.register("aura.concurrent.channelSend", native_channel_send);
            r.register("aura.concurrent.channelRecv", native_channel_recv);
            r.register("aura.concurrent.channelTryRecv", native_channel_try_recv);
            r.register("aura.concurrent.select", native_select);
            r.register("aura.concurrent.spawnActor", native_spawn_actor);
            r.register("aura.concurrent.supervise", native_supervise);
            r.register("aura.concurrent.actorAlive", native_actor_alive);
        }

        r
    }

    /// 按需创建原生函数注册表（只注册 prelu + 指定模块）
    ///
    /// `modules` 是模块名集合，如 `["math", "io"]`。
    /// 未指定的模块不注册，对应代码不编译进二进制。
    /// 预lu（17 个全局内置）始终注册。
    pub fn with_modules(modules: &[&str]) -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };

        // 注册 prelude（17 个全局内置，始终存在）
        r.register("println", native_println);
        r.register("print", native_print);
        r.register("puts", native_puts);
        r.register("abs", native_abs);
        r.register("sqrt", native_sqrt);
        r.register("pow", native_pow);
        r.register("toInt", native_to_int);
        r.register("toFloat", native_to_float);
        r.register("toStr", native_to_str);
        r.register("toString", native_to_str); // alias for toStr, used as method call
        r.register("clock", native_clock);
        r.register("strlen", native_strlen);
        r.register("CString", native_cstring);
        r.register("CStr", native_cstr);
        r.register("ptrIsNull", native_ptr_is_null);
        r.register("ptrToInt", native_ptr_to_int);
        r.register("intToPtr", native_int_to_ptr);
        r.register("makeCallback", native_make_callback);

        // 按需注册 std 模块
        crate::std::register_with_modules(&mut r, modules);

        // P10: 并发运行时（需 std-concurrent feature 且导入 aura.concurrent）
        #[cfg(feature = "std-concurrent")]
        if modules.iter().any(|m| *m == "concurrent") {
            r.register("aura.concurrent.spawn", native_spawn);
            r.register("aura.concurrent.send", native_send);
            r.register("aura.concurrent.ask", native_ask);
            r.register("aura.concurrent.newChannel", native_new_channel);
            r.register("aura.concurrent.channelSend", native_channel_send);
            r.register("aura.concurrent.channelRecv", native_channel_recv);
            r.register("aura.concurrent.channelTryRecv", native_channel_try_recv);
            r.register("aura.concurrent.select", native_select);
            r.register("aura.concurrent.spawnActor", native_spawn_actor);
            r.register("aura.concurrent.supervise", native_supervise);
            r.register("aura.concurrent.actorAlive", native_actor_alive);
        }

        r
    }

    pub fn register(&mut self, name: &str, f: NativeFn) {
        self.fns.insert(name.to_string(), f);
    }

    /// 返回已注册的原生函数数量
    pub fn len(&self) -> usize {
        self.fns.len()
    }

    /// 查找原生函数（优先内置表，其次动态加载表）
    pub fn get(&self, name: &str) -> Option<NativeFn> {
        self.fns
            .get(name)
            .copied()
            .or_else(|| self.dynamic.get(name))
    }

    /// 检查是否注册了指定的原生函数（内置或动态）
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

// ─────────────────────────────────────────────────────────────────────────────
// P10 并发运行时 — 原生函数（Actor / Channel / Select / Spawn）
// ─────────────────────────────────────────────────────────────────────────────
//
// 原生函数通过 thread-local 指针访问 VM 实例的 Actor/Channel 运行时状态。
// VM 在 `run()` 开始时设置此指针，结束时清除。

use std::sync::atomic::{AtomicPtr, Ordering};

thread_local! {
    /// VM 实例指针（供原生函数访问 Actor/Channel 运行时）
    static VM_REF: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());
}

/// 设置当前 VM 实例（在 `Vm::run()` 开始时调用）
pub fn set_vm_ref(vm: *mut ()) {
    VM_REF.with(|r| r.store(vm, Ordering::SeqCst));
}

/// 清除当前 VM 实例引用（在 `Vm::run()` 结束时调用）
pub fn clear_vm_ref() {
    VM_REF.with(|r| r.store(std::ptr::null_mut(), Ordering::SeqCst));
}

/// 获取当前 VM 实例指针
fn get_vm_ref() -> Option<*mut crate::vm::Vm> {
    VM_REF.with(|r| {
        let p = r.load(Ordering::SeqCst);
        if p.is_null() { None } else { Some(p as *mut crate::vm::Vm) }
    })
}

/// spawn(expr) → Int：创建新协程（P10.1）
///
/// 将表达式作为协程入口，创建新协程并返回协程 ID。
/// spawn(...) → Int：启动协程（P10）
#[cfg(feature = "std-concurrent")]
fn native_spawn(args: &[Value]) -> Value {
    // spawn 的实际创建由 VM 协程调度器处理
    // 此处返回占位 ID（0 = 主线程）
    match args.first() {
        Some(v) => Value::Int(v.as_int()),
        None => Value::Int(0),
    }
}

/// send(actorId, msg) → Unit：向 Actor 发送消息（P10.6）
#[cfg(feature = "std-concurrent")]
fn native_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.send(actor_id, msg);
            }
        }
    }
    Value::Null
}

/// ask(actorId, msg) → Any：向 Actor 请求响应（P10.6）
#[cfg(feature = "std-concurrent")]
fn native_ask(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).actors.ask(actor_id, msg);
            }
        }
    }
    Value::Null
}

/// newChannel(bound) → Int：创建 Channel（P10.8）
///
/// `bound`: 容量上限，0 表示无界
#[cfg(feature = "std-concurrent")]
fn native_new_channel(args: &[Value]) -> Value {
    let bound = args.first().map(|v| v.as_int() as usize).unwrap_or(0);
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let id = (*vm_ptr).channels.new_channel(bound);
            return Value::Int(id as i64);
        }
    }
    Value::Int(0)
}

/// channelSend(ch, val) → Unit：向 Channel 发送值（P10.8）
#[cfg(feature = "std-concurrent")]
fn native_channel_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let ch_id = args[0].as_int() as usize;
        let val = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).channels.send(ch_id, val);
            }
        }
    }
    Value::Null
}

/// channelRecv(ch) → Any：从 Channel 接收值（阻塞语义，P10.8）
#[cfg(feature = "std-concurrent")]
fn native_channel_recv(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let ch_id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).channels.recv(ch_id);
            }
        }
    }
    Value::Null
}

/// channelTryRecv(ch) → Any：尝试从 Channel 接收值（非阻塞，P10.8）
#[cfg(feature = "std-concurrent")]
fn native_channel_try_recv(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let ch_id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).channels.try_recv(ch_id);
            }
        }
    }
    Value::Null
}

/// __select(ch1, ch2) → Any：select 多路复用（P10.9）
///
/// 检查所有通道，返回第一个有值的通道的值。
/// 若所有通道均为空，返回 `Null`。
#[cfg(feature = "std-concurrent")]
fn native_select(args: &[Value]) -> Value {
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let vm = &mut *vm_ptr;
            // 遍历所有通道，找到第一个有值的
            for arg in args {
                let ch_id = arg.as_int() as usize;
                if ch_id == 0 {
                    continue;
                }
                if !vm.channels.is_empty(ch_id) {
                    return vm.channels.recv(ch_id);
                }
            }
        }
    }
    Value::Null
}

/// __spawnActor(name) → Int：创建 Actor 实例（P10.4）
#[cfg(feature = "std-concurrent")]
fn native_spawn_actor(args: &[Value]) -> Value {
    let name = args.first().map(|v| v.to_string()).unwrap_or_else(|| "unnamed".to_string());
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let id = (*vm_ptr).actors.spawn(&name);
            return Value::Int(id as i64);
        }
    }
    Value::Int(0)
}

/// __supervise(parent, child) → Unit：建立监督关系（P10.7）
#[cfg(feature = "std-concurrent")]
fn native_supervise(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let parent_id = args[0].as_int() as usize;
        let child_id = args[1].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.supervise(parent_id, child_id);
            }
        }
    }
    Value::Null
}

/// __actorAlive(id) → Boolean：检查 Actor 是否存活（P10.7）
#[cfg(feature = "std-concurrent")]
fn native_actor_alive(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return Value::Bool((*vm_ptr).actors.is_alive(id));
            }
        }
    }
    Value::Bool(false)
}
