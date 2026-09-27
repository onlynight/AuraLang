//! std.sb — 可变字符串缓冲区（StringBuilder）
//!
//! 与 AOT C 运行库 `aura_lang_std_StringBuilder_*`（见 `cffi/aura_std_cffi.c`）
//! 语义一致：VM 侧用全局句柄注册表（仿 `std_net.rs::SocketRegistry`）保存
//! 活跃缓冲区，句柄为 `Value::Int`。
//!
//! 用途：在无 GC 的 AOT 运行时里，用「可增长缓冲区 + 摊还 O(1) 追加 + finish
//! 零拷贝」替代不可变 String 的反复拼接，避免海量中间串驻留内存。
//!
//! API（与 Aura 声明 `aura/core/aura/lang/std/StringBuilder.aura` 对齐）：
//! - `create() -> Long`
//! - `append(handle, text) -> Long`
//! - `appendChar(handle, ch) -> Long`
//! - `appendInt(handle, value) -> Long`
//! - `length(handle) -> Int`
//! - `finish(handle) -> String`（转移内容，句柄失效）
//! - `reset(handle) -> Long`

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 全局缓冲区注册表（句柄 → 内容）。
struct SbRegistry {
    items: HashMap<i64, String>,
    next_id: i64,
}

impl SbRegistry {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
            next_id: 1,
        }
    }

    fn create(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.insert(id, String::new());
        id
    }

    fn get_mut(&mut self, id: i64) -> Option<&mut String> {
        self.items.get_mut(&id)
    }
}

/// 获取全局缓冲区注册表。
pub fn get_sb_registry() -> &'static Mutex<SbRegistry> {
    static REGISTRY: OnceLock<Mutex<SbRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(SbRegistry::new()))
}

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.StringBuilder.create", nat_create);
    reg.register("aura.lang.std.StringBuilder.append", nat_append);
    reg.register("aura.lang.std.StringBuilder.appendChar", nat_append_char);
    reg.register("aura.lang.std.StringBuilder.appendInt", nat_append_int);
    reg.register("aura.lang.std.StringBuilder.length", nat_length);
    reg.register("aura.lang.std.StringBuilder.finish", nat_finish);
    reg.register("aura.lang.std.StringBuilder.reset", nat_reset);
}

fn handle_of(args: &[Value]) -> i64 {
    args.first().map(|v| v.as_int()).unwrap_or(0)
}

fn nat_create(_args: &[Value]) -> Value {
    let id = get_sb_registry().lock().unwrap().create();
    Value::Int(id)
}

fn nat_append(args: &[Value]) -> Value {
    let handle = handle_of(args);
    let text = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    if let Some(buf) = get_sb_registry().lock().unwrap().get_mut(handle) {
        buf.push_str(&text);
    }
    Value::Int(handle)
}

fn nat_append_char(args: &[Value]) -> Value {
    let handle = handle_of(args);
    let code = args.get(1).map(|v| v.as_int()).unwrap_or(0);
    if let Some(buf) = get_sb_registry().lock().unwrap().get_mut(handle) {
        if let Some(c) = char::from_u32(code as u32) {
            buf.push(c);
        }
    }
    Value::Int(handle)
}

fn nat_append_int(args: &[Value]) -> Value {
    let handle = handle_of(args);
    let value = args.get(1).map(|v| v.as_int()).unwrap_or(0);
    if let Some(buf) = get_sb_registry().lock().unwrap().get_mut(handle) {
        buf.push_str(&value.to_string());
    }
    Value::Int(handle)
}

fn nat_length(args: &[Value]) -> Value {
    let handle = handle_of(args);
    let len = get_sb_registry().lock().unwrap().get_mut(handle).map(|s| s.len()).unwrap_or(0);
    Value::Int(len as i64)
}

fn nat_finish(args: &[Value]) -> Value {
    let handle = handle_of(args);
    // 交出内容后**保留句柄**（置为空缓冲），以便 `reset`/`append` 复用同一句柄。
    // 与 AOT 侧语义对齐：AOT 的 finish 把 buf 置 NULL（同样保留结构体），
    // 后续 reset 会重新分配缓冲区。
    let mut reg = get_sb_registry().lock().unwrap();
    match reg.get_mut(handle) {
        Some(s) => Value::Str(std::rc::Rc::from(std::mem::take(s))),
        None => Value::str_(""),
    }
}

fn nat_reset(args: &[Value]) -> Value {
    let handle = handle_of(args);
    if let Some(buf) = get_sb_registry().lock().unwrap().get_mut(handle) {
        buf.clear();
    }
    Value::Int(handle)
}
