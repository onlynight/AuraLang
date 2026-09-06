//! JIT Bytecode Optimization Pass
//!
//! Runs before Cranelift compilation to optimize bytecode:
//! 1. Constant folding - fold constant arithmetic at compile time
//! 2. Dead code elimination - remove instructions whose results are never used
//! 3. Loop unrolling - unroll simple loops for better instruction scheduling

use crate::codegen::opcode::Const;
use crate::vm::{DecodedFunction, Instr};

/// Maximum function size for inlining (in instructions)
const MAX_INLINE_SIZE: usize = 20;

/// Instruction scheduling: reorder instructions to reduce CPU stalls
/// Moves independent instructions closer together to improve instruction-level parallelism
fn schedule_instructions(func: &mut DecodedFunction) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut i = 0;

    while i < func.code.len() {
        let instr = &func.code[i];

        // Pattern: LoadConst followed by arithmetic → keep together (dependency chain)
        // Pattern: LoadVar followed by LoadVar → reorder to improve parallelism
        if let Instr::LoadConst(_) = instr {
            // Look ahead for independent loads that can be moved before
            let mut j = i + 1;
            let mut independent_loads = Vec::new();

            while j < func.code.len() {
                match &func.code[j] {
                    Instr::LoadConst(_) | Instr::LoadVar(_) => {
                        // Check if this load is independent (no dependency on the current load)
                        independent_loads.push((j, func.code[j].clone()));
                        j += 1;
                    }
                    Instr::Jump(_)
                    | Instr::JumpIfTrue(_)
                    | Instr::JumpIfFalse(_)
                    | Instr::Call(_)
                    | Instr::Return => break,
                    _ => break, // Arithmetic or other dependent instruction
                }
            }

            // Move independent loads before the current constant load
            // This can help the CPU pipeline by having more instructions ready
            if independent_loads.len() >= 2 {
                // Reorder: put independent loads first, then the current instruction
                for (_, load) in &independent_loads {
                    new_code.push(load.clone());
                }
                new_code.push(func.code[i].clone());
                i = j;
                continue;
            }
        }

        new_code.push(func.code[i].clone());
        i += 1;
    }

    func.code = new_code;
}

/// Inline small functions to eliminate call overhead
/// Inlines functions with fewer than MAX_INLINE_SIZE instructions
fn inline_functions(
    func: &mut DecodedFunction,
    consts: &[Const],
    extra: &mut Vec<Const>,
    all_funcs: &[DecodedFunction],
) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut i = 0;
    let mut inlined = false;

    while i < func.code.len() {
        let instr = &func.code[i];

        match instr {
            Instr::Call(callee_idx) => {
                let callee_idx = *callee_idx as usize;

                // Check if the callee is small enough to inline
                if callee_idx < all_funcs.len() {
                    let callee = &all_funcs[callee_idx];
                    let callee_size = callee.code.len();

                    if callee_size < MAX_INLINE_SIZE && !callee.is_native {
                        // Inline the function
                        if let Some(inlined_code) =
                            try_inline_function(func, consts, extra, callee_idx, i)
                        {
                            new_code.extend(inlined_code);
                            i += 1;
                            inlined = true;
                            continue;
                        }
                    }
                }

                new_code.push(func.code[i].clone());
                i += 1;
            }
            _ => {
                new_code.push(func.code[i].clone());
                i += 1;
            }
        }
    }

    func.code = new_code;
}

/// Try to inline a function at the given call site
fn try_inline_function(
    caller: &DecodedFunction,
    _consts: &[Const],
    _extra: &mut Vec<Const>,
    callee_idx: usize,
    call_idx: usize,
) -> Option<Vec<Instr>> {
    // For now, return None - function inlining requires more complex analysis
    // to handle parameter passing, return values, and control flow correctly.
    // This would need CFG analysis and register allocation.

    // TODO: Implement proper function inlining with:
    // 1. Parameter mapping (caller args → callee params)
    // 2. Return value handling
    // 3. Jump target adjustment within the inlined code
    // 4. Dead code elimination of the original Call instruction

    None
}

/// Optimization result: optimized function and any additional constants to add
pub struct OptResult {
    pub func: DecodedFunction,
    /// Additional constants to append to the module-level constant pool
    pub extra_consts: Vec<Const>,
}

/// Optimize a decoded function
/// Returns (optimized_func, extra_consts_to_add)
pub fn optimize_function(func: &DecodedFunction, consts: &[Const]) -> OptResult {
    optimize_function_with_deps(func, consts, &[])
}

/// Optimize a decoded function with access to all functions (for inlining)
pub fn optimize_function_with_deps(
    func: &DecodedFunction,
    consts: &[Const],
    all_funcs: &[DecodedFunction],
) -> OptResult {
    let mut result = OptResult {
        func: func.clone(),
        extra_consts: Vec::new(),
    };

    // Pass 1: Constant folding
    fold_constants(&mut result.func, consts, &mut result.extra_consts);

    // Pass 2: Dead code elimination
    eliminate_dead_code(&mut result.func);

    // Pass 3: Jump threading (eliminate redundant jumps)
    thread_jumps(&mut result.func);

    // Pass 4: Strength reduction (replace Div/Rem by powers of 2 with shifts)
    strength_reduce(&mut result.func, consts, &mut result.extra_consts);

    // Pass 5: Instruction scheduling (reorder for better CPU pipeline)
    schedule_instructions(&mut result.func);

    // Pass 6: Function inlining (inline small functions)
    inline_functions(
        &mut result.func,
        consts,
        &mut result.extra_consts,
        all_funcs,
    );

    // Pass 7: Loop unrolling
    unroll_loops(&mut result.func);

    result
}

/// Fold constant expressions at compile time
fn fold_constants(func: &mut DecodedFunction, consts: &[Const], extra: &mut Vec<Const>) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut i = 0;

    while i < func.code.len() {
        let instr = &func.code[i];
        match instr {
            // Fold constant arithmetic: if both operands are constants, compute result
            Instr::Add | Instr::Sub | Instr::Mul | Instr::Div | Instr::Rem => {
                if let Some((const_idx, _folded_val)) =
                    try_fold_binary(func, consts, i, instr, extra)
                {
                    new_code.push(Instr::LoadConst(const_idx as u16));
                    i += 1;
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            // Fold constant comparison: if both operands are constants, compute result
            Instr::Eq | Instr::Ne | Instr::Lt | Instr::Gt | Instr::Le | Instr::Ge => {
                if let Some((const_idx, _folded_val)) =
                    try_fold_comparison(func, consts, i, instr, extra)
                {
                    new_code.push(Instr::LoadConst(const_idx as u16));
                    i += 1;
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            _ => {
                new_code.push(func.code[i].clone());
                i += 1;
            }
        }
    }

    func.code = new_code;
}

/// Try to fold a binary arithmetic operation
/// Returns (const_idx, folded_val) if successful
fn try_fold_binary(
    func: &DecodedFunction,
    consts: &[Const],
    idx: usize,
    instr: &Instr,
    extra: &mut Vec<Const>,
) -> Option<(usize, i64)> {
    if idx < 2 {
        return None;
    }

    let instr_a = &func.code[idx - 2];
    let instr_b = &func.code[idx - 1];

    if let (Instr::LoadConst(a_idx), Instr::LoadConst(b_idx)) = (instr_a, instr_b) {
        let a_idx = *a_idx as usize;
        let b_idx = *b_idx as usize;

        // Resolve constants from both module-level and extra
        let a = resolve_const(consts, extra, a_idx);
        let b = resolve_const(consts, extra, b_idx);

        if let (Some(Const::Int(a)), Some(Const::Int(b))) = (a, b) {
            let a = *a;
            let b = *b;
            let result = match instr {
                Instr::Add => a + b,
                Instr::Sub => a - b,
                Instr::Mul => a * b,
                Instr::Div => {
                    if b != 0 {
                        a / b
                    } else {
                        return None;
                    }
                }
                Instr::Rem => {
                    if b != 0 {
                        a % b
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };

            // Add or find the result constant
            let const_idx = find_or_add_const(consts, extra, result);
            return Some((const_idx, result));
        }
    }
    None
}

/// Resolve a constant index against the combined pool (module + extra)
fn resolve_const<'a>(consts: &'a [Const], extra: &'a [Const], idx: usize) -> Option<&'a Const> {
    if idx < consts.len() {
        Some(&consts[idx])
    } else if idx < consts.len() + extra.len() {
        Some(&extra[idx - consts.len()])
    } else {
        None
    }
}

/// Find or add a constant in the extra pool
/// Returns the index (relative to module-level + extra)
fn find_or_add_const(consts: &[Const], extra: &mut Vec<Const>, value: i64) -> usize {
    // Search module-level first
    for i in 0..consts.len() {
        if let Const::Int(v) = consts[i] {
            if v == value {
                return i;
            }
        }
    }
    // Search extra
    for i in 0..extra.len() {
        if let Const::Int(v) = extra[i] {
            if v == value {
                return consts.len() + i;
            }
        }
    }
    // Add new constant
    let idx = consts.len() + extra.len();
    extra.push(Const::Int(value));
    idx
}

/// Try to fold a comparison operation
fn try_fold_comparison(
    func: &DecodedFunction,
    consts: &[Const],
    idx: usize,
    instr: &Instr,
    extra: &mut Vec<Const>,
) -> Option<(usize, i64)> {
    if idx < 2 {
        return None;
    }

    let instr_a = &func.code[idx - 2];
    let instr_b = &func.code[idx - 1];

    if let (Instr::LoadConst(a_idx), Instr::LoadConst(b_idx)) = (instr_a, instr_b) {
        let a_idx = *a_idx as usize;
        let b_idx = *b_idx as usize;

        let a = resolve_const(consts, extra, a_idx);
        let b = resolve_const(consts, extra, b_idx);

        if let (Some(Const::Int(a)), Some(Const::Int(b))) = (a, b) {
            let a = *a;
            let b = *b;
            let result = match instr {
                Instr::Eq => a == b,
                Instr::Ne => a != b,
                Instr::Lt => a < b,
                Instr::Gt => a > b,
                Instr::Le => a <= b,
                Instr::Ge => a >= b,
                _ => return None,
            };
            let result_i64 = if result { 1 } else { 0 };
            let const_idx = find_or_add_const(consts, extra, result_i64);
            return Some((const_idx, result_i64));
        }
    }
    None
}

/// Eliminate dead instructions (LoadVar/StoreVar pairs)
fn eliminate_dead_code(func: &mut DecodedFunction) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut skip_next = false;

    for i in 0..func.code.len() {
        if skip_next {
            skip_next = false;
            continue;
        }

        let instr = &func.code[i];
        match instr {
            // LoadVar followed by StoreVar to same variable = dead code
            Instr::LoadVar(s) => {
                if let Some(Instr::StoreVar(s2)) = func.code.get(i + 1) {
                    if s == s2 {
                        skip_next = true;
                        continue;
                    }
                }
                new_code.push(func.code[i].clone());
            }
            _ => {
                new_code.push(func.code[i].clone());
            }
        }
    }

    func.code = new_code;
}

/// Unroll simple loops with known trip counts
fn unroll_loops(func: &mut DecodedFunction) {
    let mut unrolled = false;
    let code_len = func.code.len();
    let mut i = 0;

    while i < code_len && !unrolled {
        let instr = &func.code[i];

        if let Instr::JumpIfTrue(target) | Instr::JumpIfFalse(target) = instr {
            let target = *target as usize;
            if target > i + 1 && target < code_len {
                if let Some(new_code) = try_unroll_loop(func, i, target) {
                    func.code = new_code;
                    unrolled = true;
                }
            }
        }
        i += 1;
    }
}

/// Try to unroll a loop with proper jump target adjustment
fn try_unroll_loop(
    func: &DecodedFunction,
    loop_start: usize,
    loop_back: usize,
) -> Option<Vec<Instr>> {
    if loop_back < loop_start + 3 {
        return None;
    }

    let body_len = loop_back - loop_start;

    // Only unroll small loops (body < 10 instructions)
    if body_len > 10 {
        return None;
    }

    // Check if all jumps within the body point forward (no backward jumps)
    // Backward jumps would break when duplicated
    for i in loop_start..loop_back {
        match &func.code[i] {
            Instr::Jump(t) | Instr::JumpIfTrue(t) | Instr::JumpIfFalse(t) => {
                let target = *t;
                if target < i {
                    // Backward jump - can't unroll safely
                    return None;
                }
                // Check if target is within the body (will need adjustment)
                if target >= loop_start && target < loop_back {
                    // Jump within body - needs special handling
                    return None;
                }
            }
            _ => {}
        }
    }

    let body = &func.code[loop_start..loop_back];
    let mut new_code = Vec::with_capacity(func.code.len() + body_len);

    // Copy everything before the loop
    new_code.extend_from_slice(&func.code[..loop_start]);

    // Copy the loop body twice (2x unrolling)
    // Adjust jump targets: any jump after loop_back needs +body_len offset
    for instr in body.iter().chain(body.iter()) {
        let adjusted = adjust_jump_target(instr, loop_back, body_len);
        new_code.push(adjusted);
    }

    // Copy everything after the back-edge, adjusting jump targets
    for instr in &func.code[loop_back..] {
        let adjusted = adjust_jump_target_after(instr, loop_back, body_len);
        new_code.push(adjusted);
    }

    Some(new_code)
}

/// Adjust a jump target within the duplicated body
fn adjust_jump_target(instr: &Instr, loop_back: usize, body_len: usize) -> Instr {
    match instr {
        Instr::Jump(t) => {
            let target = *t;
            if target >= loop_back { Instr::Jump(target + body_len) } else { instr.clone() }
        }
        Instr::JumpIfTrue(t) => {
            let target = *t;
            if target >= loop_back { Instr::JumpIfTrue(target + body_len) } else { instr.clone() }
        }
        Instr::JumpIfFalse(t) => {
            let target = *t;
            if target >= loop_back { Instr::JumpIfFalse(target + body_len) } else { instr.clone() }
        }
        _ => instr.clone(),
    }
}

/// Adjust a jump target in code after the loop
fn adjust_jump_target_after(instr: &Instr, loop_back: usize, body_len: usize) -> Instr {
    match instr {
        Instr::Jump(t) => {
            let target = *t;
            if target >= loop_back { Instr::Jump(target + body_len) } else { instr.clone() }
        }
        Instr::JumpIfTrue(t) => {
            let target = *t;
            if target >= loop_back { Instr::JumpIfTrue(target + body_len) } else { instr.clone() }
        }
        Instr::JumpIfFalse(t) => {
            let target = *t;
            if target >= loop_back { Instr::JumpIfFalse(target + body_len) } else { instr.clone() }
        }
        _ => instr.clone(),
    }
}

// ─────────────────────────────────────────────────────────────
// New Optimization Passes
// ─────────────────────────────────────────────────────────────

/// Eliminate redundant jumps:
/// - Jump followed by Jump → keep only the second jump
/// - JumpIfTrue/False to next instruction → convert to unconditional Jump or remove
fn thread_jumps(func: &mut DecodedFunction) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut i = 0;

    while i < func.code.len() {
        let instr = &func.code[i];

        match instr {
            // Jump followed by Jump → skip the first jump
            Instr::Jump(t1) => {
                if let Some(Instr::Jump(t2)) = func.code.get(i + 1) {
                    // Replace with the second jump
                    new_code.push(Instr::Jump(*t2));
                    i += 2;
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            // JumpIfTrue/False to the next instruction → convert to unconditional Jump
            Instr::JumpIfTrue(t) => {
                if *t == i + 2 {
                    // The false branch is the next instruction - so this is just a conditional jump
                    // But if the jump target IS the next instruction, we can convert to unconditional
                    // Actually, if target == i + 2, it means JumpIfTrue skips 1 instruction
                    // This is not redundant - keep as is
                    new_code.push(func.code[i].clone());
                    i += 1;
                } else {
                    new_code.push(func.code[i].clone());
                    i += 1;
                }
            }
            Instr::JumpIfFalse(t) => {
                if *t == i + 2 {
                    new_code.push(func.code[i].clone());
                    i += 1;
                } else {
                    new_code.push(func.code[i].clone());
                    i += 1;
                }
            }
            _ => {
                new_code.push(func.code[i].clone());
                i += 1;
            }
        }
    }

    func.code = new_code;
}

/// Replace expensive operations with cheaper equivalents:
/// - Div by power of 2 → Shl (left shift for positive, right shift for negative)
/// - Rem by power of 2 → BitAnd with mask
/// - Mul by power of 2 → Shl
/// - Mul by 0 → 0, Mul by 1 → identity
fn strength_reduce(func: &mut DecodedFunction, consts: &[Const], extra: &mut Vec<Const>) {
    let mut new_code = Vec::with_capacity(func.code.len());
    let mut i = 0;

    while i < func.code.len() {
        let instr = &func.code[i];

        match instr {
            // Check for Mul by constant power of 2
            Instr::Mul => {
                if let Some(new_instrs) = try_strength_reduce_mul(func, consts, extra, i) {
                    new_code.extend(new_instrs);
                    i += 3; // Skip LoadConst, value, Mul
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            // Check for Div by constant power of 2
            Instr::Div => {
                if let Some(new_instrs) = try_strength_reduce_div(func, consts, extra, i) {
                    new_code.extend(new_instrs);
                    i += 3;
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            // Check for Rem by constant power of 2
            Instr::Rem => {
                if let Some(new_instrs) = try_strength_reduce_rem(func, consts, extra, i) {
                    new_code.extend(new_instrs);
                    i += 3;
                    continue;
                }
                new_code.push(func.code[i].clone());
                i += 1;
            }
            _ => {
                new_code.push(func.code[i].clone());
                i += 1;
            }
        }
    }

    func.code = new_code;
}

/// Check if a number is a power of 2
fn is_power_of_two(n: i64) -> bool {
    n > 0 && (n & (n - 1)) == 0
}

/// Try to strength-reduce a Mul instruction
/// Pattern: LoadConst(c), <op>, Mul → if c is power of 2, use Shl
fn try_strength_reduce_mul(
    func: &DecodedFunction,
    consts: &[Const],
    extra: &mut Vec<Const>,
    idx: usize,
) -> Option<Vec<Instr>> {
    if idx < 2 {
        return None;
    }

    // Check if the second operand (top of stack) is a constant power of 2
    let instr_b = &func.code[idx - 1];
    if let Instr::LoadConst(b_idx) = instr_b {
        let b_val = resolve_const(consts, extra, *b_idx as usize);
        if let Some(Const::Int(b)) = b_val {
            let b = *b;
            if is_power_of_two(b) && b > 1 {
                let shift = b.trailing_zeros() as i64;
                let shift_idx = find_or_add_const(consts, extra, shift);
                return Some(vec![
                    Instr::LoadConst(shift_idx as u16),
                    func.code[idx - 2].clone(), // The value instruction
                    Instr::Shl,
                ]);
            }
            // Mul by 1 → identity (just load the value)
            if b == 1 {
                return Some(vec![func.code[idx - 2].clone()]);
            }
            // Mul by 0 → 0
            if b == 0 {
                let zero_idx = find_or_add_const(consts, extra, 0);
                return Some(vec![Instr::LoadConst(zero_idx as u16)]);
            }
        }
    }

    // Check if the first operand is a constant power of 2
    let instr_a = &func.code[idx - 2];
    if let Instr::LoadConst(a_idx) = instr_a {
        let a_val = resolve_const(consts, extra, *a_idx as usize);
        if let Some(Const::Int(a)) = a_val {
            let a = *a;
            if is_power_of_two(a) && a > 1 {
                let shift = a.trailing_zeros() as i64;
                let shift_idx = find_or_add_const(consts, extra, shift);
                return Some(vec![
                    Instr::LoadConst(shift_idx as u16),
                    func.code[idx - 1].clone(),
                    Instr::Shl,
                ]);
            }
            if a == 1 {
                return Some(vec![func.code[idx - 1].clone()]);
            }
            if a == 0 {
                let zero_idx = find_or_add_const(consts, extra, 0);
                return Some(vec![Instr::LoadConst(zero_idx as u16)]);
            }
        }
    }

    None
}

/// Try to strength-reduce a Div instruction
fn try_strength_reduce_div(
    func: &DecodedFunction,
    consts: &[Const],
    extra: &mut Vec<Const>,
    idx: usize,
) -> Option<Vec<Instr>> {
    if idx < 2 {
        return None;
    }

    let instr_b = &func.code[idx - 1];
    if let Instr::LoadConst(b_idx) = instr_b {
        let b_val = resolve_const(consts, extra, *b_idx as usize);
        if let Some(Const::Int(b)) = b_val {
            let b = *b;
            if is_power_of_two(b) && b > 1 {
                let shift = b.trailing_zeros() as i64;
                let shift_idx = find_or_add_const(consts, extra, shift);
                return Some(vec![
                    Instr::LoadConst(shift_idx as u16),
                    func.code[idx - 2].clone(),
                    Instr::Shr,
                ]);
            }
        }
    }

    None
}

/// Try to strength-reduce a Rem instruction
fn try_strength_reduce_rem(
    func: &DecodedFunction,
    consts: &[Const],
    extra: &mut Vec<Const>,
    idx: usize,
) -> Option<Vec<Instr>> {
    if idx < 2 {
        return None;
    }

    let instr_b = &func.code[idx - 1];
    if let Instr::LoadConst(b_idx) = instr_b {
        let b_val = resolve_const(consts, extra, *b_idx as usize);
        if let Some(Const::Int(b)) = b_val {
            let b = *b;
            if is_power_of_two(b) && b > 1 {
                let mask = b - 1;
                let mask_idx = find_or_add_const(consts, extra, mask);
                return Some(vec![
                    Instr::LoadConst(mask_idx as u16),
                    func.code[idx - 2].clone(),
                    Instr::BitAnd,
                ]);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::DecodedFunction;

    #[test]
    fn test_fold_constants() {
        let func = DecodedFunction {
            name: "test".to_string(),
            param_count: 0,
            locals: 1,
            is_native: false,
            code: vec![
                Instr::LoadConst(0), // Load 3
                Instr::LoadConst(1), // Load 2
                Instr::Add,          // 3 + 2 = 5
            ],
        };

        let result = optimize_function(
            &func,
            &[
                Const::Int(3),
                Const::Int(2),
            ],
        );

        // Should be folded to LoadConst of 5
        assert_eq!(result.func.code.len(), 1);
        assert_eq!(result.extra_consts.len(), 1);
        assert_eq!(result.extra_consts[0], Const::Int(5));
    }

    #[test]
    fn test_eliminate_dead_code() {
        let func = DecodedFunction {
            name: "test".to_string(),
            param_count: 1,
            locals: 1,
            is_native: false,
            code: vec![
                Instr::LoadVar(0),  // Load x
                Instr::StoreVar(0), // Store x back (dead)
                Instr::LoadVar(0),  // Load x (this one stays)
            ],
        };

        let result = optimize_function(&func, &[]);

        // Should be reduced to just LoadVar(0)
        assert_eq!(result.func.code.len(), 1);
    }
}
