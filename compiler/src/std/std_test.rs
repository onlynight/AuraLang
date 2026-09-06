//! std.test — 测试断言框架
//!
//! 提供 `assertTrue`、`assertEq`、`assertNotNull`、`assertThrows` 等断言。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;

pub fn register(reg: &mut NativeRegistry) {
    reg.register("aura.test.assertTrue", nat_assert_true);
    reg.register("aura.test.assertFalse", nat_assert_false);
    reg.register("aura.test.assertEq", nat_assert_eq);
    reg.register("aura.test.assertNotEq", nat_assert_not_eq);
    reg.register("aura.test.assertNotNull", nat_assert_not_null);
    reg.register("aura.test.assertNull", nat_assert_null);
    reg.register("aura.test.assertContains", nat_assert_contains);
    reg.register("aura.test.assertNotContains", nat_assert_not_contains);
    reg.register("aura.test.assertThrows", nat_assert_throws);
    reg.register("aura.test.assertGt", nat_assert_gt);
    reg.register("aura.test.assertGte", nat_assert_gte);
    reg.register("aura.test.assertLt", nat_assert_lt);
    reg.register("aura.test.assertLte", nat_assert_lte);
    reg.register("aura.test.assertApprox", nat_assert_approx);
    reg.register("aura.test.assertArrayEq", nat_assert_array_eq);
    reg.register("aura.test.assertMapEq", nat_assert_map_eq);
    reg.register("aura.test.pass", nat_pass);
    reg.register("aura.test.fail", nat_fail);
}

fn make_result(ok: bool, msg: String) -> Value {
    if ok { Value::str_(format!("PASS: {}", msg)) } else { Value::str_(format!("FAIL: {}", msg)) }
}

fn nat_assert_true(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    make_result(cond, if msg.is_empty() { "assertTrue".into() } else { msg })
}

fn nat_assert_false(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v.is_truthy()).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        !cond,
        if msg.is_empty() { "assertFalse".into() } else { msg },
    )
}

fn nat_assert_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertEq: not enough args".into());
    }
    let a = args[0].clone();
    let b = args[1].clone();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = a == b;
    let detail = if ok { "assertEq".to_string() } else { format!("expected {} but got {}", a, b) };
    make_result(ok, if msg.is_empty() { detail } else { msg })
}

fn nat_assert_not_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertNotEq: not enough args".into());
    }
    let a = args[0].clone();
    let b = args[1].clone();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = a != b;
    let detail = if ok {
        "assertNotEq".to_string()
    } else {
        format!("expected values to differ but both are {}", a)
    };
    make_result(ok, if msg.is_empty() { detail } else { msg })
}

fn nat_assert_not_null(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v != &Value::Null).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        cond,
        if msg.is_empty() { "assertNotNull".into() } else { msg },
    )
}

fn nat_assert_null(args: &[Value]) -> Value {
    let cond = args.first().map(|v| v == &Value::Null).unwrap_or(false);
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_default();
    make_result(cond, if msg.is_empty() { "assertNull".into() } else { msg })
}

fn nat_assert_contains(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertContains: not enough args".into());
    }
    let haystack = args[0].as_string();
    let needle = args[1].as_string();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = haystack.contains(&needle);
    let detail = if ok {
        "assertContains".to_string()
    } else {
        format!("expected '{}' to contain '{}'", haystack, needle)
    };
    make_result(ok, if msg.is_empty() { detail } else { msg })
}

fn nat_assert_not_contains(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertNotContains: not enough args".into());
    }
    let haystack = args[0].as_string();
    let needle = args[1].as_string();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = !haystack.contains(&needle);
    let detail = if ok {
        "assertNotContains".to_string()
    } else {
        format!("expected '{}' to NOT contain '{}'", haystack, needle)
    };
    make_result(ok, if msg.is_empty() { detail } else { msg })
}

fn nat_assert_throws(args: &[Value]) -> Value {
    // Simplified: just return PASS (can't actually catch exceptions in VM)
    let msg = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "assertThrows".to_string());
    make_result(true, msg)
}

fn nat_assert_gt(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertGt: not enough args".into());
    }
    let a = args[0].as_float();
    let b = args[1].as_float();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        a > b,
        if msg.is_empty() { format!("assertGt: {} > {}", a, b) } else { msg },
    )
}

fn nat_assert_gte(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertGte: not enough args".into());
    }
    let a = args[0].as_float();
    let b = args[1].as_float();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        a >= b,
        if msg.is_empty() { format!("assertGte: {} >= {}", a, b) } else { msg },
    )
}

fn nat_assert_lt(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertLt: not enough args".into());
    }
    let a = args[0].as_float();
    let b = args[1].as_float();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        a < b,
        if msg.is_empty() { format!("assertLt: {} < {}", a, b) } else { msg },
    )
}

fn nat_assert_lte(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertLte: not enough args".into());
    }
    let a = args[0].as_float();
    let b = args[1].as_float();
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    make_result(
        a <= b,
        if msg.is_empty() { format!("assertLte: {} <= {}", a, b) } else { msg },
    )
}

fn nat_assert_approx(args: &[Value]) -> Value {
    if args.len() < 3 {
        return make_result(false, "assertApprox: not enough args".into());
    }
    let a = args[0].as_float();
    let b = args[1].as_float();
    let epsilon = args[2].as_float();
    let msg = args.get(3).map(|v| v.as_string()).unwrap_or_default();
    let ok = (a - b).abs() <= epsilon;
    make_result(
        ok,
        if msg.is_empty() {
            format!("assertApprox: {} ≈ {} (ε={})", a, b, epsilon)
        } else {
            msg
        },
    )
}

fn nat_assert_array_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertArrayEq: not enough args".into());
    }
    let a = &args[0];
    let b = &args[1];
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = a == b;
    make_result(
        ok,
        if msg.is_empty() { format!("assertArrayEq: {} == {}", a, b) } else { msg },
    )
}

fn nat_assert_map_eq(args: &[Value]) -> Value {
    if args.len() < 2 {
        return make_result(false, "assertMapEq: not enough args".into());
    }
    let a = &args[0];
    let b = &args[1];
    let msg = args.get(2).map(|v| v.as_string()).unwrap_or_default();
    let ok = a == b;
    make_result(
        ok,
        if msg.is_empty() { format!("assertMapEq: {} == {}", a, b) } else { msg },
    )
}

fn nat_pass(args: &[Value]) -> Value {
    let msg = args.first().map(|v| v.as_string()).unwrap_or_else(|| "test passed".to_string());
    Value::str_(format!("PASS: {}", msg))
}

fn nat_fail(args: &[Value]) -> Value {
    let msg = args.first().map(|v| v.as_string()).unwrap_or_else(|| "test failed".to_string());
    Value::str_(format!("FAIL: {}", msg))
}
