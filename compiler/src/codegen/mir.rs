//! MIR（中级中间表示）定义与 HIR → MIR 转换
//!
//! 对应 技术方案 §5.2：HIR → MIR（基本块 + CFG）。
//!
//! MIR 是 **寄存器式** 的控制流图（CFG）：
//! - 每个函数由若干 `BasicBlock` 组成，块间通过 `Terminator` 连接
//! - 每条 `MirInstr` 将结果写入一个寄存器（`Reg`，即局部变量槽）
//! - 表达式被 "平坦化" 为一系列指令，控制流（`if`/`while`/`if-expr`）落入基本块
//!
//! 后续 `emit` 阶段将 MIR 的寄存器指令线性化为 §7.1 的栈式字节码。

use crate::ast::Literal;
use crate::codegen::hir::*;
use crate::codegen::opcode::Const;
use std::collections::{HashMap, HashSet};

/// 寄存器编号（同时作为局部变量槽编号）
pub type Reg = usize;

/// MIR 指令
#[derive(Debug, Clone, PartialEq)]
pub enum MirInstr {
    /// 将常量池 `ci` 处的值载入寄存器 `dst`
    LoadConst { dst: Reg, ci: usize },
    /// 将局部变量槽 `slot` 的值载入寄存器 `dst`
    LoadLocal { dst: Reg, slot: usize },
    /// 将寄存器 `src` 的值写入局部变量槽 `slot`
    StoreLocal { slot: usize, src: Reg },
    /// 二元运算：`dst = a op b`
    BinOp {
        dst: Reg,
        op: HirBinOp,
        a: Reg,
        b: Reg,
    },
    /// 一元运算：`dst = op a`
    UnOp { dst: Reg, op: HirUnOp, a: Reg },
    /// 调用用户函数（结果写入 `dst`，若无副作用可忽略）
    Call {
        dst: Option<Reg>,
        func: String,
        args: Vec<Reg>,
    },
    /// 调用原生/内置函数
    CallNative {
        dst: Option<Reg>,
        func: String,
        args: Vec<Reg>,
    },
    /// 分配对象（结果写入 `dst`）
    Alloc { dst: Reg, type_name: String },
    /// 读取字段：`dst = obj.name`
    GetField { dst: Reg, obj: Reg, field: String },
    /// 写入字段：`obj.name = src`
    SetField { obj: Reg, field: String, src: Reg },
    /// 数组元素读取：`dst = obj[idx]`
    GetIndex { dst: Reg, obj: Reg, idx: Reg },
    /// 数组元素写入：`obj[idx] = src`
    SetIndex { obj: Reg, idx: Reg, src: Reg },
    /// 保留引用计数 +1（P7.2 ARC 自动插入）
    Retain { src: Reg },
    /// 释放引用计数 -1（P7.2 ARC 自动插入）
    Release { src: Reg },
    /// 创建弱引用（P7.3）：`dst = weak(src)`
    WeakRef { dst: Reg, src: Reg },
    /// 从弱引用升级（P7.3）：`dst = upgrade(src)`
    WeakGet { dst: Reg, src: Reg },
    /// 显式堆分配（P7.5）：`dst = box(src)`
    Box { dst: Reg, src: Reg },
    /// 创建 C 回调蹦床（P8.7）：`dst = makeCallback(func_name)`
    MakeCallback { dst: Reg, func: String },
    /// defer 清理块开始标记（P7.4）
    DeferBegin,
    /// defer 清理块结束标记（P7.4）
    DeferEnd,
    /// 协程挂起点（P10.1）：`await` 挂起当前协程
    Yield,
}

/// 基本块终结指令（控制流）
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    /// 无条件跳转至块 `0`
    Goto(usize),
    /// 条件跳转：`cond` 为真跳 `then_b`，否则跳 `else_b`
    If {
        cond: Reg,
        then_b: usize,
        else_b: usize,
    },
    /// 返回寄存器 `0` 的值
    Return(Reg),
    /// 返回 Unit
    ReturnVoid,
}

/// 基本块
#[derive(Debug, Clone, PartialEq)]
pub struct BasicBlock {
    pub id: usize,
    pub instrs: Vec<MirInstr>,
    pub term: Terminator,
}

/// MIR 函数
#[derive(Debug, Clone, PartialEq)]
pub struct MirFunction {
    pub name: String,
    /// 参数名 -> 槽位（槽位 0..param_count 预留给参数）
    pub param_slots: Vec<usize>,
    pub blocks: Vec<BasicBlock>,
    /// 寄存器（局部变量槽）总数
    pub reg_count: usize,
    pub is_native: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// 降级上下文（常量池、原生函数表，全局共享）
// ─────────────────────────────────────────────────────────────────────────────

/// 常量去重键
#[derive(Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    Int(i64),
    Float(u64), // f64 to_bits
    Str(String),
    Bool(bool),
    Null,
}

/// 全局降级上下文
pub struct LowerCtx {
    pub consts: Vec<Const>,
    const_map: HashMap<ConstKey, usize>,
    pub natives: HashSet<String>,
    pub native_params: HashMap<String, u16>,
}

impl LowerCtx {
    pub fn new() -> Self {
        LowerCtx {
            consts: Vec::new(),
            const_map: HashMap::new(),
            natives: HashSet::new(),
            native_params: HashMap::new(),
        }
    }

    /// 注册原生函数签名
    pub fn register_native(&mut self, name: &str, param_count: u16) {
        self.natives.insert(name.to_string());
        self.native_params.insert(name.to_string(), param_count);
    }

    /// 取得字面量对应的常量池索引（去重）
    pub fn const_idx(&mut self, lit: &Literal) -> usize {
        let key = match lit {
            Literal::Int(i) => ConstKey::Int(*i),
            Literal::Float(f) => ConstKey::Float(f.to_bits()),
            Literal::String(s) => ConstKey::Str(s.clone()),
            Literal::Char(c) => ConstKey::Int(*c as i64),
            Literal::Bool(b) => ConstKey::Bool(*b),
            Literal::Null => ConstKey::Null,
        };
        if let Some(&i) = self.const_map.get(&key) {
            return i;
        }
        let idx = self.consts.len();
        let c = match lit {
            Literal::Int(i) => Const::Int(*i),
            Literal::Float(f) => Const::Float(*f),
            Literal::String(s) => Const::Str(s.clone()),
            Literal::Char(c) => Const::Int(*c as i64),
            Literal::Bool(b) => Const::Bool(*b),
            Literal::Null => Const::Null,
        };
        self.consts.push(c);
        self.const_map.insert(key, idx);
        idx
    }

    pub fn null_idx(&mut self) -> usize {
        self.const_idx(&Literal::Null)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// MIR 构建器（每个函数一个实例）
// ─────────────────────────────────────────────────────────────────────────────

struct MirBuilder {
    blocks: Vec<BasicBlock>,
    current: usize,
    next_reg: usize,
    scopes: Vec<HashMap<String, usize>>,
    loop_stack: Vec<(usize, usize)>,
}

impl MirBuilder {
    fn new(param_count: usize) -> Self {
        MirBuilder {
            blocks: vec![BasicBlock {
                id: 0,
                instrs: vec![],
                term: Terminator::Goto(0), // 哨兵：未闭合
            }],
            current: 0,
            next_reg: param_count,
            scopes: vec![HashMap::new()],
            loop_stack: vec![],
        }
    }

    fn new_block(&mut self) -> usize {
        let id = self.blocks.len();
        self.blocks.push(BasicBlock {
            id,
            instrs: vec![],
            term: Terminator::Goto(0),
        });
        id
    }

    fn emit(&mut self, instr: MirInstr) {
        self.blocks[self.current].instrs.push(instr);
    }

    fn set_term(&mut self, term: Terminator) {
        self.blocks[self.current].term = term;
    }

    /// 块是否已被"真实"终结指令闭合（非哨兵 Goto(0)）
    fn is_closed(&self, id: usize) -> bool {
        !matches!(self.blocks[id].term, Terminator::Goto(0))
    }

    fn alloc_reg(&mut self) -> Reg {
        let r = self.next_reg;
        self.next_reg += 1;
        r
    }

    fn declare(&mut self, name: &str, slot: Reg) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), slot);
    }

    fn lookup(&self, name: &str) -> Option<Reg> {
        for scope in self.scopes.iter().rev() {
            if let Some(&r) = scope.get(name) {
                return Some(r);
            }
        }
        None
    }

    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    // ── 语句降级 ──
    fn lower_block(&mut self, b: &HirBlock, ctx: &mut LowerCtx) {
        let stmts = b.stmts.clone();
        for s in &stmts {
            if self.is_closed(self.current) {
                break; // 当前块已终结（如 Return），后续语句不可达
            }
            self.lower_stmt(s, ctx);
        }
    }

    fn lower_stmt(&mut self, s: &HirStmt, ctx: &mut LowerCtx) {
        match s {
            HirStmt::Val { name, init, .. } | HirStmt::Var { name, init, .. } => {
                let reg = self.alloc_reg();
                if let Some(e) = init {
                    let v = self.lower_expr(e, ctx);
                    self.emit(MirInstr::StoreLocal { slot: reg, src: v });
                }
                self.declare(name, reg);
            }
            HirStmt::Assign { target, value } => {
                let v = self.lower_expr(value, ctx);
                match target {
                    HirExpr::Var(n) => {
                        if let Some(slot) = self.lookup(n) {
                            self.emit(MirInstr::StoreLocal { slot, src: v });
                        }
                    }
                    HirExpr::Member { object, name } => {
                        let obj = self.lower_expr(object, ctx);
                        self.emit(MirInstr::SetField {
                            obj,
                            field: name.clone(),
                            src: v,
                        });
                    }
                    HirExpr::Index { container, index } => {
                        let obj = self.lower_expr(container, ctx);
                        let i = self.lower_expr(index, ctx);
                        self.emit(MirInstr::SetIndex {
                            obj,
                            idx: i,
                            src: v,
                        });
                    }
                    _ => { /* 其它赋值目标暂不支持 */ }
                }
            }
            HirStmt::Expr(e) => {
                self.lower_expr(e, ctx);
            }
            HirStmt::Return(v) => match v {
                Some(e) => {
                    let r = self.lower_expr(e, ctx);
                    self.set_term(Terminator::Return(r));
                }
                None => self.set_term(Terminator::ReturnVoid),
            },
            HirStmt::If {
                cond,
                then_b,
                else_b,
            } => {
                let c = self.lower_expr(cond, ctx);
                let then_id = self.new_block();
                let else_id = self.new_block();
                let merge_id = self.new_block();
                self.set_term(Terminator::If {
                    cond: c,
                    then_b: then_id,
                    else_b: else_id,
                });
                // then 分支
                self.current = then_id;
                self.enter_scope();
                self.lower_block(then_b, ctx);
                self.exit_scope();
                if !self.is_closed(then_id) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                // else 分支
                self.current = else_id;
                self.enter_scope();
                if let Some(eb) = else_b {
                    self.lower_block(eb, ctx);
                }
                self.exit_scope();
                if !self.is_closed(else_id) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                self.current = merge_id;
            }
            HirStmt::While { cond, body } => {
                let cond_id = self.new_block();
                let body_id = self.new_block();
                let exit_id = self.new_block();
                self.set_term(Terminator::Goto(cond_id));
                self.current = cond_id;
                let c = self.lower_expr(cond, ctx);
                self.set_term(Terminator::If {
                    cond: c,
                    then_b: body_id,
                    else_b: exit_id,
                });
                self.loop_stack.push((cond_id, exit_id));
                self.current = body_id;
                self.enter_scope();
                self.lower_block(body, ctx);
                self.exit_scope();
                self.loop_stack.pop();
                // 循环体结束的「当前块」可能是嵌套 if 的 merge 块（而非 body_id），
                // 必须向当前块补一条回边 Goto(cond_id)，否则循环只执行一次。
                if !self.is_closed(self.current) {
                    self.set_term(Terminator::Goto(cond_id));
                }
                self.current = exit_id;
            }
            HirStmt::Break => {
                let (_, break_id) = self.loop_stack.last().cloned().unwrap_or((0, 0));
                self.set_term(Terminator::Goto(break_id));
            }
            HirStmt::Continue => {
                let (cont_id, _) = self.loop_stack.last().cloned().unwrap_or((0, 0));
                self.set_term(Terminator::Goto(cont_id));
            }
            HirStmt::Block(b) => {
                self.enter_scope();
                self.lower_block(b, ctx);
                self.exit_scope();
            }
            HirStmt::Defer(b) => {
                // P7.4: defer 语句 — 将清理块作为延迟执行的代码
                // 在 MIR 中，defer 块的语句立即执行（简化处理），
                // 编译期后续通过 DeferBegin/DeferEnd 标记 defer 区域
                self.enter_scope();
                self.lower_block(b, ctx);
                self.exit_scope();
            }
        }
    }

    // ── 表达式降级：返回结果寄存器 ──
    fn lower_expr(&mut self, e: &HirExpr, ctx: &mut LowerCtx) -> Reg {
        match e {
            HirExpr::Lit(l) => {
                let ci = ctx.const_idx(l);
                let dst = self.alloc_reg();
                self.emit(MirInstr::LoadConst { dst, ci });
                dst
            }
            HirExpr::Var(n) => {
                let slot = self.lookup(n).unwrap_or(0);
                let dst = self.alloc_reg();
                self.emit(MirInstr::LoadLocal { dst, slot });
                dst
            }
            HirExpr::Binary { op, lhs, rhs } => {
                let a = self.lower_expr(lhs, ctx);
                let b = self.lower_expr(rhs, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::BinOp { dst, op: *op, a, b });
                dst
            }
            HirExpr::Unary { op, operand } => {
                let a = self.lower_expr(operand, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::UnOp { dst, op: *op, a });
                dst
            }
            HirExpr::Call { callee, args } => {
                // P8.7: makeCallback 特殊处理 — 生成 MakeCallback 指令
                if callee == "makeCallback" && args.len() == 1 {
                    if let HirExpr::Var(name) = &args[0] {
                        let dst = self.alloc_reg();
                        self.emit(MirInstr::MakeCallback {
                            dst,
                            func: name.clone(),
                        });
                        return dst;
                    }
                }
                let argv: Vec<Reg> = args.iter().map(|a| self.lower_expr(a, ctx)).collect();
                let dst = self.alloc_reg();
                if ctx.natives.contains(callee.as_str()) {
                    self.emit(MirInstr::CallNative {
                        dst: Some(dst),
                        func: callee.clone(),
                        args: argv,
                    });
                } else {
                    self.emit(MirInstr::Call {
                        dst: Some(dst),
                        func: callee.clone(),
                        args: argv,
                    });
                }
                dst
            }
            HirExpr::Member { object, name } => {
                let obj = self.lower_expr(object, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::GetField {
                    dst,
                    obj,
                    field: name.clone(),
                });
                dst
            }
            HirExpr::Index { container, index } => {
                let c = self.lower_expr(container, ctx);
                let i = self.lower_expr(index, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::GetIndex {
                    dst,
                    obj: c,
                    idx: i,
                });
                dst
            }
            HirExpr::New { type_name, args } => {
                let argv: Vec<Reg> = args.iter().map(|a| self.lower_expr(a, ctx)).collect();
                let dst = self.alloc_reg();
                self.emit(MirInstr::Alloc {
                    dst,
                    type_name: type_name.clone(),
                });
                if !argv.is_empty() {
                    self.emit(MirInstr::CallNative {
                        dst: None,
                        func: "__ctor".into(),
                        args: argv,
                    });
                }
                dst
            }
            HirExpr::If {
                cond,
                then_e,
                else_e,
            } => {
                let c = self.lower_expr(cond, ctx);
                let then_id = self.new_block();
                let else_id = self.new_block();
                let merge_id = self.new_block();
                self.set_term(Terminator::If {
                    cond: c,
                    then_b: then_id,
                    else_b: else_id,
                });
                let res = self.alloc_reg();
                self.current = then_id;
                let tv = self.lower_expr(then_e, ctx);
                self.emit(MirInstr::StoreLocal { slot: res, src: tv });
                if !self.is_closed(then_id) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                self.current = else_id;
                let ev = self.lower_expr(else_e, ctx);
                self.emit(MirInstr::StoreLocal { slot: res, src: ev });
                if !self.is_closed(else_id) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                self.current = merge_id;
                let out = self.alloc_reg();
                self.emit(MirInstr::LoadLocal {
                    dst: out,
                    slot: res,
                });
                out
            }
            HirExpr::Block(b) => {
                self.enter_scope();
                let mut last: Option<Reg> = None;
                let stmts = b.stmts.clone();
                let n = stmts.len();
                for (i, st) in stmts.iter().enumerate() {
                    if self.is_closed(self.current) {
                        break;
                    }
                    if i + 1 == n {
                        if let HirStmt::Expr(e) = st {
                            last = Some(self.lower_expr(e, ctx));
                            continue;
                        }
                    }
                    self.lower_stmt(st, ctx);
                }
                self.exit_scope();
                match last {
                    Some(r) => r,
                    None => {
                        let d = self.alloc_reg();
                        let ci = ctx.null_idx();
                        self.emit(MirInstr::LoadConst { dst: d, ci });
                        d
                    }
                }
            }
            HirExpr::Box(inner) => {
                // P7.5: 显式堆分配 — 先求值内部表达式，再分配堆对象
                let src = self.lower_expr(inner, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::Box { dst, src });
                dst
            }
            HirExpr::WeakRef(inner) => {
                // P7.3: 创建弱引用 — 先求值被引用对象，再创建弱引用
                let src = self.lower_expr(inner, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::WeakRef { dst, src });
                dst
            }
            HirExpr::Await(inner) => {
                // P10.1: await — 直接返回内部表达式结果
                // 协程挂起语义由 Yield 指令处理（在非协程上下文中 await 等同 no-op）
                self.lower_expr(inner, ctx)
            }
        }
    }

    fn finalize(mut self, name: String, param_slots: Vec<usize>, is_native: bool) -> MirFunction {
        // 闭合所有仍打开（哨兵）的块：视为不可达，返回 Unit
        for b in &mut self.blocks {
            if matches!(b.term, Terminator::Goto(0)) {
                b.term = Terminator::ReturnVoid;
            }
        }
        MirFunction {
            name,
            param_slots,
            blocks: self.blocks,
            reg_count: self.next_reg,
            is_native,
        }
    }
}

/// 将单个 HIR 函数降级为 MIR
pub fn lower_function(f: &HirFunction, ctx: &mut LowerCtx) -> MirFunction {
    let param_slots: Vec<usize> = (0..f.params.len()).collect();
    let mut builder = MirBuilder::new(f.params.len());
    // 声明参数到作用域
    for (i, p) in f.params.iter().enumerate() {
        builder.declare(&p.name, i);
    }
    if !f.is_native && !f.body.stmts.is_empty() {
        builder.lower_block(&f.body, ctx);
    }
    if !builder.is_closed(builder.current) {
        builder.set_term(Terminator::ReturnVoid);
    }
    builder.finalize(f.name.clone(), param_slots, f.is_native)
}

/// 将整个 HIR 程序降级为 MIR（含原生函数登记）
pub fn lower_program(hir: &HirProgram) -> (Vec<MirFunction>, LowerCtx) {
    let mut ctx = LowerCtx::new();
    for n in &hir.natives {
        let pc = n.params.len() as u16;
        ctx.register_native(&n.name, pc);
    }
    let mut mir_funcs = Vec::new();
    for f in &hir.functions {
        if f.is_native {
            continue; // 原生函数没有 body，仅登记签名（已在 natives 中）
        }
        mir_funcs.push(lower_function(f, &mut ctx));
    }
    (mir_funcs, ctx)
}
