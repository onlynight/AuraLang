//! std.builtin — 编译期内置函数
//!
//! 提供 `typeof`、`typeOf`、`isNull`、`isNotNull`、`assert` 等基础内省函数。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("builtin.typeof", nat_typeof);
    reg.register("builtin.typeOf", nat_typeof);
    reg.register("builtin.isNull", nat_is_null);
    reg.register("builtin.isNotNull", nat_is_not_null);
    reg.register("builtin.isZero", nat_is_zero);
    reg.register("builtin.isPositive", nat_is_positive);
    reg.register("builtin.isNegative", nat_is_negative);
    reg.register("builtin.toString", nat_to_string);
    reg.register("builtin.toInt", nat_to_int);
    reg.register("builtin.toFloat", nat_to_float);
    reg.register("builtin.toBool", nat_to_bool);
    reg.register("builtin.sizeOf", nat_size_of);
    reg.register("builtin.hash", nat_hash);
    reg.register("builtin.compare", nat_compare);
    reg.register("builtin.clone", nat_clone);
    reg.register("builtin.identity", nat_identity);
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
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).map(|c| c as i32).unwrap_or(0),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).map(|c| c as i32).unwrap_or(0),
        (Value::Str(x), Value::Str(y)) => x.cmp(y) as i32,
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y) as i32,
        _ => 0,
    }
}
