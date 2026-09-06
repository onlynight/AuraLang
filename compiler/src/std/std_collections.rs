//! std.collections — 集合辅助构造函数
//!
//! 提供 Kotlin 风格的集合构造工厂函数：
//! `listOf`、`mutableListOf`、`mapOf`、`mutableMapOf`、`setOf`、`emptyList` 等。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    // 列表构造
    reg.register("aura.collections.listOf", nat_list_of);
    reg.register("aura.collections.mutableListOf", nat_mutable_list_of);
    reg.register("aura.collections.emptyList", nat_empty_list);
    reg.register("aura.collections.arrayOf", nat_array_of);
    // 列表操作
    reg.register("aura.collections.listContains", nat_list_contains);
    reg.register("aura.collections.listIndexOf", nat_list_index_of);
    reg.register("aura.collections.listRemove", nat_list_remove);
    reg.register("aura.collections.listReverse", nat_list_reverse);
    reg.register("aura.collections.listSort", nat_list_sort);
    reg.register("aura.collections.listGet", nat_list_get);
    reg.register("aura.collections.listSet", nat_list_set);
    reg.register("aura.collections.listInsert", nat_list_insert);
    reg.register("aura.collections.listSubList", nat_list_sub_list);
    // 映射构造
    reg.register("aura.collections.mapOf", nat_map_of);
    reg.register("aura.collections.mutableMapOf", nat_mutable_map_of);
    reg.register("aura.collections.emptyMap", nat_empty_map);
    reg.register("aura.collections.mapContains", nat_map_contains);
    reg.register("aura.collections.mapContainsKey", nat_map_contains_key);
    reg.register("aura.collections.mapContainsValue", nat_map_contains_value);
    reg.register("aura.collections.mapRemove", nat_map_remove);
    reg.register("aura.collections.mapKeys", nat_map_keys);
    reg.register("aura.collections.mapValues", nat_map_values);
    // 集合构造
    reg.register("aura.collections.setOf", nat_set_of);
    reg.register("aura.collections.mutableSetOf", nat_mutable_set_of);
    reg.register("aura.collections.emptySet", nat_empty_set);
}

/// listOf(items...) → List
fn nat_list_of(args: &[Value]) -> Value {
    let items: Vec<Value> = args.iter().cloned().collect();
    Value::List(items)
}

/// mutableListOf(items...) → MutableList (same as List in VM)
fn nat_mutable_list_of(args: &[Value]) -> Value {
    nat_list_of(args)
}

/// emptyList() → empty List
fn nat_empty_list(_args: &[Value]) -> Value {
    Value::List(Vec::new())
}

/// arrayOf(items...) → Array (stored as List)
fn nat_array_of(args: &[Value]) -> Value {
    nat_list_of(args)
}

/// listContains(list, item) → Bool
fn nat_list_contains(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => Value::Bool(items.contains(&item)),
        _ => Value::Bool(false),
    }
}

/// listIndexOf(list, item) → Int
fn nat_list_index_of(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            Value::Int(items.iter().position(|i| *i == item).map(|p| p as i64).unwrap_or(-1))
        }
        _ => Value::Int(-1),
    }
}

/// listRemove(list, item) → Bool
fn nat_list_remove(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            let before = items.len();
            let mut new_items = items.clone();
            new_items.retain(|i| *i != item);
            Value::Bool(new_items.len() < before)
        }
        _ => Value::Bool(false),
    }
}

/// listReverse(list) → List
fn nat_list_reverse(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            new_items.reverse();
            Value::List(new_items)
        }
        _ => Value::List(Vec::new()),
    }
}

/// listSort(list) → List (sorted by Int/Float/String)
fn nat_list_sort(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            new_items.sort_by(|a, b| compare_values(a, b));
            Value::List(new_items)
        }
        _ => Value::List(Vec::new()),
    }
}

/// listGet(list, index) → Value
fn nat_list_get(args: &[Value]) -> Value {
    let idx = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    match &args[0] {
        Value::List(items) => items.get(idx).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// listSet(list, index, value) → Unit
fn nat_list_set(args: &[Value]) -> Value {
    let idx = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    let val = args.get(2).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            if idx < new_items.len() {
                new_items[idx] = val;
            }
            Value::List(new_items)
        }
        _ => Value::Null,
    }
}

/// listInsert(list, index, value) → Unit
fn nat_list_insert(args: &[Value]) -> Value {
    let idx = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    let val = args.get(2).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            if idx <= new_items.len() {
                new_items.insert(idx, val);
            }
            Value::List(new_items)
        }
        _ => Value::Null,
    }
}

/// listSubList(list, from, to) → List
fn nat_list_sub_list(args: &[Value]) -> Value {
    let from = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    let to = args.get(2).map(|v| v.as_int() as usize).unwrap_or(0);
    match &args[0] {
        Value::List(items) => {
            let end = to.min(items.len());
            let start = from.min(end);
            Value::List(items[start..end].to_vec())
        }
        _ => Value::List(Vec::new()),
    }
}

/// mapOf(k1, v1, k2, v2, ...) → Map
fn nat_map_of(args: &[Value]) -> Value {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        map.insert(args[i].clone(), args[i + 1].clone());
        i += 2;
    }
    Value::Map(map)
}

/// mutableMapOf(k1, v1, k2, v2, ...) → Map (same as Map in VM)
fn nat_mutable_map_of(args: &[Value]) -> Value {
    nat_map_of(args)
}

/// emptyMap() → empty Map
fn nat_empty_map(_args: &[Value]) -> Value {
    Value::Map(std::collections::HashMap::new())
}

/// mapContains(map, value) → Bool
fn nat_map_contains(args: &[Value]) -> Value {
    let val = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::Map(map) => Value::Bool(map.values().any(|v| *v == val)),
        _ => Value::Bool(false),
    }
}

/// mapContainsKey(map, key) → Bool
fn nat_map_contains_key(args: &[Value]) -> Value {
    let key = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::Map(map) => Value::Bool(map.contains_key(&key)),
        _ => Value::Bool(false),
    }
}

/// mapContainsValue(map, value) → Bool
fn nat_map_contains_value(args: &[Value]) -> Value {
    nat_map_contains(args)
}

/// mapRemove(map, key) → Value or null
fn nat_map_remove(args: &[Value]) -> Value {
    let key = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::Map(map) => {
            let mut new_map = map.clone();
            new_map.remove(&key).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

/// mapKeys(map) → List of keys
fn nat_map_keys(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => Value::List(map.keys().cloned().collect()),
        _ => Value::List(Vec::new()),
    }
}

/// mapValues(map) → List of values
fn nat_map_values(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => Value::List(map.values().cloned().collect()),
        _ => Value::List(Vec::new()),
    }
}

/// setOf(items...) → List (unique)
fn nat_set_of(args: &[Value]) -> Value {
    let mut unique: Vec<Value> = Vec::new();
    for item in args {
        if !unique.contains(item) {
            unique.push(item.clone());
        }
    }
    Value::List(unique)
}

/// mutableSetOf(items...) → List (unique)
fn nat_mutable_set_of(args: &[Value]) -> Value {
    nat_set_of(args)
}

/// emptySet() → empty List
fn nat_empty_set(_args: &[Value]) -> Value {
    Value::List(Vec::new())
}

/// 比较两个值用于排序
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => {
            (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Float(x), Value::Int(y)) => {
            x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        _ => std::cmp::Ordering::Equal,
    }
}
