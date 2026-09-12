//! 类型内省核心（Layer 0，不能上移）。
//!
//! `typeOf()` / `isOfType()` / `cast()`：三态执行模式共享的类型查询
//! 与转换语义，由 Rust native 实现。

use super::Trap;
use super::any_core;
use super::vm_core::Value;

/// 类型名常量（与 `Value::type_name` 保持一致）。
pub const T_NULL: &str = "Null";
pub const T_BOOL: &str = "Bool";
pub const T_INT: &str = "Int";
pub const T_FLOAT: &str = "Float";
pub const T_STR: &str = "Str";
pub const T_POINTER: &str = "Pointer";

/// `typeOf(v)`：返回值的运行时类型名。
pub fn type_of(v: &Value) -> &'static str {
    v.type_name()
}

/// `isOfType(v, name)`：类型检查。
pub fn is_of_type(v: &Value, type_name: &str) -> bool {
    v.type_name() == type_name
}

/// `cast(v, target)`：类型转换。
///
/// 支持的转换：
/// - → Int：Int 恒等、Bool(0/1)、Float 截断、Str 解析
/// - → Float：Float 恒等、Int 提升、Str 解析
/// - → Str：任意值的字符串表示
/// - → Bool：Int(≠0)、Float(≠0)、Str("true"/"false")、Null→false
/// - → Pointer：Pointer 恒等、Int 位模式
pub fn cast(v: &Value, target: &str) -> Result<Value, Trap> {
    match target {
        T_INT => match v {
            Value::Int(_) => Ok(v.clone()),
            Value::Bool(b) => Ok(Value::Int(*b as i64)),
            Value::Float(f) => Ok(Value::Int(*f as i64)),
            Value::Str(s) => s
                .trim()
                .parse::<i64>()
                .map(Value::Int)
                .map_err(|_| Trap::new(format!("cast: 无法将 \"{s}\" 转换为 Int"))),
            _ => Err(Trap::new(format!("cast: {} 无法转换为 Int", v.type_name()))),
        },
        T_FLOAT => match v {
            Value::Float(_) => Ok(v.clone()),
            Value::Int(n) => Ok(Value::Float(*n as f64)),
            Value::Str(s) => s
                .trim()
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| Trap::new(format!("cast: 无法将 \"{s}\" 转换为 Float"))),
            _ => Err(Trap::new(format!(
                "cast: {} 无法转换为 Float",
                v.type_name()
            ))),
        },
        T_STR => Ok(Value::Str(std::rc::Rc::from(
            any_core::to_string(v).as_str(),
        ))),
        T_BOOL => match v {
            Value::Bool(_) => Ok(v.clone()),
            Value::Null => Ok(Value::Bool(false)),
            Value::Int(n) => Ok(Value::Bool(*n != 0)),
            Value::Float(f) => Ok(Value::Bool(*f != 0.0)),
            Value::Str(s) => match s.as_ref() {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(Trap::new(format!("cast: 无法将 \"{s}\" 转换为 Bool"))),
            },
            _ => Err(Trap::new("cast: Pointer 无法转换为 Bool")),
        },
        T_POINTER => match v {
            Value::Ptr(_) => Ok(v.clone()),
            Value::Int(n) => Ok(Value::Ptr(*n as usize)),
            Value::Null => Ok(Value::Ptr(0)),
            _ => Err(Trap::new(format!(
                "cast: {} 无法转换为 Pointer",
                v.type_name()
            ))),
        },
        T_NULL => {
            if matches!(v, Value::Null) {
                Ok(v.clone())
            } else {
                Err(Trap::new("cast: 非 Null 值无法转换为 Null"))
            }
        }
        other => Err(Trap::new(format!("cast: 未知目标类型 {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::any_core::str_value;

    #[test]
    fn type_names() {
        assert_eq!(type_of(&Value::Null), T_NULL);
        assert_eq!(type_of(&Value::Int(1)), T_INT);
        assert_eq!(type_of(&Value::Float(1.0)), T_FLOAT);
        assert_eq!(type_of(&str_value("x")), T_STR);
        assert_eq!(type_of(&Value::Ptr(8)), T_POINTER);
    }

    #[test]
    fn casts() {
        assert_eq!(cast(&Value::Bool(true), T_INT).unwrap(), Value::Int(1));
        assert_eq!(cast(&Value::Float(2.9), T_INT).unwrap(), Value::Int(2));
        assert_eq!(cast(&Value::Int(3), T_FLOAT).unwrap(), Value::Float(3.0));
        assert_eq!(cast(&str_value("12"), T_INT).unwrap(), Value::Int(12));
        assert_eq!(cast(&Value::Int(9), T_STR).unwrap(), str_value("9"));
        assert_eq!(cast(&Value::Int(0), T_BOOL).unwrap(), Value::Bool(false));
        assert_eq!(cast(&Value::Int(64), T_POINTER).unwrap(), Value::Ptr(64));
        assert!(cast(&str_value("abc"), T_INT).is_err());
    }
}
