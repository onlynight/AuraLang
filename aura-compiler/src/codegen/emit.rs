//! MIR → 字节码发射（对应 技术方案 §5.2 / §7.1）
//!
//! 将寄存器式 MIR 线性化为栈式字节码：
//! - 每个 `BasicBlock` 顺序发射，记录各块起始字节偏移
//! - 寄存器即为局部变量槽，`LoadLocal`/`StoreLocal` 映射为 `LoadVar`/`StoreVar`
//! - 控制流终结指令映射为 `Jump`/`JumpIfTrue`/`JumpIfFalse`，目标为绝对字节偏移
//! - 普通块顺序：条件块 → then 块（顺序落入）→ else 块 → merge 块

use crate::codegen::hir::HirProgram;
use crate::codegen::mir::{LowerCtx, MirFunction, Terminator};
use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const, OpCode};
use std::collections::HashMap;

/// 将 MIR 函数列表发射为字节码模块
pub fn emit_module(hir: &HirProgram, mir_funcs: &[MirFunction], ctx: &LowerCtx) -> BytecodeModule {
    // 原生函数表
    let natives: Vec<BytecodeNative> = hir
        .natives
        .iter()
        .map(|n| BytecodeNative {
            name: n.name.clone(),
            param_count: n.params.len() as u16,
        })
        .collect();

    // 用户函数名 -> 索引
    let mut fn_index: HashMap<&str, u16> = HashMap::new();
    for (i, f) in mir_funcs.iter().enumerate() {
        fn_index.insert(f.name.as_str(), i as u16);
    }
    // 原生函数名 -> 索引
    let mut native_index: HashMap<&str, u16> = HashMap::new();
    for (i, n) in natives.iter().enumerate() {
        native_index.insert(n.name.as_str(), i as u16);
    }

    let mut functions = Vec::new();
    for f in mir_funcs {
        let code = emit_function(f, &fn_index, &native_index);
        functions.push(BytecodeFunction {
            name: f.name.clone(),
            param_count: f.param_slots.len() as u16,
            locals: f.reg_count as u16,
            is_native: false,
            code,
        });
    }

    let entry = fn_index.get("main").copied().unwrap_or(0);

    // P8.1: 合并 FFI 常量到常量池
    let mut consts = ctx.consts.clone();
    for (_name, c) in &hir.constants {
        consts.push(c.clone());
    }

    BytecodeModule {
        consts,
        natives,
        functions,
        entry,
    }
}

fn emit_function(
    f: &MirFunction,
    fn_index: &HashMap<&str, u16>,
    native_index: &HashMap<&str, u16>,
) -> Vec<u8> {
    // 基本块布局：保证每个 `If` 的 `then` 块紧跟其条件块之后，
    // 这样 `JumpIfFalse(else)` 后自然 fallthrough 到 `then`，与块的物理创建顺序无关。
    let order = layout_blocks(f);

    // 第一遍：计算每个块的起始字节偏移
    let mut block_offsets = vec![0usize; f.blocks.len()];
    let mut off = 0usize;
    for &bid in &order {
        let b = &f.blocks[bid];
        block_offsets[bid] = off;
        for instr in &b.instrs {
            off += instr_size(instr);
        }
        off += term_size(&b.term);
    }

    // 第二遍：发射
    let mut code = Vec::with_capacity(off);
    for &bid in &order {
        let b = &f.blocks[bid];
        for instr in &b.instrs {
            emit_instr(&mut code, instr, fn_index, native_index);
        }
        match &b.term {
            Terminator::Goto(target) => {
                OpCode::Jump(block_offsets[*target] as i32).write(&mut code);
            }
            Terminator::If {
                cond,
                then_b,
                else_b,
            } => {
                // 顺序落入 then 块：先 JumpIfFalse(else)，then 自然顺序执行
                OpCode::LoadVar(*cond as u16).write(&mut code);
                OpCode::JumpIfFalse(block_offsets[*else_b] as i32).write(&mut code);
                // then 块顺序落入；其末尾 Goto(merge)
                let _ = then_b;
            }
            Terminator::Return(reg) => {
                OpCode::LoadVar(*reg as u16).write(&mut code);
                OpCode::Return.write(&mut code);
            }
            Terminator::ReturnVoid => {
                OpCode::ReturnUnit.write(&mut code);
            }
        }
    }
    code
}

/// 计算基本块的发射顺序，使得每个 `If` 的 `then` 块紧邻其条件块之后。
///
/// 理由：字节码发射时 `Terminator::If` 依赖「顺序落入 then」的 fallthrough 语义，
/// 而 MIR 降级（尤其是嵌套控制流 / LICM 插入前置块）并不保证块在数组中的物理顺序
/// 与 fallthrough 一致。`emit` 仅使用绝对字节偏移作为跳转目标，因此只要
/// then 块在 `order` 中紧随其条件块，`JumpIfFalse(else)` 的 fallthrough 即正确。
fn layout_blocks(f: &MirFunction) -> Vec<usize> {
    let n = f.blocks.len();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut pending: Vec<usize> = vec![0]; // 待处理的 trace 起点（如 if 的 else 分支）
    while let Some(start) = pending.pop() {
        if visited[start] {
            continue;
        }
        // 沿 trace 向下：If 优先走 then（fallthrough），else 压入 pending 稍后处理；
        // Goto 顺次前进；Return/已访问块停止当前 trace。
        let mut cur = start;
        while !visited[cur] {
            visited[cur] = true;
            order.push(cur);
            match &f.blocks[cur].term {
                Terminator::If { then_b, else_b, .. } => {
                    pending.push(*else_b);
                    cur = *then_b;
                }
                Terminator::Goto(t) => cur = *t,
                Terminator::Return(_) | Terminator::ReturnVoid => break,
            }
        }
    }
    // 兜底：理论上所有块均从入口可达；若有遗漏按 id 顺序补齐
    for i in 0..n {
        if !visited[i] {
            order.push(i);
        }
    }
    order
}

/// 单条 MIR 指令发射为字节码后的精确字节数（必须与 `emit_instr` 完全一致）
fn instr_size(instr: &crate::codegen::mir::MirInstr) -> usize {
    use crate::codegen::mir::MirInstr::*;
    match instr {
        // LoadConst/LoadVar/StoreVar 各 3 字节（操作码 + u16 操作数）
        LoadConst { .. } => 6,
        LoadLocal { .. } => 6,
        StoreLocal { .. } => 6,
        // BinOp：LoadVar(a) + LoadVar(b) + 算术(1) + StoreVar(dst) = 10；`To` 仅 LoadVar(b)+StoreVar = 6
        BinOp { op, .. } => {
            if *op == crate::codegen::hir::HirBinOp::To {
                6
            } else {
                10
            }
        }
        // UnOp：LoadVar(a) + op(1) + StoreVar(dst) = 7
        UnOp { .. } => 7,
        // Call：每个参数 LoadVar(3) + Call(3) + 可选 StoreVar(3)
        Call { args, dst, .. } => 3 * args.len() + 3 + if dst.is_some() { 3 } else { 0 },
        CallNative { args, dst, .. } => 3 * args.len() + 3 + if dst.is_some() { 3 } else { 0 },
        // Alloc：NewObject(3) + StoreVar(3) = 6
        Alloc { .. } => 6,
        // GetField：LoadVar(obj) + GetField(3) + StoreVar(3) = 9
        GetField { .. } => 9,
        // SetField：LoadVar(src) + LoadVar(obj) + SetField(3) = 9
        SetField { .. } => 9,
        // GetIndex：LoadVar(obj) + LoadVar(idx) + GetIndex(1) + StoreVar(3) = 10
        GetIndex { .. } => 10,
        // SetIndex：LoadVar(src) + LoadVar(obj) + LoadVar(idx) + SetIndex(1) = 10
        SetIndex { .. } => 10,
        // Retain：LoadVar(src) + Retain(1) = 4
        Retain { .. } => 4,
        // Release：LoadVar(src) + Release(1) = 4
        Release { .. } => 4,
        // WeakRef：LoadVar(src) + WeakRef(1) + StoreVar(dst) = 7
        WeakRef { .. } => 7,
        // WeakGet：LoadVar(src) + WeakGet(1) + StoreVar(dst) = 7
        WeakGet { .. } => 7,
        // Box：LoadVar(src) + BoxAlloc(1) + StoreVar(dst) = 7
        Box { .. } => 7,
        // MakeCallback：MakeCallback(3) + StoreVar(3) = 6
        MakeCallback { .. } => 6,
        // DeferBegin：DeferBegin(1) = 1
        DeferBegin => 1,
        // DeferEnd：DeferEnd(1) = 1
        DeferEnd => 1,
        // Yield：Yield(1) = 1
        Yield => 1,
    }
}

/// 终结指令发射为字节码后的精确字节数（必须与 `emit_function` 一致）
fn term_size(t: &Terminator) -> usize {
    match t {
        // Goto → JUMP (1 + i32 = 5)
        Terminator::Goto(_) => 5,
        // If → LoadVar(cond)(3) + JUMP_IF_FALSE(5) = 8
        Terminator::If { .. } => 8,
        // Return → LoadVar(3) + RETURN(1) = 4
        Terminator::Return(_) => 4,
        // ReturnVoid → RETURN_UNIT(1)
        Terminator::ReturnVoid => 1,
    }
}

fn emit_instr(
    code: &mut Vec<u8>,
    instr: &crate::codegen::mir::MirInstr,
    fn_index: &HashMap<&str, u16>,
    native_index: &HashMap<&str, u16>,
) {
    use crate::codegen::hir::HirBinOp::*;
    use crate::codegen::mir::MirInstr::*;
    match instr {
        LoadConst { dst, ci } => {
            OpCode::LoadConst(*ci as u16).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        LoadLocal { dst, slot } => {
            OpCode::LoadVar(*slot as u16).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        StoreLocal { slot, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::StoreVar(*slot as u16).write(code);
        }
        BinOp { dst, op, a, b } => {
            if *op == To {
                // 近似：不发射运算，仅把 b 当作结果
                OpCode::LoadVar(*b as u16).write(code);
                OpCode::StoreVar(*dst as u16).write(code);
                return;
            }
            OpCode::LoadVar(*a as u16).write(code);
            OpCode::LoadVar(*b as u16).write(code);
            let oc = match op {
                Add => OpCode::Add,
                Sub => OpCode::Sub,
                Mul => OpCode::Mul,
                Div => OpCode::Div,
                Rem => OpCode::Rem,
                Eq => OpCode::Eq,
                Ne => OpCode::Ne,
                Lt => OpCode::Lt,
                Gt => OpCode::Gt,
                Le => OpCode::Le,
                Ge => OpCode::Ge,
                And => OpCode::And,
                Or => OpCode::Or,
                BitAnd => OpCode::BitAnd,
                BitOr => OpCode::BitOr,
                BitXor => OpCode::BitXor,
                Shl => OpCode::Shl,
                Shr => OpCode::Shr,
                To => OpCode::Add, // 不会到达
            };
            oc.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        UnOp { dst, op, a } => {
            OpCode::LoadVar(*a as u16).write(code);
            match op {
                crate::codegen::hir::HirUnOp::Minus => OpCode::Neg,
                crate::codegen::hir::HirUnOp::Not => OpCode::Not,
            }
            .write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        Call { dst, func, args } => {
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            let idx = fn_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::Call(idx).write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        CallNative { dst, func, args } => {
            for a in args {
                OpCode::LoadVar(*a as u16).write(code);
            }
            let idx = native_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::CallNative(idx).write(code);
            if let Some(d) = dst {
                OpCode::StoreVar(*d as u16).write(code);
            }
        }
        Alloc { dst, type_name } => {
            let idx = type_index(type_name);
            OpCode::NewObject(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        GetField { dst, obj, field } => {
            OpCode::LoadVar(*obj as u16).write(code);
            let idx = field_index(field);
            OpCode::GetField(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        SetField { obj, field, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::LoadVar(*obj as u16).write(code);
            let idx = field_index(field);
            OpCode::SetField(idx).write(code);
        }
        GetIndex { dst, obj, idx } => {
            OpCode::LoadVar(*obj as u16).write(code);
            OpCode::LoadVar(*idx as u16).write(code);
            OpCode::GetIndex.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        SetIndex { obj, idx, src } => {
            // 栈布局：值在下、索引在顶（与 SetField 的「值、对象」顺序一致扩展）
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::LoadVar(*obj as u16).write(code);
            OpCode::LoadVar(*idx as u16).write(code);
            OpCode::SetIndex.write(code);
        }
        Retain { src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::Retain.write(code);
        }
        Release { src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::Release.write(code);
        }
        WeakRef { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::WeakRef.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        WeakGet { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::WeakGet.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        Box { dst, src } => {
            OpCode::LoadVar(*src as u16).write(code);
            OpCode::BoxAlloc.write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        MakeCallback { dst, func } => {
            let idx = fn_index.get(func.as_str()).copied().unwrap_or(0);
            OpCode::MakeCallback(idx).write(code);
            OpCode::StoreVar(*dst as u16).write(code);
        }
        DeferBegin => {
            OpCode::DeferBegin.write(code);
        }
        DeferEnd => {
            OpCode::DeferEnd.write(code);
        }
        Yield => {
            OpCode::Yield.write(code);
        }
    }
}

/// 类型名 -> 类型表索引（简化：字符串哈希）
fn type_index(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}

/// 字段名 -> 字段表索引（简化：字符串哈希）
fn field_index(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}

/// 工具：常量池查找（供优化器/反汇编器复用）
pub fn find_const<'a>(module: &'a BytecodeModule, idx: usize) -> &'a Const {
    &module.consts[idx]
}
