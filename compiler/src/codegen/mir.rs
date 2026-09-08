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
    BinOp { dst: Reg, op: HirBinOp, a: Reg, b: Reg },
    /// 一元运算：`dst = op a`
    UnOp { dst: Reg, op: HirUnOp, a: Reg },
    /// 调用用户函数（结果写入 `dst`，若无副作用可忽略）
    Call { dst: Option<Reg>, func: String, args: Vec<Reg> },
    /// 调用原生/内置函数
    CallNative { dst: Option<Reg>, func: String, args: Vec<Reg> },
    /// 调用闭包（Phase 2）：`dst = CallClosure(closure, args...)`
    CallClosure { dst: Option<Reg>, closure: Reg, args: Vec<Reg> },
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
    /// 创建闭包（Phase 2）：`dst = closure(func_name, captures...)`
    MakeClosure { dst: Reg, func: String, captures: Vec<(String, Reg)> },
    /// 构造枚举变体（Phase 3）：`dst = EnumConstruct(enum_name, variant_idx)`
    EnumConstruct { dst: Reg, enum_name: String, variant_idx: u16 },
    /// 获取枚举变体索引（Phase 3）：`dst = EnumTag(src)`
    EnumTag { dst: Reg, src: Reg },
    /// 创建函数引用（Phase 3）：`dst = MakeFnRef(func_name)`
    MakeFnRef { dst: Reg, func: String },
    /// defer 清理块开始标记（P7.4）
    DeferBegin,
    /// defer 清理块结束标记（P7.4）
    DeferEnd,
    /// 协程挂起点（P10.1）：`await` 挂起当前协程
    Yield,
    /// 虚方法调用（P-K2）：vtable 分派。`args[0]` 为接收者（self），
    /// 发射时按序压参后额外再压一次接收者（VM 先弹对象）。
    CallMethod { dst: Option<Reg>, method: String, args: Vec<Reg> },
    /// 类型检查（Phase 2）：`dst = (src is instance of type_id)`
    InstanceOf { dst: Reg, src: Reg, type_id: u16 },
    /// 类型转换（Phase 2）：`dst = (src as type_id)`，不匹配则报错
    CheckCast { dst: Reg, src: Reg, type_id: u16 },
}

/// 基本块终结指令（控制流）
#[derive(Debug, Clone, PartialEq)]
pub enum Terminator {
    /// 无条件跳转至块 `0`
    Goto(usize),
    /// 条件跳转：`cond` 为真跳 `then_b`，否则跳 `else_b`
    If { cond: Reg, then_b: usize, else_b: usize },
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
    /// 闭包表（Phase 2）：函数中创建的闭包
    pub closures: Vec<MirClosure>,
}

/// MIR 闭包（Phase 2）
#[derive(Debug, Clone, PartialEq)]
pub struct MirClosure {
    /// 闭包函数名（如 `__lambda_0`）
    pub name: String,
    /// 用户参数名
    pub params: Vec<String>,
    /// 寄存器总数
    pub reg_count: usize,
    /// 捕获变量名
    pub capture_names: Vec<String>,
    /// 捕获变量在闭包函数中的参数槽位
    pub capture_slots: Vec<usize>,
    /// 闭包体（简化：单块）
    pub body: Vec<MirInstr>,
    /// 闭包体终结指令
    pub term: Terminator,
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
    /// Phase 1: 用户自定义函数名集合（用于区分 CallNative vs Call）
    pub user_functions: HashSet<String>,
    /// Phase 3: 枚举名集合（用于识别 EnumConstruct）
    pub enum_names: HashSet<String>,
    /// Phase 2: 闭包计数器
    pub next_closure_id: usize,
}

impl LowerCtx {
    pub fn new() -> Self {
        LowerCtx {
            consts: Vec::new(),
            const_map: HashMap::new(),
            natives: HashSet::new(),
            native_params: HashMap::new(),
            user_functions: HashSet::new(),
            enum_names: HashSet::new(),
            next_closure_id: 0,
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
    /// Phase 2: 闭包表（函数中创建的闭包）
    closures: Vec<MirClosure>,
    /// Phase 3: HIR 程序引用（用于枚举变体查找）
    hir_program: Option<crate::codegen::hir::HirProgram>,
}

impl MirBuilder {
    fn new(param_count: usize) -> Self {
        MirBuilder {
            blocks: vec![
                BasicBlock {
                    id: 0,
                    instrs: vec![],
                    term: Terminator::Goto(0), // 哨兵：未闭合
                },
            ],
            current: 0,
            next_reg: param_count,
            scopes: vec![HashMap::new()],
            loop_stack: vec![],
            closures: Vec::new(),
            hir_program: None,
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
        self.scopes.last_mut().unwrap().insert(name.to_string(), slot);
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

    // ── Phase 2: Lambda 降级 ──

    /// 将 HirBlock 中的标识符引用收集为捕获变量列表
    fn collect_free_vars(expr: &HirExpr, locals: &mut HashSet<String>, out: &mut Vec<String>) {
        match expr {
            HirExpr::Var(name) => {
                if !locals.contains(name) {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
            }
            HirExpr::Lambda {
                params,
                body,
                ..
            } => {
                for p in params {
                    locals.insert(p.name.clone());
                }
                Self::collect_free_vars_in_block(body, locals, out);
                for p in params {
                    locals.remove(&p.name);
                }
            }
            HirExpr::Block(b) => Self::collect_free_vars_in_block(b, locals, out),
            HirExpr::Call {
                callee: _,
                args,
            } => {
                for a in args {
                    Self::collect_free_vars(a, locals, out);
                }
            }
            HirExpr::Member {
                object,
                name: _,
            } => {
                Self::collect_free_vars(object, locals, out);
            }
            HirExpr::Index {
                container,
                index,
            } => {
                Self::collect_free_vars(container, locals, out);
                Self::collect_free_vars(index, locals, out);
            }
            HirExpr::Binary {
                op: _,
                lhs,
                rhs,
            } => {
                Self::collect_free_vars(lhs, locals, out);
                Self::collect_free_vars(rhs, locals, out);
            }
            HirExpr::Unary {
                op: _,
                operand,
            } => {
                Self::collect_free_vars(operand, locals, out);
            }
            HirExpr::If {
                cond,
                then_e,
                else_e,
            } => {
                Self::collect_free_vars(cond, locals, out);
                Self::collect_free_vars(then_e, locals, out);
                Self::collect_free_vars(else_e, locals, out);
            }
            HirExpr::New {
                type_name: _,
                args,
            } => {
                for a in args {
                    Self::collect_free_vars(a, locals, out);
                }
            }
            HirExpr::Box(inner) | HirExpr::WeakRef(inner) | HirExpr::Await(inner) => {
                Self::collect_free_vars(inner, locals, out);
            }
            HirExpr::Lambda {
                params: _,
                body,
            } => {
                Self::collect_free_vars_in_block(body, locals, out);
            }
            _ => {} // Literal, Unit, New, etc. have no free vars
        }
    }

    fn collect_free_vars_in_block(
        block: &HirBlock,
        locals: &mut HashSet<String>,
        out: &mut Vec<String>,
    ) {
        for stmt in &block.stmts {
            match stmt {
                HirStmt::Val {
                    init, name, ..
                }
                | HirStmt::Var {
                    init, name, ..
                } => {
                    if let Some(e) = init {
                        Self::collect_free_vars(e, locals, out);
                    }
                    locals.insert(name.clone());
                }
                HirStmt::Expr(e) => {
                    Self::collect_free_vars(e, locals, out);
                }
                HirStmt::Return(e) => {
                    if let Some(v) = e {
                        Self::collect_free_vars(v, locals, out);
                    }
                }
                HirStmt::If {
                    cond,
                    then_b,
                    else_b,
                } => {
                    Self::collect_free_vars(cond, locals, out);
                    Self::collect_free_vars_in_block(then_b, locals, out);
                    if let Some(eb) = else_b {
                        Self::collect_free_vars_in_block(eb, locals, out);
                    }
                }
                HirStmt::While { cond, body } => {
                    Self::collect_free_vars(cond, locals, out);
                    Self::collect_free_vars_in_block(body, locals, out);
                }
                HirStmt::Break => {}
                HirStmt::Continue => {}
                HirStmt::Defer(b) => {
                    Self::collect_free_vars_in_block(b, locals, out);
                }
                HirStmt::Assign {
                    target,
                    value,
                } => {
                    Self::collect_free_vars(target, locals, out);
                    Self::collect_free_vars(value, locals, out);
                }
                HirStmt::Block(b) => {
                    Self::collect_free_vars_in_block(b, locals, out);
                }
            }
        }
    }

    /// 将 HIR Lambda 降级为 MakeClosure 指令 + MirClosure
    fn lower_lambda(&mut self, params: &[HirParam], body: &HirBlock, ctx: &mut LowerCtx) -> Reg {
        let closure_id = ctx.next_closure_id;
        ctx.next_closure_id += 1;
        let closure_name = format!("__lambda_{}", closure_id);

        // 1. 收集 lambda 参数名（作为 locals）
        let mut locals: HashSet<String> = HashSet::new();
        for p in params {
            locals.insert(p.name.clone());
        }

        // 2. 收集捕获变量（free vars）
        let mut captures: Vec<String> = Vec::new();
        Self::collect_free_vars_in_block(body, &mut locals, &mut captures);

        // 3. 为每个捕获变量分配当前函数中的寄存器
        let capture_regs: Vec<Reg> = captures.iter().filter_map(|name| self.lookup(name)).collect();

        // 4. 构建闭包函数体
        let param_slots: Vec<usize> = (0..params.len()).collect();
        let mut builder = MirBuilder::new(params.len());
        for (i, p) in params.iter().enumerate() {
            builder.declare(&p.name, i);
        }

        // 5. 降级闭包体
        if !body.stmts.is_empty() {
            // 检查最后一条语句是否为纯表达式（Lambda 的返回值）
            if let Some(last_stmt) = body.stmts.last() {
                if let HirStmt::Expr(e) = last_stmt {
                    // 降级除最后一条外的所有语句
                    for s in &body.stmts[..body.stmts.len() - 1] {
                        builder.lower_stmt(s, ctx);
                    }
                    // 降级最后一条表达式并发射 Return
                    let r = builder.lower_expr(e, ctx);
                    builder.set_term(Terminator::Return(r));
                } else {
                    // 所有语句都不是纯表达式，正常降级
                    builder.lower_block(body, ctx);
                }
            } else {
                builder.lower_block(body, ctx);
            }
        }
        if !builder.is_closed(builder.current) {
            builder.set_term(Terminator::ReturnVoid);
        }

        // 6. 创建 MirClosure
        let closure = MirClosure {
            name: closure_name.clone(),
            params: params.iter().map(|p| p.name.clone()).collect(),
            reg_count: builder.next_reg,
            capture_names: captures.clone(),
            capture_slots: Vec::new(),
            body: builder.blocks.iter().flat_map(|b| b.instrs.clone()).collect(),
            term: builder.blocks.last().map(|b| b.term.clone()).unwrap_or(Terminator::ReturnVoid),
        };
        self.closures.push(closure);

        // 7. 发射 MakeClosure 指令
        let dst = self.alloc_reg();
        let mut make_captures = Vec::new();
        for (name, reg) in captures.iter().zip(capture_regs.iter()) {
            make_captures.push((name.clone(), *reg));
        }
        self.emit(MirInstr::MakeClosure {
            dst,
            func: closure_name.clone(),
            captures: make_captures,
        });
        dst
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
            HirStmt::Val {
                name, init, ..
            }
            | HirStmt::Var {
                name, init, ..
            } => {
                let reg = self.alloc_reg();
                if let Some(e) = init {
                    let v = self.lower_expr(e, ctx);
                    self.emit(MirInstr::StoreLocal {
                        slot: reg,
                        src: v,
                    });
                }
                self.declare(name, reg);
            }
            HirStmt::Assign {
                target,
                value,
            } => {
                let v = self.lower_expr(value, ctx);
                match target {
                    HirExpr::Var(n) => {
                        if let Some(slot) = self.lookup(n) {
                            self.emit(MirInstr::StoreLocal {
                                slot,
                                src: v,
                            });
                        }
                    }
                    HirExpr::Member {
                        object,
                        name,
                    } => {
                        let obj = self.lower_expr(object, ctx);
                        self.emit(MirInstr::SetField {
                            obj,
                            field: name.clone(),
                            src: v,
                        });
                    }
                    HirExpr::Index {
                        container,
                        index,
                    } => {
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
                if !self.is_closed(self.current) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                // else 分支
                self.current = else_id;
                self.enter_scope();
                if let Some(eb) = else_b {
                    self.lower_block(eb, ctx);
                }
                self.exit_scope();
                if !self.is_closed(self.current) {
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
            HirExpr::Binary {
                op,
                lhs,
                rhs,
            } => {
                let a = self.lower_expr(lhs, ctx);
                let b = self.lower_expr(rhs, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::BinOp {
                    dst,
                    op: *op,
                    a,
                    b,
                });
                dst
            }
            HirExpr::Unary {
                op,
                operand,
            } => {
                let a = self.lower_expr(operand, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::UnOp {
                    dst,
                    op: *op,
                    a,
                });
                dst
            }
            HirExpr::Call {
                callee,
                args,
            } => {
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
                // Phase 1: 优先检查用户自定义函数 — 有同名用户函数则走 Call，否则走 CallNative
                if ctx.user_functions.contains(callee.as_str()) {
                    self.emit(MirInstr::Call {
                        dst: Some(dst),
                        func: callee.clone(),
                        args: argv,
                    });
                } else if ctx.natives.contains(callee.as_str()) {
                    self.emit(MirInstr::CallNative {
                        dst: Some(dst),
                        func: callee.clone(),
                        args: argv,
                    });
                } else if let Some(closure_reg) = self.lookup(callee) {
                    // Phase 2: 变量调用 — 检测闭包调用，生成 CallClosure 指令
                    self.emit(MirInstr::CallClosure {
                        dst: Some(dst),
                        closure: closure_reg,
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
            HirExpr::Member {
                object,
                name,
            } => {
                // Phase 3: 检测枚举变体引用（EnumName.Variant）
                if let HirExpr::Var(enum_name) = object.as_ref() {
                    if ctx.enum_names.contains(enum_name) {
                        // 查找变体索引
                        let variant_idx = if let Some(hir_program) = self.hir_program.as_ref() {
                            hir_program
                                .enums
                                .iter()
                                .find(|e| &e.name == enum_name)
                                .and_then(|e| {
                                    e.variants.iter().position(|(vname, _)| vname == name)
                                })
                                .unwrap_or(0) as u16
                        } else {
                            0
                        };
                        let dst = self.alloc_reg();
                        self.emit(MirInstr::EnumConstruct {
                            dst,
                            enum_name: enum_name.clone(),
                            variant_idx,
                        });
                        return dst;
                    }
                }
                let obj = self.lower_expr(object, ctx);
                let dst = self.alloc_reg();
                self.emit(MirInstr::GetField {
                    dst,
                    obj,
                    field: name.clone(),
                });
                dst
            }
            HirExpr::Index {
                container,
                index,
            } => {
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
            HirExpr::New {
                type_name,
                args,
            } => {
                let argv: Vec<Reg> = args.iter().map(|a| self.lower_expr(a, ctx)).collect();
                let dst = self.alloc_reg();
                self.emit(MirInstr::Alloc {
                    dst,
                    type_name: type_name.clone(),
                });
                // 查找结构体字段名和默认值
                let (field_names, default_values, synth_ctors) = {
                    let hp = self.hir_program.as_ref();
                    hp.and_then(|hp| hp.structs.iter().find(|s| s.name == *type_name))
                        .map(|s| {
                            (
                                s.fields.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
                                s.default_values.clone(),
                                s.synth_ctors.clone(),
                            )
                        })
                        .unwrap_or_default()
                };
                // P-K2：类合成构造函数（init 块/次构造函数）—— 字段默认值 + __ctorN(self, args...)
                if let Some(&arity) = synth_ctors.iter().find(|&&a| a == argv.len()) {
                    // 先填字段默认值（构造函数体可依赖）
                    for (i, dv) in default_values.iter().enumerate() {
                        if let Some(field_name) = field_names.get(i) {
                            if let Some(expr) = dv {
                                let r = self.lower_expr(&**expr, ctx);
                                self.emit(MirInstr::SetField {
                                    obj: dst,
                                    field: field_name.clone(),
                                    src: r,
                                });
                            }
                        }
                    }
                    let mut cargs = vec![dst];
                    cargs.extend(argv);
                    let call_dst = self.alloc_reg();
                    self.emit(MirInstr::Call {
                        dst: Some(call_dst),
                        func: format!("{}.__ctor{}", type_name, arity),
                        args: cargs,
                    });
                    return dst;
                }
                if !argv.is_empty() {
                    // 有参数：使用提供的参数
                    for (i, arg) in argv.iter().enumerate() {
                        if let Some(field_name) = field_names.get(i) {
                            self.emit(MirInstr::SetField {
                                obj: dst,
                                field: field_name.clone(),
                                src: *arg,
                            });
                        }
                    }
                } else {
                    // 无参数：使用默认值
                    for (i, dv) in default_values.iter().enumerate() {
                        if let Some(field_name) = field_names.get(i) {
                            if let Some(expr) = dv {
                                let r = self.lower_expr(&**expr, ctx);
                                self.emit(MirInstr::SetField {
                                    obj: dst,
                                    field: field_name.clone(),
                                    src: r,
                                });
                            }
                        }
                    }
                }
                dst
            }
            HirExpr::CallVirtual {
                recv,
                name,
                args,
            } => {
                // 栈序：args（含 self）按序压栈，最后再压一次接收者（do_call_method 先弹对象）
                let mut argv: Vec<Reg> = vec![self.lower_expr(recv, ctx)];
                for a in args {
                    argv.push(self.lower_expr(a, ctx));
                }
                let dst = self.alloc_reg();
                self.emit(MirInstr::CallMethod {
                    dst: Some(dst),
                    method: name.clone(),
                    args: argv,
                });
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
                // then 分支：注意 lower_expr 可能改变 self.current（嵌套 If），
                // 因此用 self.current 而非 then_id 检查是否已终结
                self.current = then_id;
                let tv = self.lower_expr(then_e, ctx);
                self.emit(MirInstr::StoreLocal {
                    slot: res,
                    src: tv,
                });
                if !self.is_closed(self.current) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                // else 分支：同理
                self.current = else_id;
                let ev = self.lower_expr(else_e, ctx);
                self.emit(MirInstr::StoreLocal {
                    slot: res,
                    src: ev,
                });
                if !self.is_closed(self.current) {
                    self.set_term(Terminator::Goto(merge_id));
                }
                // merge 块：加载分支结果
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
            // Phase 2: Lambda — 降级为 MakeClosure 指令
            HirExpr::Lambda {
                params,
                body,
            } => self.lower_lambda(params, body, ctx),
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
            closures: self.closures,
        }
    }
}

/// 将单个 HIR 函数降级为 MIR
pub fn lower_function(f: &HirFunction, ctx: &mut LowerCtx, hir: &HirProgram) -> MirFunction {
    let param_slots: Vec<usize> = (0..f.params.len()).collect();
    let mut builder = MirBuilder::new(f.params.len());
    builder.hir_program = Some(hir.clone());
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
    // Phase 3: 注册枚举名
    for e in &hir.enums {
        ctx.enum_names.insert(e.name.clone());
    }
    let mut mir_funcs = Vec::new();
    for f in &hir.functions {
        if f.is_native {
            continue; // 原生函数没有 body，仅登记签名（已在 natives 中）
        }
        // Phase 1: 记录用户自定义函数名，供 lower_expr 区分 CallNative vs Call
        ctx.user_functions.insert(f.name.clone());
        mir_funcs.push(lower_function(f, &mut ctx, hir));
    }
    (mir_funcs, ctx)
}
