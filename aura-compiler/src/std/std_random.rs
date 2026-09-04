//! std.random — 随机数生成
//!
//! 使用 `rand` crate 提供随机数、洗牌、选择等随机操作。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use rand::Rng;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.random.nextInt", nat_next_int);
    reg.register("aura.random.nextLong", nat_next_long);
    reg.register("aura.random.nextFloat", nat_next_float);
    reg.register("aura.random.nextDouble", nat_next_double);
    reg.register("aura.random.nextBool", nat_next_bool);
    reg.register("aura.random.nextIntRange", nat_next_int_range);
    reg.register("aura.random.nextFloatRange", nat_next_float_range);
    reg.register("aura.random.choice", nat_choice);
    reg.register("aura.random.shuffle", nat_shuffle);
    reg.register("aura.random.seed", nat_seed);
    reg.register("aura.random.random", nat_random);
}

/// 获取全局随机数生成器
fn get_rng() -> rand::rngs::ThreadRng {
    rand::thread_rng()
}

/// random.nextInt() → Int (full range)
fn nat_next_int(_args: &[Value]) -> Value {
    let val = get_rng().gen_range(i64::MIN..i64::MAX);
    Value::Int(val)
}

/// random.nextLong() → Int (same as nextInt for 64-bit)
fn nat_next_long(_args: &[Value]) -> Value {
    nat_next_int(_args)
}

/// random.nextFloat() → Float [0.0, 1.0)
fn nat_next_float(_args: &[Value]) -> Value {
    Value::Float(get_rng().r#gen::<f64>())
}

/// random.nextDouble() → Float (alias)
fn nat_next_double(_args: &[Value]) -> Value {
    nat_next_float(_args)
}

/// random.nextBool() → Bool
fn nat_next_bool(_args: &[Value]) -> Value {
    Value::Bool(get_rng().r#gen::<bool>())
}

/// random.nextIntRange(min, max) → Int in [min, max)
fn nat_next_int_range(args: &[Value]) -> Value {
    let min = args.first().map(|v| v.as_int()).unwrap_or(0);
    let max = args.get(1).map(|v| v.as_int()).unwrap_or(i64::MAX);
    let val = get_rng().gen_range(min..max);
    Value::Int(val)
}

/// random.nextFloatRange(min, max) → Float in [min, max)
fn nat_next_float_range(args: &[Value]) -> Value {
    let min = args.first().map(|v| v.as_float()).unwrap_or(0.0);
    let max = args.get(1).map(|v| v.as_float()).unwrap_or(1.0);
    Value::Float(get_rng().gen_range(min..max))
}

/// random.choice(items...) → one random item
fn nat_choice(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let idx = get_rng().gen_range(0..args.len());
    args[idx].clone()
}

/// random.shuffle(list) → shuffled List
fn nat_shuffle(args: &[Value]) -> Value {
    match &args[0] {
        Value::List(items) => {
            let mut shuffled = items.clone();
            // Fisher-Yates shuffle
            let mut rng = get_rng();
            for i in (1..shuffled.len()).rev() {
                let j = rng.gen_range(0..=i);
                shuffled.swap(i, j);
            }
            Value::List(shuffled)
        }
        _ => Value::List(Vec::new()),
    }
}

/// random.seed(seed) → Unit (seed not truly used in ThreadRng)
fn nat_seed(_args: &[Value]) -> Value {
    Value::Null
}

/// random.random() → Float [0.0, 1.0) (alias)
fn nat_random(_args: &[Value]) -> Value {
    nat_next_float(_args)
}
