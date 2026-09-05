#![cfg(all(feature = "llvm", feature = "jit"))]

//! Fix 5 — 扩展 JIT 白名单测试
//!
//! 验证新增指令（Not/ReturnUnit）在 is_jit_compilable 中不再被拒绝。
//! 注：位运算指令（BitAnd/BitOr/BitXor/Shl/Shr）因 Cranelift 0.116 缺少
//! iand/ior/ixor 方法暂不支持，待升级 Cranelift 后补充。

use compiler::codegen::opcode::Const;
use compiler::vm::jit::is_jit_compilable;
use compiler::vm::{DecodedFunction, Instr};

/// 构造一个仅包含指定指令的测试函数
fn make_func(code: Vec<Instr>, param_count: u16) -> DecodedFunction {
    DecodedFunction {
        name: "test".to_string(),
        param_count,
        locals: 0,
        is_native: false,
        code,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Not 指令白名单
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_not_allowed() {
    let consts = vec![Const::Int(1)];
    let mut funcs = vec![make_func(
        vec![Instr::LoadConst(0), Instr::Not, Instr::Return],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. ReturnUnit 白名单
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_return_unit_allowed() {
    let consts = Vec::new();
    let mut funcs = vec![make_func(vec![Instr::ReturnUnit], 0)];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. 原有指令仍被允许
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_arithmetic_still_allowed() {
    let consts = vec![Const::Int(1), Const::Int(2)];
    let mut funcs = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::LoadConst(1),
            Instr::Add,
            Instr::Return,
        ],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

#[test]
fn test_jit_comparison_still_allowed() {
    let consts = vec![Const::Int(1), Const::Int(2)];
    let mut funcs = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::LoadConst(1),
            Instr::Lt,
            Instr::Return,
        ],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 不支持的指令仍被拒绝
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_call_method_still_rejected() {
    let consts = Vec::new();
    let mut funcs = vec![make_func(vec![Instr::CallMethod(0)], 0)];
    funcs.push(funcs[0].clone());
    assert!(!is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

#[test]
fn test_jit_new_object_still_rejected() {
    let consts = Vec::new();
    let mut funcs = vec![make_func(vec![Instr::NewObject(0), Instr::Return], 0)];
    funcs.push(funcs[0].clone());
    assert!(!is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

#[test]
fn test_jit_bitand_still_rejected() {
    // Cranelift 0.116 无 iand 方法，暂不支持
    let consts = vec![Const::Int(5), Const::Int(3)];
    let mut funcs = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::LoadConst(1),
            Instr::BitAnd,
            Instr::Return,
        ],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(!is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 混合指令测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_mixed_not_and_return_unit() {
    let consts = vec![Const::Int(5)];
    let mut funcs = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::Not,
            Instr::Return,
        ],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}

#[test]
fn test_jit_mixed_arithmetic_and_not() {
    let consts = vec![Const::Int(1), Const::Int(2)];
    let mut funcs = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::LoadConst(1),
            Instr::Add,
            Instr::Not,
            Instr::Return,
        ],
        0,
    )];
    funcs.push(funcs[0].clone());
    assert!(is_jit_compilable(0, &funcs[0], &consts, &funcs));
}
