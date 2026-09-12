//! Any 核心虚方法（Layer 0，不能上移）。
//!
//! `toString()` / `equals()` / `hashCode()` 是所有值的基础协议，
//! 由 Rust native 实现，三态执行模式（VM/JIT/AOT）共享同一语义。

use std::rc::Rc;

use super::vm_core::Value;

/// `Any.toString()`：值的规范字符串表示。
pub fn to_string(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => {
            if f.is_nan() {
                "nan".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 { "inf".to_string() } else { "-inf".to_string() }
            } else {
                format!("{f}")
            }
        }
        Value::Str(s) => s.to_string(),
        Value::Ptr(p) => format!("ptr({p:#x})"),
    }
}

/// `Any.equals()`：值相等（Int/Float 数值相等，Str 按内容）。
pub fn equals(a: &Value, b: &Value) -> bool {
    a == b
}

/// `Any.hashCode()`：FNV-1a 64 位哈希，跨类型分布均匀且稳定。
pub fn hash_code(v: &Value) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut h = FNV_OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
    };

    feed(v.type_name().as_bytes());
    match v {
        Value::Null => {}
        Value::Bool(b) => feed(&[*b as u8]),
        Value::Int(n) => feed(&n.to_le_bytes()),
        Value::Float(f) => {
            // 规范化：0.0 与 -0.0 同哈希；NaN 全部同哈希
            let bits = if f.is_nan() {
                f64::NAN.to_bits()
            } else if *f == 0.0 {
                0f64.to_bits()
            } else {
                f.to_bits()
            };
            feed(&bits.to_le_bytes())
        }
        Value::Str(s) => feed(s.as_bytes()),
        Value::Ptr(p) => feed(&p.to_le_bytes()),
    }
    h
}

/// 便捷构造：`Rc<str>`。
pub fn str_value(s: &str) -> Value {
    Value::Str(Rc::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_string_basics() {
        assert_eq!(to_string(&Value::Null), "null");
        assert_eq!(to_string(&Value::Bool(true)), "true");
        assert_eq!(to_string(&Value::Int(-7)), "-7");
        assert_eq!(to_string(&Value::Float(1.5)), "1.5");
        assert_eq!(to_string(&str_value("hi")), "hi");
    }

    #[test]
    fn hash_stable_and_distinct() {
        let a = hash_code(&Value::Int(42));
        let b = hash_code(&Value::Int(42));
        let c = hash_code(&Value::Int(43));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(hash_code(&Value::Float(42.0)), a);
    }

    #[test]
    fn zero_sign_and_nan_normalized() {
        assert_eq!(
            hash_code(&Value::Float(0.0)),
            hash_code(&Value::Float(-0.0))
        );
        assert_eq!(
            hash_code(&Value::Float(f64::NAN)),
            hash_code(&Value::Float(-f64::NAN))
        );
    }
}
