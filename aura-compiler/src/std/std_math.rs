//! std.math — 数学函数与常量
//!
//! 提供三角函数、对数、幂函数、取整、随机数以及常用数学常量。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("math.abs", nat_abs);
    reg.register("math.min", nat_min);
    reg.register("math.max", nat_max);
    reg.register("math.ceil", nat_ceil);
    reg.register("math.floor", nat_floor);
    reg.register("math.round", nat_round);
    reg.register("math.trunc", nat_trunc);
    reg.register("math.sqrt", nat_sqrt);
    reg.register("math.cbrt", nat_cbrt);
    reg.register("math.pow", nat_pow);
    reg.register("math.exp", nat_exp);
    reg.register("math.log", nat_log);
    reg.register("math.log2", nat_log2);
    reg.register("math.log10", nat_log10);
    reg.register("math.sin", nat_sin);
    reg.register("math.cos", nat_cos);
    reg.register("math.tan", nat_tan);
    reg.register("math.asin", nat_asin);
    reg.register("math.acos", nat_acos);
    reg.register("math.atan", nat_atan);
    reg.register("math.atan2", nat_atan2);
    reg.register("math.PI", nat_pi);
    reg.register("math.E", nat_e);
    reg.register("math.INT_MAX", nat_int_max);
    reg.register("math.INT_MIN", nat_int_min);
    reg.register("math.FLOAT_MAX", nat_float_max);
    reg.register("math.sign", nat_sign);
    reg.register("math.clamp", nat_clamp);
}

fn arg0(args: &[Value]) -> f64 {
    args.first().map(|v| v.as_float()).unwrap_or(0.0)
}

fn arg1(args: &[Value]) -> f64 {
    args.get(1).map(|v| v.as_float()).unwrap_or(0.0)
}

fn int_arg0(args: &[Value]) -> i64 {
    args.first().map(|v| v.as_int()).unwrap_or(0)
}

fn int_arg1(args: &[Value]) -> i64 {
    args.get(1).map(|v| v.as_int()).unwrap_or(0)
}

fn nat_abs(args: &[Value]) -> Value {
    let v = args.first();
    match v {
        Some(Value::Int(i)) => Value::Int(i.abs()),
        Some(Value::Float(f)) => Value::Float(f.abs()),
        _ => Value::Int(0),
    }
}

fn nat_min(args: &[Value]) -> Value {
    let a = int_arg0(args);
    let b = int_arg1(args);
    Value::Int(a.min(b))
}

fn nat_max(args: &[Value]) -> Value {
    let a = int_arg0(args);
    let b = int_arg1(args);
    Value::Int(a.max(b))
}

fn nat_ceil(args: &[Value]) -> Value {
    Value::Float(arg0(args).ceil())
}

fn nat_floor(args: &[Value]) -> Value {
    Value::Float(arg0(args).floor())
}

fn nat_round(args: &[Value]) -> Value {
    Value::Int(arg0(args).round() as i64)
}

fn nat_trunc(args: &[Value]) -> Value {
    Value::Float(arg0(args).trunc())
}

fn nat_sqrt(args: &[Value]) -> Value {
    Value::Float(arg0(args).sqrt())
}

fn nat_cbrt(args: &[Value]) -> Value {
    Value::Float(arg0(args).cbrt())
}

fn nat_pow(args: &[Value]) -> Value {
    Value::Float(arg0(args).powf(arg1(args)))
}

fn nat_exp(args: &[Value]) -> Value {
    Value::Float(arg0(args).exp())
}

fn nat_log(args: &[Value]) -> Value {
    Value::Float(arg0(args).ln())
}

fn nat_log2(args: &[Value]) -> Value {
    Value::Float(arg0(args).log2())
}

fn nat_log10(args: &[Value]) -> Value {
    Value::Float(arg0(args).log10())
}

fn nat_sin(args: &[Value]) -> Value {
    Value::Float(arg0(args).sin())
}

fn nat_cos(args: &[Value]) -> Value {
    Value::Float(arg0(args).cos())
}

fn nat_tan(args: &[Value]) -> Value {
    Value::Float(arg0(args).tan())
}

fn nat_asin(args: &[Value]) -> Value {
    Value::Float(arg0(args).asin())
}

fn nat_acos(args: &[Value]) -> Value {
    Value::Float(arg0(args).acos())
}

fn nat_atan(args: &[Value]) -> Value {
    Value::Float(arg0(args).atan())
}

fn nat_atan2(args: &[Value]) -> Value {
    Value::Float(arg0(args).atan2(arg1(args)))
}

fn nat_pi(_args: &[Value]) -> Value {
    Value::Float(std::f64::consts::PI)
}

fn nat_e(_args: &[Value]) -> Value {
    Value::Float(std::f64::consts::E)
}

fn nat_int_max(_args: &[Value]) -> Value {
    Value::Int(i64::MAX)
}

fn nat_int_min(_args: &[Value]) -> Value {
    Value::Int(i64::MIN)
}

fn nat_float_max(_args: &[Value]) -> Value {
    Value::Float(f64::MAX)
}

fn nat_sign(args: &[Value]) -> Value {
    let v = arg0(args);
    Value::Int(v.signum() as i64)
}

fn nat_clamp(args: &[Value]) -> Value {
    let v = arg0(args);
    let lo = arg1(args);
    let hi = args.get(2).map(|v| v.as_float()).unwrap_or(f64::MAX);
    Value::Float(v.max(lo).min(hi))
}
