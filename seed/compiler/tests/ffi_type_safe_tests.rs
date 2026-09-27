#![cfg(feature = "llvm")]

//! Fix 7 — 静态链接类型安全调用测试
//!
//! 验证 CFuncInfo + CType 类型转换机制。

use compiler::vm::Value;
use compiler::vm::ffi::{CFuncInfo, CType};

// ─────────────────────────────────────────────────────────────────────────────
// 1. CType 解析
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ctype_from_str_int32() {
    assert_eq!(CType::from_str("int32"), Some(CType::Int32));
    assert_eq!(CType::from_str("int32_t"), Some(CType::Int32));
    assert_eq!(CType::from_str("i32"), Some(CType::Int32));
}

#[test]
fn test_ctype_from_str_int64() {
    assert_eq!(CType::from_str("int64"), Some(CType::Int64));
    assert_eq!(CType::from_str("int64_t"), Some(CType::Int64));
    assert_eq!(CType::from_str("i64"), Some(CType::Int64));
}

#[test]
fn test_ctype_from_str_float() {
    assert_eq!(CType::from_str("float"), Some(CType::Float32));
    assert_eq!(CType::from_str("f32"), Some(CType::Float32));
    assert_eq!(CType::from_str("double"), Some(CType::Float64));
    assert_eq!(CType::from_str("f64"), Some(CType::Float64));
}

#[test]
fn test_ctype_from_str_bool() {
    assert_eq!(CType::from_str("bool"), Some(CType::Bool));
    assert_eq!(CType::from_str("boolean"), Some(CType::Bool));
}

#[test]
fn test_ctype_from_str_unknown() {
    assert_eq!(CType::from_str("unknown"), None);
    assert_eq!(CType::from_str(""), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. CType pack/unpack
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_ctype_int_pack_unpack() {
    let ty = CType::Int64;
    let value = Value::Int(42);
    let packed = ty.pack(&value);
    assert_eq!(packed, 42);
    let unpacked = ty.unpack(packed);
    assert_eq!(unpacked, Value::Int(42));
}

#[test]
fn test_ctype_int32_pack_unpack() {
    let ty = CType::Int32;
    let value = Value::Int(42);
    let packed = ty.pack(&value);
    assert_eq!(packed, 42);
    let unpacked = ty.unpack(packed);
    assert_eq!(unpacked, Value::Int(42));
}

#[test]
fn test_ctype_float_pack_unpack() {
    let ty = CType::Float64;
    let value = Value::Float(3.14);
    let packed = ty.pack(&value);
    let unpacked = ty.unpack(packed);
    if let Value::Float(f) = unpacked {
        assert!((f - 3.14).abs() < 0.0001);
    } else {
        panic!("Expected Float value");
    }
}

#[test]
fn test_ctype_bool_pack_unpack() {
    let ty = CType::Bool;
    let value = Value::Bool(true);
    let packed = ty.pack(&value);
    assert_eq!(packed, 1);
    let unpacked = ty.unpack(packed);
    assert_eq!(unpacked, Value::Bool(true));

    let value2 = Value::Bool(false);
    let packed2 = ty.pack(&value2);
    assert_eq!(packed2, 0);
    let unpacked2 = ty.unpack(packed2);
    assert_eq!(unpacked2, Value::Bool(false));
}

#[test]
fn test_ctype_ptr_pack_unpack() {
    let ty = CType::Ptr;
    let value = Value::Ptr(12345);
    let packed = ty.pack(&value);
    assert_eq!(packed, 12345);
    let unpacked = ty.unpack(packed);
    assert_eq!(unpacked, Value::Ptr(12345));
}

#[test]
fn test_ctype_void_unpack() {
    let ty = CType::Void;
    let unpacked = ty.unpack(0);
    assert_eq!(unpacked, Value::Null);
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. CFuncInfo 构造
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cfunc_info_construction() {
    let info = CFuncInfo {
        name: "myFunc".to_string(),
        param_types: vec![
            CType::Int32,
            CType::Int64,
            CType::Float64,
        ],
        return_type: CType::Bool,
    };
    assert_eq!(info.name, "myFunc");
    assert_eq!(info.param_types.len(), 3);
    assert_eq!(info.return_type, CType::Bool);
}

#[test]
fn test_cfunc_info_empty_params() {
    let info = CFuncInfo {
        name: "noArgFunc".to_string(),
        param_types: vec![],
        return_type: CType::Int64,
    };
    assert_eq!(info.param_types.len(), 0);
}
