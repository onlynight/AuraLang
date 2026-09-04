//! std.iter — 迭代器/函数式工具
//!
//! 提供 map、filter、reduce、sort、sum、avg 等函数式集合操作。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("iter.sum", nat_sum);
    reg.register("iter.avg", nat_avg);
    reg.register("iter.min", nat_min);
    reg.register("iter.max", nat_max);
    reg.register("iter.product", nat_product);
    reg.register("iter.contains", nat_contains);
    reg.register("iter.indexOf", nat_index_of);
    reg.register("iter.count", nat_count);
    reg.register("iter.every", nat_every);
    reg.register("iter.some", nat_some);
    reg.register("iter.flatMap", nat_flat_map);
    reg.register("iter.zip", nat_zip);
    reg.register("iter.unzip", nat_unzip);
    reg.register("iter.enumerate", nat_enumerate);
    reg.register("iter.chain", nat_chain);
    reg.register("iter.take", nat_take);
    reg.register("iter.skip", nat_skip);
    reg.register("iter.dropWhile", nat_drop_while);
    reg.register("iter.takeWhile", nat_take_while);
    reg.register("iter.distinct", nat_distinct);
    reg.register("iter.groupBy", nat_group_by);
    reg.register("iter.partition", nat_partition);
    reg.register("iter.fold", nat_fold);
    reg.register("iter.scan", nat_scan);
    reg.register("iter.toMap", nat_to_map);
    reg.register("iter.toList", nat_to_list);
    reg.register("iter.range", nat_range);
    reg.register("iter.rangeTo", nat_range_to);
    reg.register("iter.rangeUntil", nat_range_until);
    reg.register("iter.repeatN", nat_repeat_n);
}

/// 安全取第一个元素
fn get_list(args: &[Value]) -> Option<&Vec<Value>> {
    match &args[0] {
        Value::List(items) => Some(items),
        _ => None,
    }
}

/// 取第一个参数
fn i0(args: &[Value]) -> i64 {
    args.first().map(|v| v.as_int()).unwrap_or(0)
}

/// iter.sum(list) → Float
fn nat_sum(args: &[Value]) -> Value {
    let sum = get_list(args).map_or(0.0, |items| {
        items.iter().map(|v| v.as_float()).sum::<f64>()
    });
    if sum.fract() == 0.0 {
        Value::Int(sum as i64)
    } else {
        Value::Float(sum)
    }
}

/// iter.avg(list) → Float
fn nat_avg(args: &[Value]) -> Value {
    match get_list(args) {
        Some(items) if !items.is_empty() => {
            let sum: f64 = items.iter().map(|v| v.as_float()).sum();
            Value::Float(sum / items.len() as f64)
        }
        _ => Value::Float(0.0),
    }
}

/// iter.min(list) → Value
fn nat_min(args: &[Value]) -> Value {
    get_list(args).and_then(|items| items.iter().min_by(|a, b| compare_values(a, b)).cloned()).unwrap_or(Value::Null)
}

/// iter.max(list) → Value
fn nat_max(args: &[Value]) -> Value {
    get_list(args).and_then(|items| items.iter().max_by(|a, b| compare_values(a, b)).cloned()).unwrap_or(Value::Null)
}

/// iter.product(list) → Float
fn nat_product(args: &[Value]) -> Value {
    let product = get_list(args).map_or(1.0, |items| {
        items.iter().map(|v| v.as_float()).product::<f64>()
    });
    if product.fract() == 0.0 {
        Value::Int(product as i64)
    } else {
        Value::Float(product)
    }
}

/// iter.contains(list, item) → Bool
fn nat_contains(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    Value::Bool(get_list(args).map_or(false, |items| items.contains(&item)))
}

/// iter.indexOf(list, item) → Int
fn nat_index_of(args: &[Value]) -> Value {
    let item = args.get(1).cloned().unwrap_or(Value::Null);
    let idx = get_list(args).and_then(|items| items.iter().position(|i| *i == item));
    Value::Int(idx.map(|p| p as i64).unwrap_or(-1))
}

/// iter.count(list) → Int
fn nat_count(args: &[Value]) -> Value {
    Value::Int(get_list(args).map_or(0, |items| items.len() as i64))
}

/// iter.every(list, predicate) → Bool (all items match predicate)
fn nat_every(args: &[Value]) -> Value {
    let predicate = args.get(1).cloned().unwrap_or(Value::Null);
    Value::Bool(get_list(args).map_or(true, |items| {
        items.iter().all(|item| matches_predicate(item, &predicate))
    }))
}

/// iter.some(list, predicate) → Bool (any item matches predicate)
fn nat_some(args: &[Value]) -> Value {
    let predicate = args.get(1).cloned().unwrap_or(Value::Null);
    Value::Bool(get_list(args).map_or(false, |items| {
        items.iter().any(|item| matches_predicate(item, &predicate))
    }))
}

/// iter.flatMap(list, fn) → List (flatten mapped results)
fn nat_flat_map(args: &[Value]) -> Value {
    let fn_val = args.get(1).cloned().unwrap_or(Value::Null);
    match get_list(args) {
        Some(items) => {
            let mut result = Vec::new();
            for item in items {
                let mapped = apply_function(item, &fn_val);
                match mapped {
                    Value::List(sub) => result.extend(sub),
                    other => result.push(other),
                }
            }
            Value::List(result)
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.zip(list1, list2, ...) → List of lists
fn nat_zip(args: &[Value]) -> Value {
    let lists: Vec<&Vec<Value>> = args
        .iter()
        .filter_map(|v| match v {
            Value::List(items) => Some(items),
            _ => None,
        })
        .collect();
    if lists.is_empty() {
        return Value::List(Vec::new());
    }
    let len = lists.iter().map(|l| l.len()).min().unwrap_or(0);
    let mut result = Vec::with_capacity(len);
    for i in 0..len {
        let tup: Vec<Value> = lists.iter().map(|l| l[i].clone()).collect();
        result.push(Value::List(tup));
    }
    Value::List(result)
}

/// iter.unzip(tuples) → List of two lists
fn nat_unzip(args: &[Value]) -> Value {
    match get_list(args) {
        Some(items) => {
            let mut first = Vec::new();
            let mut second = Vec::new();
            for item in items {
                if let Value::List(tup) = item {
                    if tup.len() >= 1 {
                        first.push(tup[0].clone());
                    }
                    if tup.len() >= 2 {
                        second.push(tup[1].clone());
                    }
                }
            }
            Value::List(vec![Value::List(first), Value::List(second)])
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.enumerate(list) → List of [index, value] pairs
fn nat_enumerate(args: &[Value]) -> Value {
    match get_list(args) {
        Some(items) => {
            let pairs: Vec<Value> = items
                .iter()
                .enumerate()
                .map(|(i, v)| Value::List(vec![Value::Int(i as i64), v.clone()]))
                .collect();
            Value::List(pairs)
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.chain(list1, list2, ...) → List (concatenated)
fn nat_chain(args: &[Value]) -> Value {
    let mut result = Vec::new();
    for arg in args {
        if let Value::List(items) = arg {
            result.extend(items.iter().cloned());
        }
    }
    Value::List(result)
}

/// iter.take(list, n) → List (first n items)
fn nat_take(args: &[Value]) -> Value {
    let n = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    match get_list(args) {
        Some(items) => Value::List(items.iter().take(n).cloned().collect()),
        None => Value::List(Vec::new()),
    }
}

/// iter.skip(list, n) → List (skip first n items)
fn nat_skip(args: &[Value]) -> Value {
    let n = args.get(1).map(|v| v.as_int() as usize).unwrap_or(0);
    match get_list(args) {
        Some(items) => Value::List(items.iter().skip(n).cloned().collect()),
        None => Value::List(Vec::new()),
    }
}

/// iter.dropWhile(list, predicate) → List (drop from start while predicate matches)
fn nat_drop_while(args: &[Value]) -> Value {
    let predicate = args.get(1).cloned().unwrap_or(Value::Null);
    match get_list(args) {
        Some(items) => {
            let dropped = items.iter().take_while(|item| matches_predicate(item, &predicate)).count();
            Value::List(items.iter().skip(dropped).cloned().collect())
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.takeWhile(list, predicate) → List (take from start while predicate matches)
fn nat_take_while(args: &[Value]) -> Value {
    let predicate = args.get(1).cloned().unwrap_or(Value::Null);
    match get_list(args) {
        Some(items) => {
            let taken: Vec<Value> = items
                .iter()
                .take_while(|item| matches_predicate(item, &predicate))
                .cloned()
                .collect();
            Value::List(taken)
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.distinct(list) → List (unique items)
fn nat_distinct(args: &[Value]) -> Value {
    match get_list(args) {
        Some(items) => {
            let mut unique = Vec::new();
            for item in items {
                if !unique.contains(item) {
                    unique.push(item.clone());
                }
            }
            Value::List(unique)
        }
        None => Value::List(Vec::new()),
    }
}

/// iter.groupBy(list, keyFn) → Map (group items by key)
fn nat_group_by(args: &[Value]) -> Value {
    let key_fn = args.get(1).cloned().unwrap_or(Value::Null);
    let mut map: std::collections::HashMap<Value, Value> = std::collections::HashMap::new();
    match get_list(args) {
        Some(items) => {
            for item in items {
                let key = apply_function(item, &key_fn);
                match map.get_mut(&key) {
                    Some(Value::List(list)) => {
                        list.push(item.clone());
                    }
                    _ => {
                        map.insert(key, Value::List(vec![item.clone()]));
                    }
                }
            }
        }
        None => {}
    }
    Value::Map(map)
}

/// iter.partition(list, predicate) → List of two lists [matches, nonMatches]
fn nat_partition(args: &[Value]) -> Value {
    let predicate = args.get(1).cloned().unwrap_or(Value::Null);
    let mut matches_list = Vec::new();
    let mut non_matches = Vec::new();
    match get_list(args) {
        Some(items) => {
            for item in items {
                if matches_predicate(item, &predicate) {
                    matches_list.push(item.clone());
                } else {
                    non_matches.push(item.clone());
                }
            }
        }
        None => {}
    }
    Value::List(vec![Value::List(matches_list), Value::List(non_matches)])
}

/// iter.fold(list, init, fn) → Value (reduce with initial value)
fn nat_fold(args: &[Value]) -> Value {
    if args.len() < 3 {
        return Value::Null;
    }
    let init = args[1].clone();
    let fn_val = args[2].clone();
    let mut acc = init;
    match get_list(args) {
        Some(items) => {
            for item in items {
                acc = apply_binary(acc, item.clone(), &fn_val);
            }
        }
        None => {}
    }
    acc
}

/// iter.scan(list, init, fn) → List (scan with initial value, returns intermediate values)
fn nat_scan(args: &[Value]) -> Value {
    if args.len() < 3 {
        return Value::List(Vec::new());
    }
    let init = args[1].clone();
    let fn_val = args[2].clone();
    let mut acc = init;
    let mut result = Vec::new();
    match get_list(args) {
        Some(items) => {
            for item in items {
                acc = apply_binary(acc, item.clone(), &fn_val);
                result.push(acc.clone());
            }
        }
        None => {}
    }
    Value::List(result)
}

/// iter.toMap(list, keyFn, valueFn) → Map
fn nat_to_map(args: &[Value]) -> Value {
    let key_fn = args.get(1).cloned().unwrap_or(Value::Null);
    let value_fn = args.get(2).cloned().unwrap_or(Value::Null);
    let mut map = std::collections::HashMap::new();
    match get_list(args) {
        Some(items) => {
            for item in items {
                let key = apply_function(item, &key_fn);
                let val = if key_fn == value_fn {
                    item.clone()
                } else {
                    apply_function(item, &value_fn)
                };
                map.insert(key, val);
            }
        }
        None => {}
    }
    Value::Map(map)
}

/// iter.toList(value) → List (wrap in list if not already a list)
fn nat_to_list(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::List(_)) => args[0].clone(),
        Some(v) => Value::List(vec![v.clone()]),
        None => Value::List(Vec::new()),
    }
}

/// iter.range(from, to) → List (inclusive range)
fn nat_range(args: &[Value]) -> Value {
    let from = i0(args);
    let to = args.get(1).map(|v| v.as_int()).unwrap_or(0);
    let range: std::ops::RangeInclusive<i64> = if from <= to {
        from..=to
    } else {
        from..=from
    };
    Value::List(range.map(Value::Int).collect())
}

/// iter.rangeTo(from, to) → List (alias for range)
fn nat_range_to(args: &[Value]) -> Value {
    nat_range(args)
}

/// iter.rangeUntil(from, to) → List (exclusive range)
fn nat_range_until(args: &[Value]) -> Value {
    let from = i0(args);
    let to = args.get(1).map(|v| v.as_int()).unwrap_or(0);
    let range: std::ops::Range<i64> = if from < to {
        from..to
    } else if from > to {
        (to + 1)..from
    } else {
        from..from
    };
    Value::List(range.map(Value::Int).collect())
}

/// iter.repeatN(value, count) → List
fn nat_repeat_n(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::List(Vec::new());
    }
    let value = args[0].clone();
    let count = i0(args);
    Value::List(vec![value; count as usize])
}

/// 应用一元函数
fn apply_function(arg: &Value, fn_val: &Value) -> Value {
    // Simple function application: if fn_val is a Ref to a closure, we can't call it
    // without the VM context. For now, return the argument unchanged.
    // This would need VM integration for full closure support.
    match fn_val {
        Value::Str(s) => {
            // Built-in function names
            let s = s.as_ref();
            match s.as_ref() {
                "toString" => Value::str_(arg.to_string()),
                "toInt" => Value::Int(arg.as_int()),
                "toFloat" => Value::Float(arg.as_float()),
                "toBool" => Value::Bool(arg.is_truthy()),
                "identity" => arg.clone(),
                "not" => Value::Bool(!arg.is_truthy()),
                "abs" => match arg {
                    Value::Int(i) => Value::Int(i.abs()),
                    Value::Float(f) => Value::Float(f.abs()),
                    _ => Value::Int(0),
                },
                _ => arg.clone(),
            }
        }
        _ => arg.clone(),
    }
}

/// 应用二元函数
fn apply_binary(a: Value, b: Value, fn_val: &Value) -> Value {
    match fn_val {
        Value::Str(s) => {
            let s = s.as_ref();
            match s.as_ref() {
                "add" | "+" => Value::Float(a.as_float() + b.as_float()),
                "sub" | "-" => Value::Float(a.as_float() - b.as_float()),
                "mul" | "*" => Value::Float(a.as_float() * b.as_float()),
                "div" | "/" => {
                    if b.as_float() != 0.0 {
                        Value::Float(a.as_float() / b.as_float())
                    } else {
                        Value::Float(0.0)
                    }
                }
                "concat" => Value::str_(a.to_string() + &b.to_string()),
                _ => a,
            }
        }
        _ => a,
    }
}

/// 谓词匹配：predicate 可以是字符串形式的内置谓词名
fn matches_predicate(item: &Value, predicate: &Value) -> bool {
    match predicate {
        Value::Str(s) => {
            let s = s.as_ref();
            match s {
                "isTruthy" => item.is_truthy(),
                "isZero" => item.as_int() == 0,
                "isPositive" => item.as_float() > 0.0,
                "isNegative" => item.as_float() < 0.0,
                "isEven" => item.as_int() % 2 == 0,
                "isOdd" => item.as_int() % 2 != 0,
                _ => item.is_truthy(),
            }
        }
        Value::Int(i) => item.as_int() == *i,
        Value::Float(f) => item.as_float() == *f,
        Value::Bool(b) => item.as_bool() == *b,
        Value::Null => false,
        _ => item.is_truthy(),
    }
}

/// 比较两个值
fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Str(x), Value::Str(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
        _ => std::cmp::Ordering::Equal,
    }
}
