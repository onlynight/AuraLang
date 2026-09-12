//! std.builtin — 编译期内置函数
//!
//! 提供 `typeof`、`typeOf`、`isNull`、`isNotNull`、`assert` 等基础内省函数。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

/// 注册 prelude 内置函数（**裸名** + `aura.lang.std.*` / `aura.lang.std.Builtin.*` 别名）。
///
/// 背景：编译器把 `isNull(x)` 这类 prelude 调用解析为**裸名**原生调用
/// （见 `codegen::hir::resolve_builtin_method` → `is_prelude`），若 VM 只注册了
/// `aura.lang.std.Builtin.isNull`，该调用会落入「未链接的外部函数」分支被静默忽略，
/// 导致 `isNull` 恒为假等错误行为。此处补齐裸名注册。
pub fn register_prelude(reg: &mut NativeRegistry) {
    let table: [(&str, fn(&[Value]) -> Value); 12] = [
        ("typeof", nat_typeof),
        ("isNull", nat_is_null),
        ("isNotNull", nat_is_not_null),
        ("isZero", nat_is_zero),
        ("isPositive", nat_is_positive),
        ("isNegative", nat_is_negative),
        ("toBool", nat_to_bool),
        ("sizeOf", nat_size_of),
        ("hash", nat_hash),
        ("compare", nat_compare),
        ("clone", nat_clone),
        ("identity", nat_identity),
    ];
    for (name, f) in table {
        reg.register(name, f);
        reg.register(&format!("aura.lang.std.{name}"), f);
        reg.register(&format!("aura.lang.std.Builtin.{name}"), f);
    }

    // `throw expr` 由 HIR 降级为 `__throw(expr)` 调用。
    // 此前未注册 → 落入「未链接的外部函数」分支被**静默忽略**（错误被吞掉）。
    // 注意：VM 目前尚无异常传播（try/catch 的 handler 栈），因此这里不改变控制流，
    // 仅在 stderr 输出诊断，避免把错误藏起来。完整异常传播见 README 待办。
    reg.register("__throw", nat_throw);

    // `Process.exit` / `Process.exitProcess` 的短名别名。
    // 单例对象方法的运行时名解析为 `Process.exit` 形式，而注册表只有
    // `aura.lang.std.Process.exit`；补别名可让 Aura 代码无需 import 即可退出。
    for name in [
        "Process.exit",
        "Process.exitProcess",
    ] {
        reg.register(name, crate::std::std_process::nat_exit_pub);
    }
}

/// `__throw(value)` — `throw` 表达式的降级目标。
///
/// **解释器路径不会走到这里**：`Vm::do_call_native(_args)` 在原生派发前拦截 `__throw`
/// 并调用 `Vm::raise()` 做栈展开（`try/catch`）。此处仅为 JIT / AOT 直接调用原生函数
/// 的场景保留一个安全的兜底实现：不改变控制流，仅输出诊断，避免异常被彻底吞掉。
fn nat_throw(args: &[Value]) -> Value {
    let v = args.first().cloned().unwrap_or(Value::Null);
    eprintln!(
        "[vm] throw: {} (exception propagation not implemented, call ignored)",
        v
    );
    Value::Null
}

pub fn register(reg: &mut NativeRegistry) {
    // ── 类型查询（Layer 1，不能上移）──
    reg.register("aura.lang.std.Builtin.typeof", nat_typeof);
    reg.register("aura.lang.std.Builtin.typeOf", nat_typeof);

    // ── 空值检查（Layer 1，不能上移）──
    reg.register("aura.lang.std.Builtin.isNull", nat_is_null);
    reg.register("aura.lang.std.Builtin.isNotNull", nat_is_not_null);

    // ── 数值检查（Layer 1，不能上移）──
    reg.register("aura.lang.std.Builtin.isZero", nat_is_zero);
    reg.register("aura.lang.std.Builtin.isPositive", nat_is_positive);
    reg.register("aura.lang.std.Builtin.isNegative", nat_is_negative);

    // ── 类型转换（Layer 1 核心虚方法，不能上移）──
    // toString 是 Any 核心虚方法，保留在 Rust
    reg.register("aura.lang.std.Builtin.toString", nat_to_string);

    // ── 类型转换（Layer 1 扩展方法，已上移到 Aura，但 VM 仍需 native 实现）──
    reg.register("aura.lang.std.Builtin.toInt", nat_to_int);
    reg.register("aura.lang.std.Builtin.toFloat", nat_to_float);
    reg.register("aura.lang.std.Builtin.toBool", nat_to_bool);

    // ── 大小与哈希（Layer 1，不能上移）──
    reg.register("aura.lang.std.Builtin.sizeOf", nat_size_of);
    reg.register("aura.lang.std.Builtin.hash", nat_hash);

    // ── 比较与克隆（Layer 1，不能上移）──
    reg.register("aura.lang.std.Builtin.compare", nat_compare);
    reg.register("aura.lang.std.Builtin.clone", nat_clone);
    reg.register("aura.lang.std.Builtin.identity", nat_identity);
}

/// typeof(value) → String (type name)
fn nat_typeof(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::str_(v.type_name().to_string()),
        None => Value::str_("Null"),
    }
}

/// isNull(value) → Bool
fn nat_is_null(args: &[Value]) -> Value {
    Value::Bool(args.first().map(|v| v == &Value::Null).unwrap_or(true))
}

/// isNotNull(value) → Bool
fn nat_is_not_null(args: &[Value]) -> Value {
    Value::Bool(args.first().map(|v| v != &Value::Null).unwrap_or(false))
}

/// isZero(value) → Bool
fn nat_is_zero(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Bool(*i == 0),
        Some(Value::Float(f)) => Value::Bool(*f == 0.0),
        _ => Value::Bool(false),
    }
}

/// isPositive(value) → Bool
fn nat_is_positive(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Bool(*i > 0),
        Some(Value::Float(f)) => Value::Bool(*f > 0.0),
        _ => Value::Bool(false),
    }
}

/// isNegative(value) → Bool
fn nat_is_negative(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Bool(*i < 0),
        Some(Value::Float(f)) => Value::Bool(*f < 0.0),
        _ => Value::Bool(false),
    }
}

/// toString(value) → String
fn nat_to_string(args: &[Value]) -> Value {
    Value::str_(args.first().map(|v| v.to_string()).unwrap_or_default())
}

/// toInt(value) → Int
fn nat_to_int(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Int(v.as_int()),
        None => Value::Int(0),
    }
}

/// toFloat(value) → Float
fn nat_to_float(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Float(v.as_float()),
        None => Value::Float(0.0),
    }
}

/// toBool(value) → Bool
fn nat_to_bool(args: &[Value]) -> Value {
    Value::Bool(args.first().map(|v| v.is_truthy()).unwrap_or(false))
}

/// sizeOf(value) → Int (size in bytes approximation)
fn nat_size_of(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(_)) => Value::Int(8),
        Some(Value::Float(_)) => Value::Int(8),
        Some(Value::Bool(_)) => Value::Int(1),
        Some(Value::Str(s)) => Value::Int(s.len() as i64),
        Some(Value::Null) => Value::Int(0),
        _ => Value::Int(8),
    }
}

/// hash(value) → Int (hash code)
fn nat_hash(args: &[Value]) -> Value {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    args.first().map(|v| v.hash(&mut hasher)).unwrap_or(());
    Value::Int(hasher.finish() as i64)
}

/// compare(a, b) → Int (-1, 0, 1)
fn nat_compare(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Int(0);
    }
    let cmp = compare_values(&args[0], &args[1]);
    Value::Int(cmp as i64)
}

/// clone(value) → Value (shallow clone)
fn nat_clone(args: &[Value]) -> Value {
    args.first().cloned().unwrap_or(Value::Null)
}

/// identity(value) → Value (return as-is)
fn nat_identity(args: &[Value]) -> Value {
    args.first().cloned().unwrap_or(Value::Null)
}

fn compare_values(a: &Value, b: &Value) -> i32 {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y) as i32,
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).map(|c| c as i32).unwrap_or(0),
        (Value::Int(x), Value::Float(y)) => {
            (*x as f64).partial_cmp(y).map(|c| c as i32).unwrap_or(0)
        }
        (Value::Float(x), Value::Int(y)) => {
            x.partial_cmp(&(*y as f64)).map(|c| c as i32).unwrap_or(0)
        }
        (Value::Str(x), Value::Str(y)) => x.cmp(y) as i32,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y) as i32,
        _ => 0,
    }
}
