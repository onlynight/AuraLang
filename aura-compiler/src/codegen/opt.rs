//! 优化通道（对应 P4.5–P4.9）
//!
//! - `fold_hir`     常量折叠（P4.5）：在 HIR 上折叠 `Lit op Lit`
//! - `inline_hir`   内联展开（P4.7）：将单表达式体的小函数内联到调用点
//! - `dce_mir`      死代码消除（P4.6）：移除不可达基本块与无用的纯计算指令
//! - `escape_mir`   逃逸分析（P4.8）：标注未逃逸的分配（可栈分配）
//! - `licm_mir`     循环优化（P4.9）：基本块级循环不变量外提（LICM）
//!
//! 所有优化均保持语义不变（逃逸分析仅做标注，不改变代码行为）。

use crate::ast::Literal;
use crate::codegen::hir::*;
use crate::codegen::mir::{BasicBlock, MirFunction, MirInstr, Terminator};
use std::collections::{HashMap, HashSet};

// ─────────────────────────────────────────────────────────────────────────────
// 常量折叠（P4.5）
// ─────────────────────────────────────────────────────────────────────────────

/// 对 HIR 程序进行常量折叠（原地修改）
pub fn fold_hir(hir: &mut HirProgram) {
    for f in &mut hir.functions {
        if !f.is_native {
            f.body = fold_block(&f.body);
        }
    }
}

fn fold_block(b: &HirBlock) -> HirBlock {
    HirBlock {
        stmts: b.stmts.iter().map(fold_stmt).collect(),
    }
}

fn fold_stmt(s: &HirStmt) -> HirStmt {
    match s {
        HirStmt::Val { name, ty, init } => HirStmt::Val {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(fold_expr),
        },
        HirStmt::Var { name, ty, init } => HirStmt::Var {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(fold_expr),
        },
        HirStmt::Assign { target, value } => HirStmt::Assign {
            target: fold_expr(target),
            value: fold_expr(value),
        },
        HirStmt::Expr(e) => HirStmt::Expr(fold_expr(e)),
        HirStmt::Return(v) => HirStmt::Return(v.as_ref().map(fold_expr)),
        HirStmt::If {
            cond,
            then_b,
            else_b,
        } => {
            let cond = fold_expr(cond);
            // `if (true) a else b` → a
            if let HirExpr::Lit(Literal::Bool(true)) = &cond {
                return HirStmt::Block(fold_block(then_b));
            }
            if let HirExpr::Lit(Literal::Bool(false)) = &cond {
                return match else_b {
                    Some(eb) => HirStmt::Block(fold_block(eb)),
                    None => HirStmt::Expr(HirExpr::Lit(Literal::Null)),
                };
            }
            HirStmt::If {
                cond,
                then_b: fold_block(then_b),
                else_b: else_b.as_ref().map(fold_block),
            }
        }
        HirStmt::While { cond, body } => HirStmt::While {
            cond: fold_expr(cond),
            body: fold_block(body),
        },
        HirStmt::Break => HirStmt::Break,
        HirStmt::Continue => HirStmt::Continue,
        HirStmt::Block(b) => HirStmt::Block(fold_block(b)),
    }
}

fn fold_expr(e: &HirExpr) -> HirExpr {
    match e {
        HirExpr::Binary { op, lhs, rhs } => {
            let l = fold_expr(lhs);
            let r = fold_expr(rhs);
            if let (HirExpr::Lit(a), HirExpr::Lit(b)) = (&l, &r) {
                if let Some(res) = eval_bin(*op, a, b) {
                    return HirExpr::Lit(res);
                }
            }
            HirExpr::Binary {
                op: *op,
                lhs: Box::new(l),
                rhs: Box::new(r),
            }
        }
        HirExpr::Unary { op, operand } => {
            let o = fold_expr(operand);
            if let HirExpr::Lit(v) = &o {
                if let Some(res) = eval_un(*op, v) {
                    return HirExpr::Lit(res);
                }
            }
            HirExpr::Unary {
                op: *op,
                operand: Box::new(o),
            }
        }
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            let c = fold_expr(cond);
            if let HirExpr::Lit(Literal::Bool(true)) = &c {
                return fold_expr(then_e);
            }
            if let HirExpr::Lit(Literal::Bool(false)) = &c {
                return fold_expr(else_e);
            }
            HirExpr::If {
                cond: Box::new(c),
                then_e: Box::new(fold_expr(then_e)),
                else_e: Box::new(fold_expr(else_e)),
            }
        }
        HirExpr::Call { callee, args } => HirExpr::Call {
            callee: callee.clone(),
            args: args.iter().map(fold_expr).collect(),
        },
        HirExpr::Member { object, name } => HirExpr::Member {
            object: Box::new(fold_expr(object)),
            name: name.clone(),
        },
        HirExpr::Index { container, index } => HirExpr::Index {
            container: Box::new(fold_expr(container)),
            index: Box::new(fold_expr(index)),
        },
        HirExpr::New { type_name, args } => HirExpr::New {
            type_name: type_name.clone(),
            args: args.iter().map(fold_expr).collect(),
        },
        HirExpr::Block(b) => HirExpr::Block(fold_block(b)),
        other => other.clone(),
    }
}

fn eval_bin(op: HirBinOp, a: &Literal, b: &Literal) -> Option<Literal> {
    use HirBinOp::*;
    match (op, a, b) {
        (Add, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x + y)),
        (Sub, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x - y)),
        (Mul, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x * y)),
        (Div, Literal::Int(x), Literal::Int(y)) if *y != 0 => Some(Literal::Int(x / y)),
        (Rem, Literal::Int(x), Literal::Int(y)) if *y != 0 => Some(Literal::Int(x % y)),
        (Add, Literal::Float(x), Literal::Float(y)) => Some(Literal::Float(x + y)),
        (Sub, Literal::Float(x), Literal::Float(y)) => Some(Literal::Float(x - y)),
        (Mul, Literal::Float(x), Literal::Float(y)) => Some(Literal::Float(x * y)),
        (Div, Literal::Float(x), Literal::Float(y)) if *y != 0.0 => Some(Literal::Float(x / y)),
        (Eq, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x == y)),
        (Ne, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x != y)),
        (Lt, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x < y)),
        (Gt, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x > y)),
        (Le, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x <= y)),
        (Ge, Literal::Int(x), Literal::Int(y)) => Some(Literal::Bool(x >= y)),
        (Eq, Literal::Bool(x), Literal::Bool(y)) => Some(Literal::Bool(x == y)),
        (And, Literal::Bool(x), Literal::Bool(y)) => Some(Literal::Bool(*x && *y)),
        (Or, Literal::Bool(x), Literal::Bool(y)) => Some(Literal::Bool(*x || *y)),
        (BitAnd, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x & y)),
        (BitOr, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x | y)),
        (BitXor, Literal::Int(x), Literal::Int(y)) => Some(Literal::Int(x ^ y)),
        (To, _, y) => Some(y.clone()),
        _ => None,
    }
}

fn eval_un(op: HirUnOp, v: &Literal) -> Option<Literal> {
    match (op, v) {
        (HirUnOp::Minus, Literal::Int(x)) => Some(Literal::Int(-*x)),
        (HirUnOp::Minus, Literal::Float(x)) => Some(Literal::Float(-*x)),
        (HirUnOp::Not, Literal::Bool(b)) => Some(Literal::Bool(!*b)),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 内联展开（P4.7）
// ─────────────────────────────────────────────────────────────────────────────

/// 对 HIR 程序进行内联（原地修改）
pub fn inline_hir(hir: &mut HirProgram) {
    // 收集内联候选：单表达式体（或单 Return(Some(expr))）的非递归函数
    let mut candidates: HashMap<String, (Vec<String>, HirExpr)> = HashMap::new();
    for f in &hir.functions {
        if f.is_native || f.type_params.len() > 0 {
            continue;
        }
        let body_expr = match &f.body.stmts[..] {
            [HirStmt::Return(Some(e))] => Some(e.clone()),
            [HirStmt::Expr(e)] => Some(e.clone()),
            _ => None,
        };
        if let Some(be) = body_expr {
            // 跳过递归（body 中引用自身名）
            if expr_references(&be, &f.name) {
                continue;
            }
            let params: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
            candidates.insert(f.name.clone(), (params, be));
        }
    }

    for f in &mut hir.functions {
        if f.is_native {
            continue;
        }
        let new_body = inline_block(&f.body, &candidates);
        f.body = new_body;
    }
}

fn expr_references(e: &HirExpr, name: &str) -> bool {
    match e {
        HirExpr::Var(n) => n == name,
        HirExpr::Binary { lhs, rhs, .. } => expr_references(lhs, name) || expr_references(rhs, name),
        HirExpr::Unary { operand, .. } => expr_references(operand, name),
        HirExpr::Call { callee, args } => {
            callee == name || args.iter().any(|a| expr_references(a, name))
        }
        HirExpr::Member { object, .. } => expr_references(object, name),
        HirExpr::Index { container, index } => {
            expr_references(container, name) || expr_references(index, name)
        }
        HirExpr::New { args, .. } => args.iter().any(|a| expr_references(a, name)),
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            expr_references(cond, name)
                || expr_references(then_e, name)
                || expr_references(else_e, name)
        }
        HirExpr::Block(b) => b.stmts.iter().any(|s| stmt_references(s, name)),
        _ => false,
    }
}

fn stmt_references(s: &HirStmt, name: &str) -> bool {
    match s {
        HirStmt::Val { init, .. } | HirStmt::Var { init, .. } => {
            init.as_ref().map(|e| expr_references(e, name)).unwrap_or(false)
        }
        HirStmt::Assign { target, value } => {
            expr_references(target, name) || expr_references(value, name)
        }
        HirStmt::Expr(e) => expr_references(e, name),
        HirStmt::Return(v) => v.as_ref().map(|e| expr_references(e, name)).unwrap_or(false),
        HirStmt::If { cond, then_b, else_b } => {
            expr_references(cond, name)
                || then_b.stmts.iter().any(|s| stmt_references(s, name))
                || else_b
                    .as_ref()
                    .map(|b| b.stmts.iter().any(|s| stmt_references(s, name)))
                    .unwrap_or(false)
        }
        HirStmt::While { cond, body } => {
            expr_references(cond, name)
                || body.stmts.iter().any(|s| stmt_references(s, name))
        }
        _ => false,
    }
}

fn inline_block(b: &HirBlock, cand: &HashMap<String, (Vec<String>, HirExpr)>) -> HirBlock {
    HirBlock {
        stmts: b.stmts.iter().map(|s| inline_stmt(s, cand)).collect(),
    }
}

fn inline_stmt(s: &HirStmt, cand: &HashMap<String, (Vec<String>, HirExpr)>) -> HirStmt {
    match s {
        HirStmt::Val { name, ty, init } => HirStmt::Val {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(|e| inline_expr(e, cand)),
        },
        HirStmt::Var { name, ty, init } => HirStmt::Var {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(|e| inline_expr(e, cand)),
        },
        HirStmt::Assign { target, value } => HirStmt::Assign {
            target: inline_expr(target, cand),
            value: inline_expr(value, cand),
        },
        HirStmt::Expr(e) => HirStmt::Expr(inline_expr(e, cand)),
        HirStmt::Return(v) => HirStmt::Return(v.as_ref().map(|e| inline_expr(e, cand))),
        HirStmt::If {
            cond,
            then_b,
            else_b,
        } => HirStmt::If {
            cond: inline_expr(cond, cand),
            then_b: inline_block(then_b, cand),
            else_b: else_b.as_ref().map(|b| inline_block(b, cand)),
        },
        HirStmt::While { cond, body } => HirStmt::While {
            cond: inline_expr(cond, cand),
            body: inline_block(body, cand),
        },
        HirStmt::Block(b) => HirStmt::Block(inline_block(b, cand)),
        other => other.clone(),
    }
}

fn inline_expr(e: &HirExpr, cand: &HashMap<String, (Vec<String>, HirExpr)>) -> HirExpr {
    match e {
        HirExpr::Call { callee, args } => {
            let new_args: Vec<HirExpr> = args.iter().map(|a| inline_expr(a, cand)).collect();
            if let Some((params, body)) = cand.get(callee) {
                if params.len() == new_args.len() {
                    let mapping: Vec<(String, HirExpr)> =
                        params.iter().cloned().zip(new_args.iter().cloned()).collect();
                    return subst_expr(body, &mapping);
                }
            }
            HirExpr::Call {
                callee: callee.clone(),
                args: new_args,
            }
        }
        HirExpr::Binary { op, lhs, rhs } => HirExpr::Binary {
            op: *op,
            lhs: Box::new(inline_expr(lhs, cand)),
            rhs: Box::new(inline_expr(rhs, cand)),
        },
        HirExpr::Unary { op, operand } => HirExpr::Unary {
            op: *op,
            operand: Box::new(inline_expr(operand, cand)),
        },
        HirExpr::Member { object, name } => HirExpr::Member {
            object: Box::new(inline_expr(object, cand)),
            name: name.clone(),
        },
        HirExpr::Index { container, index } => HirExpr::Index {
            container: Box::new(inline_expr(container, cand)),
            index: Box::new(inline_expr(index, cand)),
        },
        HirExpr::New { type_name, args } => HirExpr::New {
            type_name: type_name.clone(),
            args: args.iter().map(|a| inline_expr(a, cand)).collect(),
        },
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => HirExpr::If {
            cond: Box::new(inline_expr(cond, cand)),
            then_e: Box::new(inline_expr(then_e, cand)),
            else_e: Box::new(inline_expr(else_e, cand)),
        },
        HirExpr::Block(b) => HirExpr::Block(inline_block(b, cand)),
        other => other.clone(),
    }
}

/// 在表达式中将参数名替换为实参
fn subst_expr(e: &HirExpr, mapping: &[(String, HirExpr)]) -> HirExpr {
    match e {
        HirExpr::Var(n) => {
            for (p, a) in mapping {
                if p == n {
                    return a.clone();
                }
            }
            e.clone()
        }
        HirExpr::Binary { op, lhs, rhs } => HirExpr::Binary {
            op: *op,
            lhs: Box::new(subst_expr(lhs, mapping)),
            rhs: Box::new(subst_expr(rhs, mapping)),
        },
        HirExpr::Unary { op, operand } => HirExpr::Unary {
            op: *op,
            operand: Box::new(subst_expr(operand, mapping)),
        },
        HirExpr::Call { callee, args } => HirExpr::Call {
            callee: callee.clone(),
            args: args.iter().map(|a| subst_expr(a, mapping)).collect(),
        },
        HirExpr::Member { object, name } => HirExpr::Member {
            object: Box::new(subst_expr(object, mapping)),
            name: name.clone(),
        },
        HirExpr::Index { container, index } => HirExpr::Index {
            container: Box::new(subst_expr(container, mapping)),
            index: Box::new(subst_expr(index, mapping)),
        },
        HirExpr::New { type_name, args } => HirExpr::New {
            type_name: type_name.clone(),
            args: args.iter().map(|a| subst_expr(a, mapping)).collect(),
        },
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => HirExpr::If {
            cond: Box::new(subst_expr(cond, mapping)),
            then_e: Box::new(subst_expr(then_e, mapping)),
            else_e: Box::new(subst_expr(else_e, mapping)),
        },
        HirExpr::Block(b) => HirExpr::Block(HirBlock {
            stmts: b.stmts.iter().map(|s| subst_stmt(s, mapping)).collect(),
        }),
        other => other.clone(),
    }
}

fn subst_stmt(s: &HirStmt, mapping: &[(String, HirExpr)]) -> HirStmt {
    match s {
        HirStmt::Val { name, ty, init } => HirStmt::Val {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(|e| subst_expr(e, mapping)),
        },
        HirStmt::Var { name, ty, init } => HirStmt::Var {
            name: name.clone(),
            ty: ty.clone(),
            init: init.as_ref().map(|e| subst_expr(e, mapping)),
        },
        HirStmt::Assign { target, value } => HirStmt::Assign {
            target: subst_expr(target, mapping),
            value: subst_expr(value, mapping),
        },
        HirStmt::Expr(e) => HirStmt::Expr(subst_expr(e, mapping)),
        HirStmt::Return(v) => HirStmt::Return(v.as_ref().map(|e| subst_expr(e, mapping))),
        HirStmt::If {
            cond,
            then_b,
            else_b,
        } => HirStmt::If {
            cond: subst_expr(cond, mapping),
            then_b: HirBlock {
                stmts: then_b.stmts.iter().map(|s| subst_stmt(s, mapping)).collect(),
            },
            else_b: else_b.as_ref().map(|b| HirBlock {
                stmts: b.stmts.iter().map(|s| subst_stmt(s, mapping)).collect(),
            }),
        },
        HirStmt::While { cond, body } => HirStmt::While {
            cond: subst_expr(cond, mapping),
            body: HirBlock {
                stmts: body.stmts.iter().map(|s| subst_stmt(s, mapping)).collect(),
            },
        },
        other => other.clone(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 死代码消除（P4.6）
// ─────────────────────────────────────────────────────────────────────────────

/// 对 MIR 函数列表进行死代码消除（原地修改）
pub fn dce_mir(funcs: &mut [MirFunction]) {
    for f in funcs.iter_mut() {
        if f.is_native {
            continue;
        }
        // 1. 计算从入口块可达的基本块
        let reach = reachable(f);
        let mut id_map: Vec<Option<usize>> = vec![None; f.blocks.len()];
        let mut new_blocks: Vec<BasicBlock> = Vec::new();
        for (old_id, b) in f.blocks.iter().enumerate() {
            if !reach.contains(&old_id) {
                continue;
            }
            let new_id = new_blocks.len();
            id_map[old_id] = Some(new_id);
            new_blocks.push(BasicBlock {
                id: new_id,
                instrs: b.instrs.clone(),
                term: remap_term(&b.term, &id_map),
            });
        }
        f.blocks = new_blocks;

        // 2. 删除结果寄存器未被使用的纯计算指令
        let used = used_regs(f);
        for b in &mut f.blocks {
            b.instrs.retain(|instr| {
                if let Some(dst) = pure_dst(instr) {
                    used.contains(&dst)
                } else {
                    true // 有副作用或控制相关，保留
                }
            });
        }
    }
}

fn reachable(f: &MirFunction) -> HashSet<usize> {
    let mut seen = HashSet::new();
    let mut stack = vec![0usize];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        match &f.blocks[id].term {
            Terminator::Goto(t) => stack.push(*t),
            Terminator::If { then_b, else_b, .. } => {
                stack.push(*then_b);
                stack.push(*else_b);
            }
            Terminator::Return(_) | Terminator::ReturnVoid => {}
        }
    }
    seen
}

fn remap_term(t: &Terminator, id_map: &[Option<usize>]) -> Terminator {
    let m = |i: usize| id_map[i].unwrap_or(0);
    match t {
        Terminator::Goto(t) => Terminator::Goto(m(*t)),
        Terminator::If {
            cond,
            then_b,
            else_b,
        } => Terminator::If {
            cond: *cond,
            then_b: m(*then_b),
            else_b: m(*else_b),
        },
        Terminator::Return(r) => Terminator::Return(*r),
        Terminator::ReturnVoid => Terminator::ReturnVoid,
    }
}

/// 返回指令写入的寄存器（若有），用于判断是否为"纯计算"
fn pure_dst(instr: &MirInstr) -> Option<usize> {
    match instr {
        MirInstr::LoadConst { dst, .. }
        | MirInstr::LoadLocal { dst, .. }
        | MirInstr::BinOp { dst, .. }
        | MirInstr::UnOp { dst, .. }
        | MirInstr::Alloc { dst, .. }
        | MirInstr::GetField { dst, .. } => Some(*dst),
        _ => None,
    }
}

/// 收集所有被"读取"的寄存器（用于 DCE 用途分析）
fn used_regs(f: &MirFunction) -> HashSet<usize> {
    let mut used = HashSet::new();
    for b in &f.blocks {
        for instr in &b.instrs {
            match instr {
                MirInstr::BinOp { a, b: rb, .. } => {
                    used.insert(*a);
                    used.insert(*rb);
                }
                MirInstr::UnOp { a, .. } => {
                    used.insert(*a);
                }
                MirInstr::Call { args, .. } | MirInstr::CallNative { args, .. } => {
                    for a in args {
                        used.insert(*a);
                    }
                }
                MirInstr::StoreLocal { src, .. } => {
                    used.insert(*src);
                }
                MirInstr::SetField { obj, src, .. } => {
                    used.insert(*obj);
                    used.insert(*src);
                }
                MirInstr::GetField { obj, .. } => {
                    used.insert(*obj);
                }
                _ => {}
            }
        }
        match &b.term {
            Terminator::If { cond, .. } => {
                used.insert(*cond);
            }
            Terminator::Return(r) => {
                used.insert(*r);
            }
            _ => {}
        }
    }
    used
}

// ─────────────────────────────────────────────────────────────────────────────
// 逃逸分析（P4.8）
// ─────────────────────────────────────────────────────────────────────────────

/// 返回每个函数中"逃逸"的分配寄存器集合（逃逸 = 作为调用参数 / 写入字段 / 被返回）
pub fn escape_mir(funcs: &[MirFunction]) -> HashMap<String, HashSet<usize>> {
    let mut result = HashMap::new();
    for f in funcs {
        if f.is_native {
            continue;
        }
        let mut escaping = HashSet::new();
        for b in &f.blocks {
            for instr in &b.instrs {
                match instr {
                    MirInstr::Call { args, .. } | MirInstr::CallNative { args, .. } => {
                        for a in args {
                            escaping.insert(*a);
                        }
                    }
                    MirInstr::SetField { src, .. } => {
                        escaping.insert(*src);
                    }
                    _ => {}
                }
            }
            if let Terminator::Return(r) = &b.term {
                escaping.insert(*r);
            }
        }
        result.insert(f.name.clone(), escaping);
    }
    result
}

// ─────────────────────────────────────────────────────────────────────────────
// 循环不变量外提（P4.9，基础版 LICM）
// ─────────────────────────────────────────────────────────────────────────────

/// 对 MIR 函数列表做基础循环不变量外提（原地修改）
pub fn licm_mir(funcs: &mut [MirFunction]) {
    for f in funcs.iter_mut() {
        if f.is_native {
            continue;
        }
        let param_count = f.param_slots.len();
        // 找出回边目标（被更高 id 的块跳转回的块 = 循环头）
        let mut headers: HashSet<usize> = HashSet::new();
        for b in &f.blocks {
            for succ in term_succs(&b.term) {
                if succ < b.id {
                    headers.insert(succ);
                }
            }
        }
        for h in headers {
            hoist_header(f, h, param_count);
        }
    }
}

fn term_succs(t: &Terminator) -> Vec<usize> {
    match t {
        Terminator::Goto(x) => vec![*x],
        Terminator::If { then_b, else_b, .. } => vec![*then_b, *else_b],
        _ => vec![],
    }
}

/// 将循环头块开头"纯且循环不变量"的指令外提到新建的前置块
fn hoist_header(f: &mut MirFunction, header: usize, param_count: usize) {
    if header >= f.blocks.len() {
        return;
    }
    let mut hoisted: Vec<MirInstr> = Vec::new();
    let mut remaining: Vec<MirInstr> = Vec::new();
    for instr in f.blocks[header].instrs.clone() {
        if is_invariant(&instr, &hoisted, param_count) {
            hoisted.push(instr);
        } else {
            remaining.push(instr);
        }
    }
    if hoisted.is_empty() {
        return;
    }
    f.blocks[header].instrs = remaining;

    // 新建前置块：跳转到 header，原前驱改为跳向前置块
    let pre_id = f.blocks.len();
    let mut preds: Vec<usize> = Vec::new();
    for (id, b) in f.blocks.iter().enumerate() {
        if term_succs(&b.term).contains(&header) {
            preds.push(id);
        }
    }
    // 前置块：先执行 hoisted，再 Goto(header)
    let pre = BasicBlock {
        id: pre_id,
        instrs: hoisted,
        term: Terminator::Goto(header),
    };
    f.blocks.push(pre);
    for p in preds {
        f.blocks[p].term = redirect(&f.blocks[p].term, header, pre_id);
    }
}

fn is_invariant(instr: &MirInstr, hoisted: &[MirInstr], param_count: usize) -> bool {
    match instr {
        MirInstr::LoadConst { .. } => true,
        MirInstr::LoadLocal { slot, .. } => *slot < param_count,
        MirInstr::BinOp { a, b, .. } => {
            reg_invariant(*a, hoisted, param_count) && reg_invariant(*b, hoisted, param_count)
        }
        MirInstr::UnOp { a, .. } => reg_invariant(*a, hoisted, param_count),
        _ => false,
    }
}

fn reg_invariant(reg: usize, hoisted: &[MirInstr], param_count: usize) -> bool {
    if reg < param_count {
        return true;
    }
    hoisted.iter().any(|i| match i {
        MirInstr::LoadConst { dst, .. }
        | MirInstr::LoadLocal { dst, .. }
        | MirInstr::BinOp { dst, .. }
        | MirInstr::UnOp { dst, .. } => *dst == reg,
        _ => false,
    })
}

fn redirect(t: &Terminator, from: usize, to: usize) -> Terminator {
    match t {
        Terminator::Goto(x) if *x == from => Terminator::Goto(to),
        Terminator::If {
            cond,
            then_b,
            else_b,
        } => Terminator::If {
            cond: *cond,
            then_b: if *then_b == from { to } else { *then_b },
            else_b: if *else_b == from { to } else { *else_b },
        },
        other => other.clone(),
    }
}
