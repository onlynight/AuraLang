//! std.assert — 通用断言
//!
//! 提供编译期和运行期的断言检查。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.assert.assert", nat_assert);
    reg.register("aura.assert.assertTrue", nat_assert_true);
    reg.register("aura.assert.assertFalse", nat_assert_false);
    reg.register("aura.assert.assertEq", nat_assert_eq);
    reg.register("aura.assert.assertNotEq", nat_assert_not_eq);
    reg.register("aura.assert.assertNotNull", nat_assert_not_null);
    reg.register("aura.assert.assertNull", nat_assert_null);
    reg.register("aura.assert.debugAssert", nat_debug_assert);
}

fn ok_result(msg: String) -> Value {
    Value::str_(format!("OK: {}", msg))
}

fn fail_result(msg: String) -> Value {
    Value::str_(format!("ASSERTION FAILED: {}", msg))
}

/// assert.assert(condition, message) → PASS or FAIL
fn nat_assert(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assert".to_string());
    if cond {
        ok_result(msg)
    } else {
        fail_result(msg)
    }
}

/// assert.assertTrue(condition, message) → PASS or FAIL
fn nat_assert_true(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assertTrue".to_string());
    if cond {
        ok_result(msg)
    } else {
        fail_result(msg)
    }
}

/// assert.assertFalse(condition, message) → PASS or FAIL
fn nat_assert_false(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assertFalse".to_string());
    if !cond {
        ok_result(msg)
    } else {
        fail_result(msg)
    }
}

/// assert.assertEq(a, b, message) → PASS or FAIL
fn nat_assert_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return fail_result("assertEq: not enough args".to_string());
    }
    let a = args[0].clone();
    let b = args[1].clone();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_else(|| format!("{} == {}", a, b));
    if a == b {
        ok_result(msg)
    } else {
        fail_result(format!("expected {} == {} but got {} vs {}", a, b, a, b))
    }
}

/// assert.assertNotEq(a, b, message) → PASS or FAIL
fn nat_assert_not_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return fail_result("assertNotEq: not enough args".to_string());
    }
    let a = args[0].clone();
    let b = args[1].clone();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_else(|| format!("{} != {}", a, b));
    if a != b {
        ok_result(msg)
    } else {
        fail_result(format!("expected {} != {} but both are {}", a, b, a))
    }
}

/// assert.assertNotNull(value, message) → PASS or FAIL
fn nat_assert_not_null(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v != &Value::Null).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assertNotNull".to_string());
    if cond {
        ok_result(msg)
    } else {
        fail_result(msg)
    }
}

/// assert.assertNull(value, message) → PASS or FAIL
fn nat_assert_null(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v == &Value::Null).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assertNull".to_string());
    if cond {
        ok_result(msg)
    } else {
        fail_result(msg)
    }
}

/// assert.debugAssert(condition, message) → PASS or FAIL (debug only)
fn nat_debug_assert(args: &[Value]) -> Value {
    nat_assert(args)
}
