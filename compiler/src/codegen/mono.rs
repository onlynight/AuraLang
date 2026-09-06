//! 泛型单态化（对应 P4.14，基础版）
//!
//! 由于字节码阶段类型已被擦除（MIR 不含类型信息），此处做**结构化单态化**：
//! - 识别带类型参数的泛型函数（如 `fun <T> id(x: T): T = x`）
//! - 按其调用点的**实参个数（arity）**分桶，为每个不同 arity 生成一份特化副本
//!   （命名为 `name#arity`，清空类型参数），并将对应调用点改写为特化名
//!
//! 该实现保证每个（泛型函数, arity）组合只生成一份副本，避免代码膨胀，并为
//! 未来基于语义类型的精确单态化预留结构。运行期语义不变。

use crate::codegen::hir::*;
use std::collections::{HashMap, HashSet};

/// 对 HIR 程序进行泛型单态化（原地修改）
pub fn mono_hir(hir: &mut HirProgram) {
    // 收集泛型函数：name -> 索引
    let generic: HashMap<String, usize> = hir
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_native && !f.type_params.is_empty())
        .map(|(i, f)| (f.name.clone(), i))
        .collect();

    if generic.is_empty() {
        return;
    }

    // 统计每个泛型函数被调用的 arity 组合
    let mut arities: HashMap<String, HashSet<usize>> = HashMap::new();
    for f in &hir.functions {
        if f.is_native {
            continue;
        }
        collect_call_arities(&f.body, &generic, &mut arities);
    }

    // 为每个 (泛型函数, arity) 生成特化副本
    let mut spec_map: HashMap<(String, usize), String> = HashMap::new();
    let mut new_funcs: Vec<HirFunction> = Vec::new();
    for (name, arset) in &arities {
        if let Some(&idx) = generic.get(name) {
            let orig = &hir.functions[idx];
            for &arity in arset {
                let spec_name = format!("{}#{}", name, arity);
                let mut spec = orig.clone();
                spec.name = spec_name.clone();
                spec.type_params.clear();
                // 参数数量与 arity 对齐（调用点实参数 == 形参数）
                while spec.params.len() < arity {
                    spec.params.push(HirParam {
                        name: format!("__p{}", spec.params.len()),
                        ty: None,
                    });
                }
                spec_map.insert((name.clone(), arity), spec_name.clone());
                new_funcs.push(spec);
            }
        }
    }

    // 改写调用点 + 追加特化函数
    for f in &mut hir.functions {
        if f.is_native {
            continue;
        }
        rewrite_calls(&mut f.body, &spec_map);
    }
    hir.functions.extend(new_funcs);
}

fn collect_call_arities(
    b: &HirBlock,
    generic: &HashMap<String, usize>,
    out: &mut HashMap<String, HashSet<usize>>,
) {
    for s in &b.stmts {
        match s {
            HirStmt::Val { init, .. } | HirStmt::Var { init, .. } => {
                if let Some(e) = init {
                    collect_expr(e, generic, out);
                }
            }
            HirStmt::Assign {
                target,
                value,
            } => {
                collect_expr(target, generic, out);
                collect_expr(value, generic, out);
            }
            HirStmt::Expr(e) => collect_expr(e, generic, out),
            HirStmt::Return(v) => {
                if let Some(e) = v {
                    collect_expr(e, generic, out);
                }
            }
            HirStmt::If {
                cond,
                then_b,
                else_b,
            } => {
                collect_expr(cond, generic, out);
                collect_call_arities(then_b, generic, out);
                if let Some(eb) = else_b {
                    collect_call_arities(eb, generic, out);
                }
            }
            HirStmt::While { cond, body } => {
                collect_expr(cond, generic, out);
                collect_call_arities(body, generic, out);
            }
            HirStmt::Block(bb) => collect_call_arities(bb, generic, out),
            _ => {}
        }
    }
}

fn collect_expr(
    e: &HirExpr,
    generic: &HashMap<String, usize>,
    out: &mut HashMap<String, HashSet<usize>>,
) {
    match e {
        HirExpr::Call {
            callee,
            args,
        } => {
            if generic.contains_key(callee) {
                out.entry(callee.clone()).or_default().insert(args.len());
            }
            for a in args {
                collect_expr(a, generic, out);
            }
        }
        HirExpr::Binary {
            lhs, rhs, ..
        } => {
            collect_expr(lhs, generic, out);
            collect_expr(rhs, generic, out);
        }
        HirExpr::Unary {
            operand, ..
        } => collect_expr(operand, generic, out),
        HirExpr::Member { object, .. } => collect_expr(object, generic, out),
        HirExpr::Index {
            container,
            index,
        } => {
            collect_expr(container, generic, out);
            collect_expr(index, generic, out);
        }
        HirExpr::New { args, .. } => {
            for a in args {
                collect_expr(a, generic, out);
            }
        }
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            collect_expr(cond, generic, out);
            collect_expr(then_e, generic, out);
            collect_expr(else_e, generic, out);
        }
        HirExpr::Block(b) => collect_call_arities(b, generic, out),
        _ => {}
    }
}

fn rewrite_calls(b: &mut HirBlock, spec_map: &HashMap<(String, usize), String>) {
    for s in &mut b.stmts {
        match s {
            HirStmt::Val { init, .. } | HirStmt::Var { init, .. } => {
                if let Some(e) = init {
                    rewrite_expr(e, spec_map);
                }
            }
            HirStmt::Assign {
                target,
                value,
            } => {
                rewrite_expr(target, spec_map);
                rewrite_expr(value, spec_map);
            }
            HirStmt::Expr(e) => rewrite_expr(e, spec_map),
            HirStmt::Return(v) => {
                if let Some(e) = v {
                    rewrite_expr(e, spec_map);
                }
            }
            HirStmt::If {
                cond,
                then_b,
                else_b,
            } => {
                rewrite_expr(cond, spec_map);
                rewrite_calls(then_b, spec_map);
                if let Some(eb) = else_b {
                    rewrite_calls(eb, spec_map);
                }
            }
            HirStmt::While { cond, body } => {
                rewrite_expr(cond, spec_map);
                rewrite_calls(body, spec_map);
            }
            HirStmt::Block(bb) => rewrite_calls(bb, spec_map),
            _ => {}
        }
    }
}

fn rewrite_expr(e: &mut HirExpr, spec_map: &HashMap<(String, usize), String>) {
    // 由于需在 Call 节点改写 callee，采用递归 + 就地替换
    match e {
        HirExpr::Call {
            callee,
            args,
        } => {
            for a in args.iter_mut() {
                rewrite_expr(a, spec_map);
            }
            if let Some(spec) = spec_map.get(&(callee.clone(), args.len())) {
                *callee = spec.clone();
            }
        }
        HirExpr::Binary {
            lhs, rhs, ..
        } => {
            rewrite_expr(lhs, spec_map);
            rewrite_expr(rhs, spec_map);
        }
        HirExpr::Unary {
            operand, ..
        } => rewrite_expr(operand, spec_map),
        HirExpr::Member { object, .. } => rewrite_expr(object, spec_map),
        HirExpr::Index {
            container,
            index,
        } => {
            rewrite_expr(container, spec_map);
            rewrite_expr(index, spec_map);
        }
        HirExpr::New { args, .. } => {
            for a in args.iter_mut() {
                rewrite_expr(a, spec_map);
            }
        }
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            rewrite_expr(cond, spec_map);
            rewrite_expr(then_e, spec_map);
            rewrite_expr(else_e, spec_map);
        }
        HirExpr::Block(b) => rewrite_calls(b, spec_map),
        _ => {}
    }
}
