//! std.json — JSON 解析与序列化
//!
//! 使用 `serde_json` crate 实现 JSON 的解析与序列化。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use serde_json::Value as JsonValue;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.lang.std.Json.parse", nat_parse);
    reg.register("aura.lang.std.Json.stringify", nat_stringify);
    reg.register("aura.lang.std.Json.isValid", nat_is_valid);
    reg.register("aura.lang.std.Json.get", nat_get);
    reg.register("aura.lang.std.Json.set", nat_set);
    reg.register("aura.lang.std.Json.keys", nat_keys);
    reg.register("aura.lang.std.Json.values", nat_values);
    reg.register("aura.lang.std.Json.length", nat_length);
    reg.register("aura.lang.std.Json.contains", nat_contains);
    reg.register("aura.lang.std.Json.remove", nat_remove);
}

/// json.parse(text) → Value (JSON tree)
fn nat_parse(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    match serde_json::from_str::<JsonValue>(&text) {
        Ok(v) => json_to_value(&v),
        Err(e) => Value::str_(format!("JSON parse error: {}", e)),
    }
}

/// json.stringify(value, pretty) → String
fn nat_stringify(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::str_("null");
    }
    let json = value_to_json(&args[0]);
    let pretty = args.get(1).map(|v| v.as_bool()).unwrap_or(false);
    match serde_json::to_string_pretty(&json) {
        Ok(s) if pretty => Value::str_(s),
        Ok(s) => Value::str_(s),
        Err(e) => Value::str_(format!("JSON stringify error: {}", e)),
    }
}

/// json.isValid(text) → Bool
fn nat_is_valid(args: &[Value]) -> Value {
    let text = args.first().map(|v| v.as_string()).unwrap_or_default();
    Value::Bool(serde_json::from_str::<JsonValue>(&text).is_ok())
}

/// json.get(obj, key) → Value
fn nat_get(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let key = args[1].as_string();
    match &args[0] {
        Value::Map(map) => map.get(&Value::str_(key)).cloned().unwrap_or(Value::Null),
        Value::List(items) => {
            // 按索引访问
            let idx: i64 = key.parse().unwrap_or(-1);
            if idx >= 0 {
                items.get(idx as usize).cloned().unwrap_or(Value::Null)
            } else {
                Value::Null
            }
        }
        _ => Value::Null,
    }
}

/// json.set(obj, key, value) → Unit
fn nat_set(args: &[Value]) -> Value {
    if args.len() < 3 {
        return Value::Null;
    }
    let key = args[1].as_string();
    let val = args[2].clone();
    // Note: can't mutate map in-place due to &[Value] signature
    // Return the updated map for the caller to reassign
    if let Value::Map(ref map) = args[0] {
        let mut new_map = map.clone();
        new_map.insert(Value::str_(key), val);
        return Value::Map(new_map);
    } else {
        return Value::Null;
    }
}

/// json.keys(obj) → List
fn nat_keys(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => Value::List(map.keys().map(|k| k.clone()).collect()),
        _ => Value::List(Vec::new()),
    }
}

/// json.values(obj) → List
fn nat_values(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => Value::List(map.values().cloned().collect()),
        _ => Value::List(Vec::new()),
    }
}

/// json.length(obj) → Int
fn nat_length(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => Value::Int(map.len() as i64),
        Value::List(items) => Value::Int(items.len() as i64),
        _ => Value::Int(0),
    }
}

/// json.contains(obj, key) → Bool
fn nat_contains(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Bool(false);
    }
    let key = args[1].as_string();
    match &args[0] {
        Value::Map(map) => Value::Bool(map.contains_key(&Value::str_(key))),
        _ => Value::Bool(false),
    }
}

/// json.remove(obj, key) → Value or null
fn nat_remove(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Null;
    }
    let key = args[1].as_string();
    match &args[0] {
        Value::Map(map) => {
            let mut new_map = map.clone();
            new_map.remove(&Value::str_(key)).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

/// 将 JSON 值转换为 Aura Value
fn json_to_value(v: &JsonValue) -> Value {
    match v {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(b) => Value::Bool(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::Int(0)
            }
        }
        JsonValue::String(s) => Value::str_(s.clone()),
        JsonValue::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        JsonValue::Object(map) => {
            let mut result = std::collections::HashMap::new();
            for (k, v) in map {
                result.insert(Value::str_(k.clone()), json_to_value(v));
            }
            Value::Map(result)
        }
    }
}

/// 将 Aura Value 转换为 JSON 值
fn value_to_json(v: &Value) -> JsonValue {
    match v {
        Value::Null => JsonValue::Null,
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Int(i) => JsonValue::Number((*i).into()),
        Value::Float(f) => {
            let num =
                serde_json::Number::from_f64(*f).unwrap_or_else(|| serde_json::Number::from(0));
            JsonValue::Number(num)
        }
        Value::Str(s) => JsonValue::String(s.to_string()),
        Value::List(items) => JsonValue::Array(items.iter().map(value_to_json).collect()),
        Value::Map(map) => {
            let mut result = serde_json::Map::new();
            for (k, v) in map {
                if let Value::Str(key) = k {
                    result.insert(key.to_string(), value_to_json(v));
                } else {
                    result.insert(k.to_string(), value_to_json(v));
                }
            }
            JsonValue::Object(result)
        }
        _ => JsonValue::Null,
    }
}
