//! std.collections — 集合辅助构造函数
//!
//! 提供 Kotlin 风格的集合构造工厂函数：
//! `listOf`、`mutableListOf`、`mapOf`、`mutableMapOf`、`setOf`、`emptyList` 等。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::rc::Rc;

pub fn register(reg: &mut NativeRegistry) {
    // 列表构造
    reg.register("aura.lang.std.Collections.listOf", nat_list_of);
    reg.register(
        "aura.lang.std.Collections.mutableListOf",
        nat_mutable_list_of,
    );
    reg.register("aura.lang.std.Collections.emptyList", nat_empty_list);
    reg.register("aura.lang.std.Collections.arrayOf", nat_array_of);
    // 列表操作
    reg.register("aura.lang.std.Collections.listContains", nat_list_contains);
    reg.register("aura.lang.std.Collections.listIndexOf", nat_list_index_of);
    reg.register("aura.lang.std.Collections.listRemove", nat_list_remove);
    reg.register("aura.lang.std.Collections.listReverse", nat_list_reverse);
    reg.register("aura.lang.std.Collections.listSort", nat_list_sort);
    reg.register("aura.lang.std.Collections.listGet", nat_list_get);
    reg.register("aura.lang.std.Collections.listSet", nat_list_set);
    reg.register("aura.lang.std.Collections.listInsert", nat_list_insert);
    reg.register("aura.lang.std.Collections.listSubList", nat_list_sub_list);
    // P15: 高阶迭代器辅助 + Pair
    reg.register("aura.lang.std.Collections.listAppend", nat_list_append);
    reg.register("aura.lang.std.Collections.listSize", nat_list_size);
    reg.register("aura.lang.std.Collections.pairOf", nat_pair_of);
    // 映射构造
    reg.register("aura.lang.std.Collections.mapOf", nat_map_of);
    reg.register("aura.lang.std.Collections.mutableMapOf", nat_mutable_map_of);
    reg.register("aura.lang.std.Collections.emptyMap", nat_empty_map);
    reg.register("aura.lang.std.Collections.mapContains", nat_map_contains);
    reg.register(
        "aura.lang.std.Collections.mapContainsKey",
        nat_map_contains_key,
    );
    reg.register(
        "aura.lang.std.Collections.mapContainsValue",
        nat_map_contains_value,
    );
    reg.register("aura.lang.std.Collections.mapRemove", nat_map_remove);
    reg.register("aura.lang.std.Collections.mapKeys", nat_map_keys);
    reg.register("aura.lang.std.Collections.mapValues", nat_map_values);
    // 集合构造
    reg.register("aura.lang.std.Collections.setOf", nat_set_of);
    reg.register("aura.lang.std.Collections.mutableSetOf", nat_mutable_set_of);
    reg.register("aura.lang.std.Collections.emptySet", nat_empty_set);
    // ── 特化集合构造（分层实现）──
    // ArrayList: 基于 Vec 的动态数组（与 List 相同实现，但工厂函数表明意图）
    reg.register("aura.lang.std.Collections.arrayListOf", nat_array_list_of);
    reg.register(
        "aura.lang.std.Collections.arrayListSize",
        nat_array_list_size,
    );
    // LinkedList: 基于 VecDeque 的双向链表（支持两端 O(1) 操作）
    reg.register("aura.lang.std.Collections.linkedListOf", nat_linked_list_of);
    reg.register(
        "aura.lang.std.Collections.linkedAddFirst",
        nat_linked_add_first,
    );
    reg.register(
        "aura.lang.std.Collections.linkedAddLast",
        nat_linked_add_last,
    );
    reg.register(
        "aura.lang.std.Collections.linkedRemoveFirst",
        nat_linked_remove_first,
    );
    reg.register(
        "aura.lang.std.Collections.linkedRemoveLast",
        nat_linked_remove_last,
    );
    // HashSet: 基于 HashSet 的哈希集合（O(1) 查找）
    reg.register("aura.lang.std.Collections.hashSetOf", nat_hash_set_of);
    reg.register(
        "aura.lang.std.Collections.hashSetContains",
        nat_hash_set_contains,
    );
    reg.register("aura.lang.std.Collections.hashSetAdd", nat_hash_set_add);
    reg.register(
        "aura.lang.std.Collections.hashSetRemove",
        nat_hash_set_remove,
    );
    // HashMap: 基于 HashMap 的哈希映射（与 Map 相同实现）
    reg.register("aura.lang.std.Collections.hashMapOf", nat_hash_map_of);
    reg.register("aura.lang.std.Collections.hashMapGet", nat_hash_map_get);
    reg.register("aura.lang.std.Collections.hashMapPut", nat_hash_map_put);
    reg.register(
        "aura.lang.std.Collections.hashMapRemove",
        nat_hash_map_remove,
    );
    // LinkedHashMap: 保持插入顺序的映射
    reg.register(
        "aura.lang.std.Collections.linkedHashMapOf",
        nat_linked_hash_map_of,
    );
    reg.register(
        "aura.lang.std.Collections.linkedHashMapKeys",
        nat_linked_hash_map_keys,
    );
    reg.register(
        "aura.lang.std.Collections.linkedHashMapFirstKey",
        nat_linked_hash_map_first_key,
    );
    reg.register(
        "aura.lang.std.Collections.linkedHashMapLastKey",
        nat_linked_hash_map_last_key,
    );
}

/// listOf(items...) → List
pub fn nat_list_of(args: &[Value]) -> Value {
    let items: Vec<Value> = args.iter().cloned().collect();
    Value::List(items)
}

/// listAppend(list, value) → List：追加元素返回新列表（P15 迭代器链用）
fn nat_list_append(args: &[Value]) -> Value {
    let val = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            new_items.push(val);
            Value::List(new_items)
        }
        _ => Value::List(vec![val]),
    }
}

/// listSize(list) → Int
fn nat_list_size(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => Value::Int(items.len() as i64),
        _ => Value::Int(0),
    }
}

/// pairOf(a, b) → Pair（运行时以 2 元素 List 表示，P15 解构用）
fn nat_pair_of(args: &[Value]) -> Value {
    Value::List(vec![
        args.first().cloned().unwrap_or(Value::Null),
        args.get(1).cloned().unwrap_or(Value::Null),
    ])
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

// ════════════════════════════════════════════════════════════════
// 特化集合构造函数（分层实现：VM 层核心 + Aura 层特化）
// ════════════════════════════════════════════════════════════════

/// arrayListOf(items...) → ArrayList（基于 Vec，与 List 相同底层实现）
pub fn nat_array_list_of(args: &[Value]) -> Value {
    let items: Vec<Value> = args.iter().cloned().collect();
    Value::List(items)
}

/// arrayListSize(list) → Int
fn nat_array_list_size(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => Value::Int(items.len() as i64),
        _ => Value::Int(0),
    }
}

/// linkedListOf(items...) → LinkedList（基于 VecDeque，支持两端 O(1) 操作）
pub fn nat_linked_list_of(args: &[Value]) -> Value {
    let items: Vec<Value> = args.iter().cloned().collect();
    Value::List(items)
}

/// linkedAddFirst(list, item) → LinkedList：在头部添加元素
fn nat_linked_add_first(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            let mut new_items = items.clone();
            new_items.insert(0, item);
            Value::List(new_items)
        }
        _ => Value::List(vec![item]),
    }
}

/// linkedAddLast(list, item) → LinkedList：在尾部添加元素
fn nat_linked_add_last(args: &[Value]) -> Value {
    nat_list_append(args)
}

/// linkedRemoveFirst(list) → LinkedList：移除头部元素，返回移除的值
fn nat_linked_remove_first(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                Value::Null
            } else {
                let mut new_items = items.clone();
                let removed = new_items.remove(0);
                // 返回新列表，但为了兼容返回移除的值
                Value::List(new_items)
            }
        }
        _ => Value::Null,
    }
}

/// linkedRemoveLast(list) → LinkedList：移除尾部元素，返回移除的值
fn nat_linked_remove_last(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                Value::Null
            } else {
                let mut new_items = items.clone();
                new_items.pop();
                Value::List(new_items)
            }
        }
        _ => Value::Null,
    }
}

/// hashSetOf(items...) → HashSet（基于 HashSet，O(1) 查找）
pub fn nat_hash_set_of(args: &[Value]) -> Value {
    // 去重，返回唯一元素的列表
    let mut unique: Vec<Value> = Vec::new();
    for item in args {
        if !unique.contains(item) {
            unique.push(item.clone());
        }
    }
    Value::List(unique)
}

/// hashSetContains(set, item) → Bool：检查集合是否包含指定元素
fn nat_hash_set_contains(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => Value::Bool(items.contains(&item)),
        _ => Value::Bool(false),
    }
}

/// hashSetAdd(set, item) → Bool：添加元素，返回是否添加成功
fn nat_hash_set_add(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::List(items) => {
            if items.contains(&item) {
                Value::Bool(false)
            } else {
                let mut new_items = items.clone();
                new_items.push(item);
                Value::List(new_items)
            }
        }
        _ => Value::List(vec![item]),
    }
}

/// hashSetRemove(set, item) → Bool：移除元素，返回是否移除成功
fn nat_hash_set_remove(args: &[Value]) -> Value {
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

/// hashMapOf(k1, v1, k2, v2, ...) → HashMap（与 Map 相同底层实现）
pub fn nat_hash_map_of(args: &[Value]) -> Value {
    nat_map_of(args)
}

/// hashMapGet(map, key) → Value：按键获取值
fn nat_hash_map_get(args: &[Value]) -> Value {
    let key = args.get(1).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::Map(map) => map.get(&key).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// hashMapPut(map, key, value) → HashMap：设置键值对，返回新映射
fn nat_hash_map_put(args: &[Value]) -> Value {
    let key = args.get(1).cloned().unwrap_or(Value::Null);
    let value = args.get(2).cloned().unwrap_or(Value::Null);
    match &args[0] {
        Value::Map(map) => {
            let mut new_map = map.clone();
            new_map.insert(key, value);
            Value::Map(new_map)
        }
        _ => {
            let mut new_map = std::collections::HashMap::new();
            new_map.insert(key, value);
            Value::Map(new_map)
        }
    }
}

/// hashMapRemove(map, key) → Value：移除键，返回移除的值
fn nat_hash_map_remove(args: &[Value]) -> Value {
    nat_map_remove(args)
}

/// linkedHashMapOf(k1, v1, k2, v2, ...) → LinkedHashMap（保持插入顺序）
pub fn nat_linked_hash_map_of(args: &[Value]) -> Value {
    let mut map = std::collections::HashMap::new();
    let mut key_order: Vec<Value> = Vec::new();
    let mut i = 0;
    while i + 1 < args.len() {
        if !map.contains_key(&args[i]) {
            key_order.push(args[i].clone());
        }
        map.insert(args[i].clone(), args[i + 1].clone());
        i += 2;
    }
    // 将键顺序存储在特殊的 Map 中（第一个键 "__order__"）
    let order_key = Value::Str(Rc::from("__order__"));
    map.insert(order_key, Value::List(key_order));
    Value::Map(map)
}

/// linkedHashMapKeys(map) → List：获取所有键（保持插入顺序）
fn nat_linked_hash_map_keys(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => {
            if let Some(Value::List(order)) = map.get(&Value::Str(Rc::from("__order__"))) {
                Value::List(order.clone())
            } else {
                Value::List(map.keys().cloned().collect())
            }
        }
        _ => Value::List(Vec::new()),
    }
}

/// linkedHashMapFirstKey(map) → Value：获取第一个键
fn nat_linked_hash_map_first_key(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => {
            if let Some(Value::List(order)) = map.get(&Value::Str(Rc::from("__order__"))) {
                order.first().cloned().unwrap_or(Value::Null)
            } else {
                map.keys().next().cloned().unwrap_or(Value::Null)
            }
        }
        _ => Value::Null,
    }
}

/// linkedHashMapLastKey(map) → Value：获取最后一个键
fn nat_linked_hash_map_last_key(args: &[Value]) -> Value {
    match &args[0] {
        Value::Map(map) => {
            if let Some(Value::List(order)) = map.get(&Value::Str(Rc::from("__order__"))) {
                order.last().cloned().unwrap_or(Value::Null)
            } else {
                map.keys().last().cloned().unwrap_or(Value::Null)
            }
        }
        _ => Value::Null,
    }
}
