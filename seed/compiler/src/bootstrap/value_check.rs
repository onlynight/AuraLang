//! 空值/数值检查核心（Layer 0，不能上移）。
//!
//! `isNull` / `isNotNull` / `isZero` / `isPositive` / `isNegative` /
//! `isNaN` / `isInfinite`：三态执行模式共享的谓词语义。

use super::vm_core::Value;

/// `isNull(v)`：仅 Null 为真。
pub fn is_null(v: &Value) -> bool {
    matches!(v, Value::Null)
}

/// `isNotNull(v)`：`!isNull(v)`。
pub fn is_not_null(v: &Value) -> bool {
    !is_null(v)
}

/// `isZero(v)`：数值零（Int(0) / Float(±0.0)）为真，其余类型为假。
pub fn is_zero(v: &Value) -> bool {
    match v {
        Value::Int(n) => *n == 0,
        Value::Float(f) => *f == 0.0,
        _ => false,
    }
}

/// `isPositive(v)`：严格大于零。
pub fn is_positive(v: &Value) -> bool {
    match v {
        Value::Int(n) => *n > 0,
        Value::Float(f) => *f > 0.0,
        _ => false,
    }
}

/// `isNegative(v)`：严格小于零（±0.0 均不算负）。
pub fn is_negative(v: &Value) -> bool {
    match v {
        Value::Int(n) => *n < 0,
        Value::Float(f) => *f < 0.0,
        _ => false,
    }
}

/// `isNaN(v)`：仅 Float NaN 为真。
pub fn is_nan(v: &Value) -> bool {
    matches!(v, Value::Float(f) if f.is_nan())
}

/// `isInfinite(v)`：仅 Float ±inf 为真。
pub fn is_infinite(v: &Value) -> bool {
    matches!(v, Value::Float(f) if f.is_infinite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_checks() {
        assert!(is_null(&Value::Null));
        assert!(!is_null(&Value::Int(0)));
        assert!(is_not_null(&Value::Int(0)));
        assert!(!is_not_null(&Value::Null));
    }

    #[test]
    fn zero_and_sign() {
        assert!(is_zero(&Value::Int(0)));
        assert!(is_zero(&Value::Float(-0.0)));
        assert!(!is_zero(&Value::Float(f64::NAN)));
        assert!(is_positive(&Value::Int(3)) && is_negative(&Value::Int(-3)));
        assert!(!is_positive(&Value::Float(0.0)) && !is_negative(&Value::Float(0.0)));
    }

    #[test]
    fn nan_and_infinite() {
        assert!(is_nan(&Value::Float(f64::NAN)));
        assert!(!is_nan(&Value::Int(0)));
        assert!(is_infinite(&Value::Float(f64::NEG_INFINITY)));
        assert!(!is_infinite(&Value::Float(1e308 * 1e-308)));
    }
}
