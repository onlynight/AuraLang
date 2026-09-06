//! 共享调用约定 ABI (JitValue / AotEntry)
//!
//! Phase 1: AOT 机器码嵌入方案的核心基础设施。
//! JitValue 和 AotEntry 从 jit.rs 提取到此处，使 AOT 嵌入路径
//! 不依赖 `jit` feature flag。

use crate::vm::value::Value;

// ─────────────────────────────────────────────────────────────────────────────
// JitValue: 双字段结构，VM/JIT/AOT 共享
// ─────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JitValue {
    /// 类型标签
    pub tag: i64,
    /// 值载荷
    pub payload: i64,
}

// ── 类型标签常量 ──

pub const TAG_INT: i64 = 0;
pub const TAG_FLOAT: i64 = 1;
pub const TAG_BOOL: i64 = 2;
pub const TAG_NULL: i64 = 3;
pub const TAG_STR: i64 = 4;
pub const TAG_PTR: i64 = 5;
pub const TAG_OBJ: i64 = 6;
pub const TAG_FUNC: i64 = 7;
pub const TAG_ARRAY: i64 = 8;
pub const TAG_LIST: i64 = 9;
pub const TAG_MAP: i64 = 10;
pub const TAG_CLOSURE: i64 = 11;
pub const TAG_CSTRING: i64 = 12;

impl JitValue {
    /// 空值
    pub fn null() -> Self {
        JitValue {
            tag: TAG_NULL,
            payload: 0,
        }
    }

    /// 从 Aura Value 转换为 JitValue
    pub fn from_value(v: &Value) -> Self {
        match v {
            Value::Int(i) => JitValue {
                tag: TAG_INT,
                payload: *i,
            },
            Value::Float(f) => JitValue {
                tag: TAG_FLOAT,
                payload: f.to_bits() as i64,
            },
            Value::Bool(b) => JitValue {
                tag: TAG_BOOL,
                payload: *b as i64,
            },
            Value::Str(s) => JitValue {
                tag: TAG_STR,
                payload: s.as_ptr() as i64,
            },
            Value::Ptr(p) => JitValue {
                tag: TAG_PTR,
                payload: *p,
            },
            Value::Ref(h) => JitValue {
                tag: TAG_OBJ,
                payload: *h as i64,
            },
            Value::Weak(h) => JitValue {
                tag: TAG_OBJ,
                payload: *h as i64,
            },
            Value::List(items) => JitValue {
                tag: TAG_LIST,
                payload: items.as_ptr() as i64,
            },
            Value::Map(_map) => JitValue {
                // Phase 2: HashMap 无 as_ptr()，使用空指针占位
                // 实际使用中 Map 由 VM 堆管理（NewMap → Value::Ref），
                // AOT 模式下 Map 参数应为 heap handle (TAG_OBJ)
                tag: TAG_MAP,
                payload: 0,
            },
            Value::Null => JitValue {
                tag: TAG_NULL,
                payload: 0,
            },
        }
    }

    /// 从 JitValue 转换回 Aura Value
    pub fn to_value(self) -> Value {
        match self.tag {
            TAG_INT => Value::Int(self.payload),
            TAG_FLOAT => Value::Float(f64::from_bits(self.payload as u64)),
            TAG_BOOL => Value::Bool(self.payload != 0),
            TAG_PTR => Value::Ptr(self.payload),
            TAG_OBJ => Value::Ref(self.payload as usize),
            TAG_LIST => {
                // List 指针：回退为 Null（完整引用恢复需 VM 堆支持）
                // 实际场景：AOT 函数返回 List 时，VM 会重新包装
                Value::Null
            }
            TAG_MAP => {
                Value::Null
            }
            TAG_STR => {
                // 字符串指针：需要 VM 堆读取
                // 实际场景：AOT 返回的字符串指针指向 VM 堆上的 C 字符串
                Value::Null
            }
            TAG_ARRAY => Value::Null,
            TAG_CLOSURE => Value::Null,
            TAG_FUNC => Value::Null,
            TAG_CSTRING => Value::Ptr(self.payload),
            TAG_NULL => Value::Null,
            _ => Value::Null,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AotEntry: AOT 机器码入口 (与 JitEntry 签名相同)
// ─────────────────────────────────────────────────────────────────────────────

pub type AotEntry =
    unsafe extern "C" fn(args: *const JitValue, ret: *mut JitValue, argc: usize, ctx: *const ());

// ─────────────────────────────────────────────────────────────────────────────
// AotCallContext: AOT 调用上下文
// ─────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AotCallContext {
    pub runtime: *mut (),
    pub module_id: u32,
    pub func_idx: u32,
    pub call_depth: u32,
    pub exception: i32,
}

impl AotCallContext {
    pub fn new() -> Self {
        AotCallContext {
            runtime: std::ptr::null_mut(),
            module_id: 0,
            func_idx: 0,
            call_depth: 0,
            exception: 0,
        }
    }
}

impl Default for AotCallContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jitvalue_null() {
        let v = JitValue::null();
        assert_eq!(v.tag, TAG_NULL);
        assert_eq!(v.payload, 0);
    }

    #[test]
    fn test_jitvalue_int_roundtrip() {
        let val = Value::Int(42);
        let jv = JitValue::from_value(&val);
        assert_eq!(jv.tag, TAG_INT);
        assert_eq!(jv.payload, 42);
        let back = jv.to_value();
        assert_eq!(back, val);
    }

    #[test]
    fn test_jitvalue_float_roundtrip() {
        let val = Value::Float(3.14);
        let jv = JitValue::from_value(&val);
        assert_eq!(jv.tag, TAG_FLOAT);
        let back = jv.to_value();
        assert_eq!(back.as_float(), 3.14);
    }

    #[test]
    fn test_jitvalue_bool_roundtrip() {
        let val = Value::Bool(true);
        let jv = JitValue::from_value(&val);
        assert_eq!(jv.tag, TAG_BOOL);
        assert_eq!(jv.payload, 1);
        let back = jv.to_value();
        assert_eq!(back, val);
    }

    #[test]
    fn test_context_new() {
        let ctx = AotCallContext::new();
        assert!(ctx.runtime.is_null());
        assert_eq!(ctx.exception, 0);
    }
}
