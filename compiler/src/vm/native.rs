//! 原生（内置 / FFI）函数调度器
//!
//! 对应 技术方案 §7.1 的 `CallNative` / `CallC` 与 §9.3 的 FFI 调度。
//!
//! 原生函数签名统一为 `fn(&[Value]) -> Value`：参数已从操作数栈按声明顺序弹出。
//! 返回值压回操作数栈。`println` 等内置函数由 VM 启动时自动注册；`extern "c"`
//! 声明的函数（如 `puts`）若未在运行时链接，则回退为打印其参数的占位实现，
//! 保证字节码可继续执行而不崩溃。

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::codegen::opcode::FfiAbi;
use crate::vm::dynamic_ffi::DynamicLoader;
use crate::vm::value::Value;

/// 原生函数指针类型
pub type NativeFn = fn(&[Value]) -> Value;

/// 原生函数注册表
#[derive(Default)]
pub struct NativeRegistry {
    fns: HashMap<String, NativeFn>,
    /// 动态加载器（P8.9）：未内置的原生函数从动态库查找
    dynamic: DynamicLoader,
}

impl NativeRegistry {
    /// 创建并注册全部内置原生函数（向后兼容）
    pub fn new() -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };

        // 全量注册所有已启用的 std 模块（std-math / std-string / std-collections …）。
        //
        // 关键：本函数承担「全量注册」职责——`Vm::new` 在 `enabled_modules` 为空
        // （即源码**没有 import**）时走这条路。此前这里只注册了下面那份硬编码的
        // prelude 子集，并未调用 `std::register_all()`，于是「无 import」的程序
        // 反而拿不到 `aura.lang.std.String.length`、`aura.lang.std.Math.sin`
        // 等模块原生函数，调用落入 `do_call_native` 的「未链接的外部函数」兜底分支，
        // 被**静默忽略并返回 Int(0)**（表现为 `"abc".length() == 0` 这类错值）。
        //
        // 放在硬编码 prelude **之前**注册，使下面的核心/基础条目在重名时仍保持优先，
        // 本调用只补齐缺失的模块函数。
        crate::std::register_all(&mut r);

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
        r.register("aura_isOfType", native_is_of_type);
        // Phase 4: Any 基类内置方法
        r.register("equals", native_equals);
        r.register("hashCode", native_hash_code);
        r.register("typeOf", native_type_of);
        r.register("aura_cast", native_cast);
        r.register("aura_cast_safety", native_cast_safety);
        r.register("__size", native_size);
        r.register("__get", native_get);
        // prelude 裸名（isNull / listOf / min / assertEq ...）
        crate::std::register_prelude(&mut r);
        // Full-name aliases for prelude (aura.lang.std.<fn>)
        r.register("aura.lang.std.println", native_println);
        r.register("aura.lang.std.print", native_print);
        r.register("aura.lang.std.puts", native_puts);
        r.register("aura.lang.std.abs", native_abs);
        r.register("aura.lang.std.sqrt", native_sqrt);
        r.register("aura.lang.std.pow", native_pow);
        r.register("aura.lang.std.toInt", native_to_int);
        r.register("aura.lang.std.toFloat", native_to_float);
        r.register("aura.lang.std.toStr", native_to_str);
        r.register("aura.lang.std.toString", native_to_str);
        r.register("aura.lang.std.clock", native_clock);
        r.register("aura.lang.std.strlen", native_strlen);
        r.register("aura.lang.std.CString", native_cstring);
        r.register("aura.lang.std.CStr", native_cstr);
        r.register("aura.lang.std.ptrIsNull", native_ptr_is_null);
        r.register("aura.lang.std.ptrToInt", native_ptr_to_int);
        r.register("aura.lang.std.intToPtr", native_int_to_ptr);
        r.register("aura.lang.std.makeCallback", native_make_callback);
        r.register("aura.lang.std.aura_isOfType", native_is_of_type);
        r.register("aura.lang.std.equals", native_equals);
        r.register("aura.lang.std.hashCode", native_hash_code);
        r.register("aura.lang.std.typeOf", native_type_of);
        r.register("aura.lang.std.aura_cast", native_cast);
        r.register("aura.lang.std.aura_cast_safety", native_cast_safety);
        // Fix 9: 默认仅加载 prelude，不加载全部 std 模块
        // 如需加载 std 模块，使用 NativeRegistry::with_modules()

        // P10: 并发运行时（需 std-concurrent feature）——与 `with_modules` 共用同一实现
        #[cfg(feature = "std-concurrent")]
        Self::register_concurrent(&mut r);
        Self::register_ffi_aliases(&mut r);

        r
    }

    /// 按需创建原生函数注册表（只注册 prelu + 指定模块）
    ///
    /// `modules` 是模块名集合，如 `["math", "io"]`。
    /// 未指定的模块不注册，对应代码不编译进二进制。
    /// prelu（17 个全局内置）始终注册。
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
        r.register("aura_isOfType", native_is_of_type);
        // Phase 4: Any 基类内置方法
        r.register("equals", native_equals);
        r.register("hashCode", native_hash_code);
        r.register("typeOf", native_type_of);
        r.register("aura_cast", native_cast);
        r.register("aura_cast_safety", native_cast_safety);

        // prelude 裸名（isNull / listOf / min / assertEq ...）
        crate::std::register_prelude(&mut r);

        // Full-name aliases for prelude (aura.lang.std.<fn>)
        r.register("aura.lang.std.println", native_println);
        r.register("aura.lang.std.print", native_print);
        r.register("aura.lang.std.puts", native_puts);
        r.register("aura.lang.std.abs", native_abs);
        r.register("aura.lang.std.sqrt", native_sqrt);
        r.register("aura.lang.std.pow", native_pow);
        r.register("aura.lang.std.toInt", native_to_int);
        r.register("aura.lang.std.toFloat", native_to_float);
        r.register("aura.lang.std.toStr", native_to_str);
        r.register("aura.lang.std.toString", native_to_str);
        r.register("aura.lang.std.clock", native_clock);
        r.register("aura.lang.std.strlen", native_strlen);
        r.register("aura.lang.std.CString", native_cstring);
        r.register("aura.lang.std.CStr", native_cstr);
        r.register("aura.lang.std.ptrIsNull", native_ptr_is_null);
        r.register("aura.lang.std.ptrToInt", native_ptr_to_int);
        r.register("aura.lang.std.intToPtr", native_int_to_ptr);
        r.register("aura.lang.std.makeCallback", native_make_callback);
        r.register("aura.lang.std.aura_isOfType", native_is_of_type);
        r.register("aura.lang.std.equals", native_equals);
        r.register("aura.lang.std.hashCode", native_hash_code);
        r.register("aura.lang.std.typeOf", native_type_of);
        r.register("aura.lang.std.aura_cast", native_cast);
        r.register("aura.lang.std.aura_cast_safety", native_cast_safety);

        // 按需注册 std 模块
        crate::std::register_with_modules(&mut r, modules);

        // P10: concurrent runtime（按需：仅当 import 了并发模块时注册）
        #[cfg(feature = "std-concurrent")]
        if modules.iter().any(|m| *m == "concurrent") {
            Self::register_concurrent(&mut r);
        }
        // FFI 别名（aura.ffi.*）属于基础能力，始终可用
        Self::register_ffi_aliases(&mut r);

        r
    }

    /// 注册并发运行时原生函数（Coroutine / Actor / Channel）。
    ///
    /// 抽为共享函数，供「全量注册」([`Self::new`]) 与「按需注册」([`Self::with_modules`])
    /// 两条路径复用——此前只有按需路径注册它们，导致 `NativeRegistry::new()` 缺少
    /// `aura.lang.std.Coroutine.spawnActor` 等条目（docgen 的注册表一致性检查因此失败）。
    #[cfg(feature = "std-concurrent")]
    fn register_concurrent(r: &mut NativeRegistry) {
        r.register("aura.lang.std.Coroutine.spawn", native_spawn);
        r.register("aura.lang.std.Actor.send", native_send);
        r.register("aura.lang.std.Coroutine.ask", native_ask);
        r.register("aura.lang.std.Actor.reply", native_reply);
        r.register("aura.lang.std.Channel.newChannel", native_new_channel);
        r.register("aura.lang.std.Channel.channelSend", native_channel_send);
        r.register("aura.lang.std.Channel.channelRecv", native_channel_recv);
        r.register(
            "aura.lang.std.Channel.channelTryRecv",
            native_channel_try_recv,
        );
        r.register("aura.lang.std.Channel.select", native_select);
        r.register("aura.lang.std.Channel.selectTimeout", native_select_timeout);
        r.register("aura.lang.std.Actor.spawnActor", native_spawn_actor);
        // HIR 的并发路径解析也会产出 `Coroutine.spawnActor`（见 codegen::hir
        // 的 `resolve_function_path`），补别名避免同一函数两种名字解析不到。
        r.register("aura.lang.std.Coroutine.spawnActor", native_spawn_actor);
        r.register("aura.lang.std.Actor.supervise", native_supervise);
        r.register("aura.lang.std.Actor.actorAlive", native_actor_alive);

        // Phase 3: 跨进程 Actor / Channel
        r.register(
            "aura.lang.std.Actor.spawnActorProcess",
            native_spawn_actor_process,
        );
        r.register(
            "aura.lang.std.Actor.sendProcessActor",
            native_send_process_actor,
        );
        r.register(
            "aura.lang.std.Actor.recvProcessActor",
            native_recv_process_actor,
        );
        r.register(
            "aura.lang.std.Actor.processActorAlive",
            native_process_actor_alive,
        );
        r.register(
            "aura.lang.std.Actor.killProcessActor",
            native_kill_process_actor,
        );
        r.register(
            "aura.lang.std.Channel.newTcpChannel",
            native_new_tcp_channel,
        );
        r.register(
            "aura.lang.std.Channel.tcpChannelSend",
            native_tcp_channel_send,
        );
    }

    /// 注册 `aura.ffi.*` 别名（与 `aura.lang.std.*` 同源）。
    ///
    /// FFI 相关内置函数在文档与部分 import 形式下以 `aura.ffi.` 前缀出现，
    /// 此前只在 `aura.lang.std.*` / 裸名下注册，docgen 的一致性检查会报缺失。
    fn register_ffi_aliases(r: &mut NativeRegistry) {
        r.register("aura.ffi.CString", native_cstring);
        r.register("aura.ffi.readCStr", native_cstr);
        r.register("aura.ffi.ptrIsNull", native_ptr_is_null);
        r.register("aura.ffi.ptrToInt", native_ptr_to_int);
        r.register("aura.ffi.intToPtr", native_int_to_ptr);
        r.register("aura.ffi.makeCallback", native_make_callback);
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
        self.fns.get(name).copied().or_else(|| self.dynamic.get(name))
    }

    /// 检查是否注册了指定的原生函数（内置或动态）
    pub fn contains(&self, name: &str) -> bool {
        self.fns.contains_key(name) || self.dynamic.contains(name)
    }

    /// 动态加载库（P8.9）。
    pub fn load_library(&mut self, path: &str, abi: FfiAbi) -> Result<(), String> {
        self.dynamic.load_lib(path, abi)
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
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
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

/// aura.isOfType(value, typeName) → Boolean：检查值的运行时类型
fn native_is_of_type(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let value_type = args[0].type_name();
        let target_type = match &args[1] {
            Value::Str(s) => &**s,
            _ => "",
        };
        Value::Bool(value_type == target_type)
    } else {
        Value::Bool(false)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 4: Any 基类内置方法（toString / equals / hashCode / typeOf）
// ─────────────────────────────────────────────────────────────────────────────

/// equals(this, other) → Bool：值相等检查
///
/// 对基本类型（Int/Float/Bool/Str）按值比较；
/// 对堆对象默认身份相等（===），用户可重写 Any.equals 虚方法。
fn native_equals(args: &[Value]) -> Value {
    if args.len() >= 2 { Value::Bool(args[0] == args[1]) } else { Value::Bool(false) }
}

/// hashCode(value) → Int：基于身份的哈希码
///
/// 对堆对象基于堆句柄计算；基本类型使用值的哈希。
/// 用户可重写 Any.hashCode 虚方法。
fn native_hash_code(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Int(*i % 2_147_483_647),
        Some(Value::Float(f)) => Value::Int(f.to_bits() as i64 % 2_147_483_647),
        Some(Value::Bool(b)) => Value::Int(*b as i64),
        Some(Value::Str(s)) => {
            let h = s.bytes().fold(2166136261u32, |acc, b| {
                (acc ^ b as u32).wrapping_mul(16777619)
            });
            Value::Int((h % 2_147_483_647) as i64)
        }
        Some(Value::Ref(handle)) => Value::Int((*handle as i64) % 2_147_483_647),
        _ => Value::Int(0),
    }
}

/// typeOf(value) → String：返回运行时类型名（反射）
///
/// 对堆对象返回类名（通过 VM 类定义表）；对基本类型返回类型标签。
/// 替代原 type_name() 的静态字符串返回，支持类层级。
fn native_type_of(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::str_(v.type_name()),
        None => Value::str_("Any"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 4: as 类型转换（strict cast + safe cast）
// ─────────────────────────────────────────────────────────────────────────────

/// aura_cast(value, targetTypeName) → Value
///
/// 严格类型转换：
/// - 对类类型：使用 is_instance_of 检查，不匹配则返回 Null（VM 层拦截并抛异常）
/// - 对基本类型：运行时类型转换（Int/Float/Bool/Str/Null）
///
/// 此函数在 interp.rs 中被拦截，使用类层级检查。
/// 此处的 fallback 实现处理基本类型转换。
pub fn native_cast(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let value = args[0].clone();
    let target_type = match &args[1] {
        Value::Str(s) => s.as_ref().to_string(),
        _ => return Value::Null,
    };

    // 基本类型转换
    match target_type.as_str() {
        "Int" | "Long" => Value::Int(value.as_int()),
        "Float" | "Double" | "Number" => Value::Float(value.as_float()),
        "Boolean" | "Bool" => Value::Bool(value.as_bool()),
        "String" => Value::str_(value.as_string()),
        "Null" => {
            if value.is_null_ptr() || value.type_name() == "Null" {
                Value::Null
            } else {
                Value::Null // 非 Null 值无法转为 Null，返回 Null
            }
        }
        _ => {
            // 类类型：fallback 为返回原值（VM 层会拦截并做 CheckCast）
            value
        }
    }
}

/// aura_cast_safety(value, targetTypeName) → Value
///
/// 安全类型转换（as?）：
/// - 对类类型：使用 is_instance_of 检查，不匹配则返回 Null
/// - 对基本类型：同 aura_cast
///
/// 此函数在 interp.rs 中被拦截，使用类层级检查。
fn native_cast_safety(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    // 对于类类型，VM 层会拦截；此处仅处理基本类型
    native_cast(args)
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
/// 蹦床通过 thread-local 派发到 Aura VM 执行对应函数。
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

use std::cell::RefCell;

/// 当前线程的 VM 实例栈（栈式结构支持嵌套 VM 执行）
///
/// - `set_vm_ref` 压栈
/// - `clear_vm_ref` 弹栈
/// - `get_vm_ref` 返回栈顶（当前线程上活跃的 VM）
///
/// **必须是线程局部**：此前这里是进程全局的 `Mutex<Vec<usize>>`，当多个线程
/// 各自运行一个 VM 时（典型场景：`cargo test` 并发执行测试用例），后启动的 VM
/// 会被压到同一栈顶，于是 `get_vm_ref()` 返回**别的线程**的 VM 实例，导致
/// Actor / Channel 运行时状态串台 —— 表现为并发测试随机失败（且失败数随并发度浮动）。
/// 改为 `thread_local!` 后各线程互不干扰，嵌套执行语义仍由栈保证。
thread_local! {
    static VM_STACK: RefCell<Vec<usize>> = RefCell::new(Vec::new());
}

/// 设置当前 VM 实例（在 `Vm::run()` 开始时调用，压栈）
///
/// 使用 `try_with` 而非 `with`：线程局部存储销毁期间（进程退出路径）调用不会 panic。
pub fn set_vm_ref(vm: *mut ()) {
    let _ = VM_STACK.try_with(|s| s.borrow_mut().push(vm as usize));
}

/// 清除当前 VM 实例引用（在 `Vm::run()` 结束时调用，弹栈）
pub fn clear_vm_ref() {
    let _ = VM_STACK.try_with(|s| s.borrow_mut().pop());
}

/// 获取当前 VM 实例指针（当前线程栈顶）
fn get_vm_ref() -> Option<*mut crate::vm::Vm> {
    VM_STACK
        .try_with(|s| s.borrow().last().copied())
        .ok()
        .flatten()
        .map(|addr| addr as *mut crate::vm::Vm)
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

/// reply(requestId, response) → Unit：回复 Actor 请求（Phase 4）
#[cfg(feature = "std-concurrent")]
fn native_reply(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let request_id = args[0].as_int() as u64;
        let response = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.reply(request_id, response);
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

/// __select(ch1, ch2, ...) → Any：select 多路复用（P10.9）
///
/// 阶段 4.11: 基于协程挂起的非轮询实现
///
/// 检查所有通道，返回第一个有值的通道的值。
/// 若所有通道均为空，返回 `Null`（非阻塞模式）。
/// 返回值格式：`[channel_id, value]` 或 `Null`
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
                    let val = vm.channels.recv(ch_id);
                    // 返回 [channel_id, value]
                    return val;
                }
            }
        }
    }
    // 所有通道均为空，返回 Null（非阻塞）
    Value::Null
}

/// __selectTimeout(ch1, ch2, ..., timeoutMs) → Any：带超时的 select（阶段 4.11）
///
/// 在 `timeoutMs` 毫秒内等待通道消息，超时返回 `Null`。
#[cfg(feature = "std-concurrent")]
fn native_select_timeout(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }

    // 最后一个参数是超时时间（毫秒）
    let timeout_arg = args.last().unwrap().clone();
    let timeout_ms = timeout_arg.as_int();
    let channels = &args[..args.len() - 1];

    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let vm = &mut *vm_ptr;
            let start = std::time::Instant::now();
            let poll_interval = std::time::Duration::from_millis(1);

            loop {
                // 遍历所有通道，找到第一个有值的
                for arg in channels {
                    let ch_id = arg.as_int() as usize;
                    if ch_id == 0 {
                        continue;
                    }
                    if !vm.channels.is_empty(ch_id) {
                        let val = vm.channels.recv(ch_id);
                        // 返回 [channel_id, value]
                        return Value::List(vec![
                            Value::Int(ch_id as i64),
                            val,
                        ]);
                    }
                }

                // 检查超时
                if start.elapsed() >= std::time::Duration::from_millis(timeout_ms as u64) {
                    return Value::Null;
                }

                // 休眠后重试（让出 CPU）
                std::thread::sleep(poll_interval);
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

// ─────────────────────────────────────────────────────────────────────────────
// Phase 3: 跨进程 Actor / Channel 原生函数
// ─────────────────────────────────────────────────────────────────────────────

/// __spawnActorProcess(entry, name) → Map: 跨进程创建 Actor（Phase 3）
///
/// 在子进程中启动一个新的 Actor 实例，返回连接信息。
/// 当前为骨架实现：返回端口和名称。
#[cfg(feature = "std-concurrent")]
fn native_spawn_actor_process(args: &[Value]) -> Value {
    use std::collections::HashMap;
    let entry = args.first().map(|v| v.as_string()).unwrap_or_default();
    let name = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "unnamed".to_string());

    if entry.is_empty() {
        return Value::Null;
    }

    match crate::vm::actor_process::ProcessActor::spawn(&entry, &name) {
        Ok((actor, port)) => {
            // 将 actor 存入全局注册表
            let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
            let id = registry.len() + 1;
            registry.insert(id, actor);
            drop(registry);

            let mut map = HashMap::new();
            map.insert(Value::str_("id"), Value::Int(id as i64));
            map.insert(Value::str_("port"), Value::Int(port as i64));
            map.insert(Value::str_("name"), Value::str_(name));
            map.insert(Value::str_("alive"), Value::Bool(true));
            Value::Map(map)
        }
        Err(e) => {
            eprintln!("[Phase 3] 跨进程 Actor 启动失败: {}", e);
            Value::Null
        }
    }
}

/// __sendProcessActor(id, msg) → Unit: 向跨进程 Actor 发送消息（Phase 3）
#[cfg(feature = "std-concurrent")]
fn native_send_process_actor(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            if let Err(e) = actor.send(&msg) {
                eprintln!("[Phase 3] 跨进程发送失败: {}", e);
            }
        }
    }
    Value::Null
}

/// __recvProcessActor(id) → Any: 从跨进程 Actor 接收响应（Phase 3）
#[cfg(feature = "std-concurrent")]
fn native_recv_process_actor(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            match actor.recv() {
                Ok(Some(val)) => return val,
                Ok(None) => return Value::Null,
                Err(e) => {
                    eprintln!("[Phase 3] 跨进程接收失败: {}", e);
                    return Value::Null;
                }
            }
        }
    }
    Value::Null
}

/// __processActorAlive(id) → Boolean: 检查跨进程 Actor 是否存活（Phase 3）
#[cfg(feature = "std-concurrent")]
fn native_process_actor_alive(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            return Value::Bool(actor.is_alive());
        }
    }
    Value::Bool(false)
}

/// __killProcessActor(id) → Unit: 关闭跨进程 Actor（Phase 3）
#[cfg(feature = "std-concurrent")]
fn native_kill_process_actor(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap().remove(&actor_id);
    }
    Value::Null
}

/// __newTcpChannel(port) → Int: 创建跨进程 Channel（Phase 3）
///
/// `port=0` 时自动分配端口，返回端口号。
#[cfg(feature = "std-concurrent")]
fn native_new_tcp_channel(args: &[Value]) -> Value {
    let port = args.first().map(|v| v.as_int() as u16).unwrap_or(0);
    let result = if port == 0 {
        crate::vm::channel_tcp::TcpChannelServer::new_any()
    } else {
        crate::vm::channel_tcp::TcpChannelServer::new(port).map(|s| (s, port))
    };
    match result {
        Ok((server, actual_port)) => {
            let mut registry = crate::vm::channel_tcp::TCP_CHANNELS.lock().unwrap();
            let id = registry.len() + 1;
            registry.insert(id, server);
            drop(registry);
            Value::Int(id as i64)
        }
        Err(e) => {
            eprintln!("[Phase 3] TCP Channel 创建失败: {}", e);
            Value::Int(0)
        }
    }
}

/// __tcpChannelSend(id, val) → Unit: 向跨进程 Channel 发送（Phase 3）
#[cfg(feature = "std-concurrent")]
fn native_tcp_channel_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let ch_id = args[0].as_int() as usize;
        let val = args[1].clone();
        let mut registry = crate::vm::channel_tcp::TCP_CHANNELS.lock().unwrap();
        if let Some(server) = registry.get_mut(&ch_id) {
            // 注：服务端需要连接才能发送，当前简化为直接发送
            // 实际使用需通过客户端连接发送
            eprintln!("[Phase 3] TCP Channel 发送暂不支持（需客户端连接）");
        }
    }
    Value::Null
}

/// __size(iterable) -> Int: return collection length (for loop iterator support)
fn native_size(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Int(0);
    }
    match &args[0] {
        Value::List(items) => Value::Int(items.len() as i64),
        _ => Value::Int(0),
    }
}

/// __get(iterable, index) -> Value: return element at index
fn native_get(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let index = args[1].as_int() as usize;
    match &args[0] {
        Value::List(items) => {
            if index < items.len() {
                items[index].clone()
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}
