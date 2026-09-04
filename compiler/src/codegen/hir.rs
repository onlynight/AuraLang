//! HIR（高级中间表示）定义与 AST → HIR 降级（去语法糖）
//!
//! 对应 技术方案 §5.2：AST → HIR（+ 泛型单态化）。
//!
//! HIR 是去除了语法糖的 AST：
//! - `when` 降级为嵌套 `if` 表达式
//! - `for` 循环降级为 `while` 循环 + 索引变量
//! - `if` 表达式与 `if` 语句分别保留为 `HirExpr::If` / `HirStmt::If`
//! - 字符串插值、安全调用、Elvis 等降级为显式控制流/调用
//!
//! HIR 仍然是结构化、树状的表示，便于后续优化（常量折叠、内联、单态化）。

use crate::ast::*;
use crate::codegen::opcode::Const;
use crate::span::Span;

/// HIR 类型（降级阶段仅保留最简单的形式：基本类型名 / 命名类型）
#[derive(Debug, Clone, PartialEq)]
pub enum HirType {
    /// 基本/命名类型名（如 `Int`、`String`、`Player`）
    Named(String),
    /// 可空包装
    Nullable(Box<HirType>),
    /// 原始指针类型（P8.6）：`Pointer<T>` 映射为 C 的 `T*`
    Pointer(Box<HirType>),
    /// 未知（由语义阶段兜底）
    Unknown,
}

impl HirType {
    pub fn from_ast_opt(ty: &Option<Box<Type>>) -> Option<HirType> {
        ty.as_ref().map(|t| HirType::from_ast(t))
    }

    /// 从类型名字符串构造 HIR 类型（用于内置函数注册）
    pub fn from_ast_str(name: &str) -> HirType {
        match name {
            "Int" => HirType::Named("Int".into()),
            "Float" => HirType::Named("Float".into()),
            "Double" => HirType::Named("Double".into()),
            "Boolean" => HirType::Named("Boolean".into()),
            "String" => HirType::Named("String".into()),
            "Char" => HirType::Named("Char".into()),
            "Unit" | "Void" => HirType::Named("Unit".into()),
            "Any" => HirType::Named("Any".into()),
            "Long" => HirType::Named("Long".into()),
            "Short" => HirType::Named("Short".into()),
            "Byte" => HirType::Named("Byte".into()),
            "CStr" | "CString" | "Handle" => HirType::Named("CStr".into()),
            s if s.starts_with("Pointer<") => {
                let inner = &s["Pointer<".len()..s.len() - 1];
                HirType::Pointer(Box::new(HirType::from_ast_str(inner)))
            }
            s => HirType::Named(s.into()),
        }
    }

    pub fn from_ast(ty: &Type) -> HirType {
        match ty {
            Type::Nullable(inner) => HirType::Nullable(Box::new(HirType::from_ast(inner))),
            Type::Pointer(inner) => HirType::Pointer(Box::new(HirType::from_ast(inner))),
            Type::Named { name, .. } => HirType::Named(name.clone()),
            Type::Int => HirType::Named("Int".into()),
            Type::Long => HirType::Named("Long".into()),
            Type::Float => HirType::Named("Float".into()),
            Type::String => HirType::Named("String".into()),
            Type::Boolean => HirType::Named("Boolean".into()),
            _ => HirType::Named(ty.to_string()),
        }
    }
}

/// HIR 二元运算符（语义等价于 AST 的 `BinOp` 计算子集）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    To,
}

impl HirBinOp {
    pub fn from_ast(op: BinOp) -> Self {
        match op {
            BinOp::Add => HirBinOp::Add,
            BinOp::Sub => HirBinOp::Sub,
            BinOp::Mul => HirBinOp::Mul,
            BinOp::Div => HirBinOp::Div,
            BinOp::Mod => HirBinOp::Rem,
            BinOp::Eq => HirBinOp::Eq,
            BinOp::Ne => HirBinOp::Ne,
            BinOp::Lt => HirBinOp::Lt,
            BinOp::Gt => HirBinOp::Gt,
            BinOp::Le => HirBinOp::Le,
            BinOp::Ge => HirBinOp::Ge,
            BinOp::And => HirBinOp::And,
            BinOp::Or => HirBinOp::Or,
            BinOp::BitAnd => HirBinOp::BitAnd,
            BinOp::BitOr => HirBinOp::BitOr,
            BinOp::BitXor => HirBinOp::BitXor,
            BinOp::Shl => HirBinOp::Shl,
            BinOp::Shr => HirBinOp::Shr,
            BinOp::To => HirBinOp::To,
            BinOp::Assign | BinOp::UShr => HirBinOp::Shr, // 近似
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUnOp {
    Minus,
    Not,
}

impl HirUnOp {
    pub fn from_ast(op: UnOp) -> Self {
        match op {
            UnOp::Minus => HirUnOp::Minus,
            UnOp::Not => HirUnOp::Not,
            _ => HirUnOp::Not,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HIR 表达式
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum HirExpr {
    Lit(Literal),
    Var(String),
    Binary {
        op: HirBinOp,
        lhs: Box<HirExpr>,
        rhs: Box<HirExpr>,
    },
    Unary {
        op: HirUnOp,
        operand: Box<HirExpr>,
    },
    Call {
        callee: String,
        args: Vec<HirExpr>,
    },
    Member {
        object: Box<HirExpr>,
        name: String,
    },
    Index {
        container: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    New {
        type_name: String,
        args: Vec<HirExpr>,
    },
    /// if 表达式（返回分支的值）
    If {
        cond: Box<HirExpr>,
        then_e: Box<HirExpr>,
        else_e: Box<HirExpr>,
    },
    Block(HirBlock),
    /// 显式堆分配（P7.5）：`box expr` 强制将值分配到堆上
    Box(Box<HirExpr>),
    /// 弱引用（P7.3）：`weak(ref)` 创建不增加引用计数的弱引用
    WeakRef(Box<HirExpr>),
    /// await 挂起点（P10.1）：`await expr` 在 suspend 函数中挂起协程
    Await(Box<HirExpr>),
}

impl HirExpr {
    pub fn span(&self) -> Span {
        Span::single(0, 1, 1)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HIR 语句
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum HirStmt {
    Val {
        name: String,
        ty: Option<HirType>,
        init: Option<HirExpr>,
    },
    Var {
        name: String,
        ty: Option<HirType>,
        init: Option<HirExpr>,
    },
    Assign {
        target: HirExpr,
        value: HirExpr,
    },
    Expr(HirExpr),
    Return(Option<HirExpr>),
    If {
        cond: HirExpr,
        then_b: HirBlock,
        else_b: Option<HirBlock>,
    },
    While {
        cond: HirExpr,
        body: HirBlock,
    },
    Break,
    Continue,
    Block(HirBlock),
    /// defer 语句（P7.4）：注册的清理块在作用域结束时 LIFO 顺序执行
    Defer(HirBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirBlock {
    pub stmts: Vec<HirStmt>,
}

// ─────────────────────────────────────────────────────────────────────────────
// HIR 顶层
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct HirParam {
    pub name: String,
    pub ty: Option<HirType>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirFunction {
    pub name: String,
    pub params: Vec<HirParam>,
    pub ret: Option<HirType>,
    pub body: HirBlock,
    /// 是否为原生/FFI 函数（无 body 代码，仅签名）
    pub is_native: bool,
    /// 是否为泛型函数（待单态化）
    pub type_params: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirStruct {
    pub name: String,
    pub fields: Vec<(String, HirType)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
    /// 原生函数签名集合（extern "c" / 内置）
    pub natives: Vec<HirFunction>,
    /// FFI 常量（P8.1）：extern 块中的 `val` 声明
    pub constants: Vec<(String, Const)>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 降级：AST → HIR
// ─────────────────────────────────────────────────────────────────────────────

/// 降级入口
pub fn desugar_program(program: &Program) -> HirProgram {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut natives = Vec::new();
    let mut constants = Vec::new();

    for decl in &program.declarations {
        match decl {
            Decl::Function(f) => functions.push(desugar_fn(f)),
            Decl::Struct(s) => structs.push(HirStruct {
                name: s.name.clone(),
                fields: s
                    .fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.clone(),
                            HirType::from_ast_opt(&f.type_hint).unwrap_or(HirType::Unknown),
                        )
                    })
                    .collect(),
            }),
            Decl::Extern(e) => {
                for f in &e.functions {
                    natives.push(HirFunction {
                        name: f.name.clone(),
                        params: f
                            .params
                            .iter()
                            .map(|p| HirParam {
                                name: p.name.clone(),
                                ty: HirType::from_ast_opt(&p.type_hint),
                            })
                            .collect(),
                        ret: HirType::from_ast_opt(&f.return_type),
                        body: HirBlock { stmts: vec![] },
                        is_native: true,
                        type_params: vec![],
                    });
                }
                // P8.1: 处理 extern 块中的常量
                for stmt in &e.constants {
                    if let Stmt::Val { name, initializer, .. } = stmt {
                        if let Some(init) = initializer {
                            if let Expr::Literal(lit, _) = init.as_ref() {
                                let c = match lit {
                                    crate::ast::Literal::Int(i) => Const::Int(*i),
                                    crate::ast::Literal::Float(f) => Const::Float(*f),
                                    crate::ast::Literal::String(s) => Const::Str(s.clone()),
                                    crate::ast::Literal::Bool(b) => Const::Bool(*b),
                                    crate::ast::Literal::Null => Const::Null,
                                    crate::ast::Literal::Char(c) => Const::Int(*c as i64),
                                };
                                constants.push((name.clone(), c));
                            }
                        }
                    }
                }
            }
            // class/interface/enum/actor/typealias/import/annotation：P4 仅保留结构，
            // 类型层面的成员函数已展开到 functions 中（若有 body）。
            Decl::Class(c) => {
                for m in &c.methods {
                    functions.push(desugar_fn(m));
                }
            }
            Decl::Interface(_) => {}
            Decl::Enum(_) => {}
            Decl::Actor(a) => {
                for m in &a.methods {
                    functions.push(desugar_fn(m));
                }
            }
            Decl::TypeAlias(_) => {}
            Decl::Import(_) => {}
            Decl::Annotation(_) => {}
        }
    }

    // 将内置 println 注册为原生函数（若语义分析已声明）
    if !natives.iter().any(|n| n.name == "println") {
        natives.push(HirFunction {
            name: "println".into(),
            params: vec![HirParam {
                name: "message".into(),
                ty: Some(HirType::Named("Any".into())),
            }],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }

    // P7.6: 注册 malloc/free 为原生函数
    if !natives.iter().any(|n| n.name == "malloc") {
        natives.push(HirFunction {
            name: "malloc".into(),
            params: vec![HirParam {
                name: "size".into(),
                ty: Some(HirType::Named("Int".into())),
            }],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }

    // P8.5: 注册 CString/CStr 为原生函数
    for &name in &["CString", "CStr"] {
        if !natives.iter().any(|n| n.name == name) {
            natives.push(HirFunction {
                name: name.into(),
                params: vec![HirParam {
                    name: "s".into(),
                    ty: Some(HirType::Named("String".into())),
                }],
                ret: Some(HirType::Pointer(Box::new(HirType::Named("Char".into())))),
                body: HirBlock { stmts: vec![] },
                is_native: true,
                type_params: vec![],
            });
        }
    }

    // P8.6: 注册 ptrIsNull/ptrToInt/intToPtr 为原生函数
    for &(name, ref params, ret) in &[
        ("ptrIsNull", vec![("p", "Pointer<Int>")], "Boolean"),
        ("ptrToInt", vec![("p", "Pointer<Int>")], "Int"),
        ("intToPtr", vec![("n", "Int")], "Pointer<Int>"),
    ] {
        if !natives.iter().any(|n| n.name == name) {
            natives.push(HirFunction {
                name: name.into(),
                params: params.iter().map(|&(pn, pt)| HirParam {
                    name: pn.into(),
                    ty: Some(HirType::from_ast_str(pt)),
                }).collect(),
                ret: Some(HirType::from_ast_str(ret)),
                body: HirBlock { stmts: vec![] },
                is_native: true,
                type_params: vec![],
            });
        }
    }

    // P8.7: 注册 makeCallback 为原生函数
    if !natives.iter().any(|n| n.name == "makeCallback") {
        natives.push(HirFunction {
            name: "makeCallback".into(),
            params: vec![HirParam {
                name: "f".into(),
                ty: Some(HirType::Named("Any".into())),
            }],
            ret: Some(HirType::Pointer(Box::new(HirType::Named("Int".into())))),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    if !natives.iter().any(|n| n.name == "free") {
        natives.push(HirFunction {
            name: "free".into(),
            params: vec![HirParam {
                name: "ptr".into(),
                ty: Some(HirType::Named("Any".into())),
            }],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }

    // ── P10: 并发运行时原生函数（aura.concurrent.* 命名空间）──
    // aura.concurrent.spawn(expr) — 创建新协程/Actor
    if !natives.iter().any(|n| n.name == "aura.concurrent.spawn") {
        natives.push(HirFunction {
            name: "aura.concurrent.spawn".into(),
            params: vec![HirParam {
                name: "expr".into(),
                ty: Some(HirType::Named("Any".into())),
            }],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.send(actor, msg) — 向 Actor 发送消息
    if !natives.iter().any(|n| n.name == "aura.concurrent.send") {
        natives.push(HirFunction {
            name: "aura.concurrent.send".into(),
            params: vec![
                HirParam { name: "actor".into(), ty: Some(HirType::Named("Int".into())) },
                HirParam { name: "msg".into(), ty: Some(HirType::Named("Any".into())) },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.ask(actor, msg) — 向 Actor 请求响应
    if !natives.iter().any(|n| n.name == "aura.concurrent.ask") {
        natives.push(HirFunction {
            name: "aura.concurrent.ask".into(),
            params: vec![
                HirParam { name: "actor".into(), ty: Some(HirType::Named("Int".into())) },
                HirParam { name: "msg".into(), ty: Some(HirType::Named("Any".into())) },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.newChannel(bound) — 创建 Channel
    if !natives.iter().any(|n| n.name == "aura.concurrent.newChannel") {
        natives.push(HirFunction {
            name: "aura.concurrent.newChannel".into(),
            params: vec![
                HirParam { name: "bound".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.channelSend(ch, val) — 发送值到 Channel
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelSend") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelSend".into(),
            params: vec![
                HirParam { name: "ch".into(), ty: Some(HirType::Named("Int".into())) },
                HirParam { name: "val".into(), ty: Some(HirType::Named("Any".into())) },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.channelRecv(ch) — 从 Channel 接收值（阻塞）
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelRecv") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelRecv".into(),
            params: vec![
                HirParam { name: "ch".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.channelTryRecv(ch) — 从 Channel 接收值（非阻塞）
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelTryRecv") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelTryRecv".into(),
            params: vec![
                HirParam { name: "ch".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.select(ch1, ch2) — select 多路复用（最多 2 通道）
    if !natives.iter().any(|n| n.name == "aura.concurrent.select") {
        natives.push(HirFunction {
            name: "aura.concurrent.select".into(),
            params: vec![
                HirParam { name: "ch1".into(), ty: Some(HirType::Named("Int".into())) },
                HirParam { name: "ch2".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.spawnActor(name) — 创建 Actor 实例（返回 actor ID）
    if !natives.iter().any(|n| n.name == "aura.concurrent.spawnActor") {
        natives.push(HirFunction {
            name: "aura.concurrent.spawnActor".into(),
            params: vec![
                HirParam { name: "name".into(), ty: Some(HirType::Named("String".into())) },
            ],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.supervise(parent, child) — 建立监督关系
    if !natives.iter().any(|n| n.name == "aura.concurrent.supervise") {
        natives.push(HirFunction {
            name: "aura.concurrent.supervise".into(),
            params: vec![
                HirParam { name: "parent".into(), ty: Some(HirType::Named("Int".into())) },
                HirParam { name: "child".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }
    // aura.concurrent.actorAlive(id) — 检查 Actor 是否存活
    if !natives.iter().any(|n| n.name == "aura.concurrent.actorAlive") {
        natives.push(HirFunction {
            name: "aura.concurrent.actorAlive".into(),
            params: vec![
                HirParam { name: "id".into(), ty: Some(HirType::Named("Int".into())) },
            ],
            ret: Some(HirType::Named("Boolean".into())),
            body: HirBlock { stmts: vec![] },
            is_native: true,
            type_params: vec![],
        });
    }

    // P9.11: 注册所有标准库函数为原生函数（使编译器能解析 module.method() 调用）
    for (name, params) in std_native_functions() {
        if !natives.iter().any(|n| n.name == name) {
            let param_defs: Vec<HirParam> = params
                .iter()
                .map(|(pn, pt)| HirParam {
                    name: (*pn).into(),
                    ty: Some(HirType::Named((*pt).into())),
                })
                .collect();
            let ret = params.iter().any(|(_, pt)| *pt != "Unit").then(|| HirType::Named("Any".into()));
            natives.push(HirFunction {
                name: name.into(),
                params: param_defs,
                ret,
                body: HirBlock { stmts: vec![] },
                is_native: true,
                type_params: vec![],
            });
        }
    }

    HirProgram {
        functions,
        structs,
        natives,
        constants,
    }
}

fn desugar_fn(f: &FnDecl) -> HirFunction {
    let mut body = match &f.body {
        Some(b) => desugar_block(b),
        None => HirBlock { stmts: vec![] },
    };
    // 表达式体函数：`fun f() = expr` 被解析为仅含一条表达式语句的块，
    // 需将该表达式作为返回值（Kotlin 语义：块末表达式即返回值）。
    if body.stmts.len() == 1 {
        if let HirStmt::Expr(e) = &body.stmts[0] {
            let e = e.clone();
            body.stmts = vec![HirStmt::Return(Some(e))];
        }
    }
    HirFunction {
        name: f.name.clone(),
        params: f
            .params
            .iter()
            .map(|p| HirParam {
                name: p.name.clone(),
                ty: HirType::from_ast_opt(&p.type_hint),
            })
            .collect(),
        ret: HirType::from_ast_opt(&f.return_type),
        body,
        is_native: false,
        type_params: f.type_params.iter().map(|t| t.name.clone()).collect(),
    }
}

fn desugar_block(b: &Expr) -> HirBlock {
    // 表达式位置上的块：`{ stmt* }` 或 `{ stmt*; lastExpr }`
    let stmts = match b {
        Expr::Block(stmts, _) => stmts,
        other => {
            return HirBlock {
                stmts: vec![desugar_expr_stmt(other)],
            };
        }
    };

    let mut out = Vec::new();
    for (i, s) in stmts.iter().enumerate() {
        let is_last = i + 1 == stmts.len();
        match s {
            Stmt::Block(inner, _) => {
                // 嵌套块作为独立块语句
                out.push(HirStmt::Block(desugar_block(&Expr::Block(
                    inner.clone(),
                    Span::single(0, 1, 1),
                ))));
            }
            _ => {
                let hir = desugar_stmt(s);
                // 块内最后一条纯表达式（无副作用）作为值被忽略——保持为 Expr 语句
                let _ = is_last;
                out.push(hir);
            }
        }
    }
    HirBlock { stmts: out }
}

fn desugar_stmt(s: &Stmt) -> HirStmt {
    match s {
        Stmt::Expr(e) => desugar_expr_stmt(e),
        Stmt::Val {
            name,
            type_hint,
            initializer,
            ..
        } => HirStmt::Val {
            name: name.clone(),
            ty: HirType::from_ast_opt(type_hint),
            init: initializer.as_ref().map(|e| desugar_expr(e)),
        },
        Stmt::Var {
            name,
            type_hint,
            initializer,
            ..
        } => HirStmt::Var {
            name: name.clone(),
            ty: HirType::from_ast_opt(type_hint),
            init: initializer.as_ref().map(|e| desugar_expr(e)),
        },
        Stmt::Destructure { patterns, expr, .. } => {
            // 近似：仅将首个模式绑定到表达式的值
            let init = desugar_expr(expr);
            if let Some(Expr::Ident(first, _)) = patterns.first() {
                HirStmt::Val {
                    name: first.clone(),
                    ty: None,
                    init: Some(init),
                }
            } else {
                HirStmt::Expr(init)
            }
        }
        Stmt::Block(inner, _) => HirStmt::Block(desugar_block(&Expr::Block(
            inner.clone(),
            Span::single(0, 1, 1),
        ))),
    }
}

/// 表达式位置上的语句（如 `if`/`while`/`for` 作为语句、赋值、调用、throw 等）
fn desugar_expr_stmt(e: &Expr) -> HirStmt {
    match e {
        Expr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => HirStmt::If {
            cond: desugar_expr(condition),
            then_b: desugar_block(then_branch),
            else_b: else_branch.as_ref().map(|e| desugar_block(e)),
        },
        Expr::While {
            condition, body, ..
        } => HirStmt::While {
            cond: desugar_expr(condition),
            body: desugar_block(body),
        },
        Expr::DoWhile {
            condition, body, ..
        } => {
            // do-while：body 至少执行一次，降级为 `while(true){ body; if(!cond) break }`
            let mut stmts = desugar_block(body).stmts;
            stmts.push(HirStmt::If {
                cond: HirExpr::Unary {
                    op: HirUnOp::Not,
                    operand: Box::new(desugar_expr(condition)),
                },
                then_b: HirBlock {
                    stmts: vec![HirStmt::Break],
                },
                else_b: None,
            });
            HirStmt::While {
                cond: HirExpr::Lit(Literal::Bool(true)),
                body: HirBlock { stmts },
            }
        }
        Expr::For {
            pattern,
            iterable,
            body,
            ..
        } => desugar_for(pattern, iterable, body),
        Expr::Assign { target, value, .. } => HirStmt::Assign {
            target: desugar_expr(target),
            value: desugar_expr(value),
        },
        Expr::Return { value, .. } => HirStmt::Return(value.as_ref().map(|e| desugar_expr(e))),
        Expr::Break { .. } => HirStmt::Break,
        Expr::Continue { .. } => HirStmt::Continue,
        Expr::Throw { value, .. } => HirStmt::Expr(HirExpr::Call {
            callee: "__throw".into(),
            args: vec![desugar_expr(value)],
        }),
        Expr::Try {
            block,
            catches: _,
            finally,
            ..
        } => {
            // 近似：try 块作为普通块执行（catch/finally 在 P4 中暂不建模）
            let mut stmts = desugar_block(block).stmts;
            if let Some(f) = finally {
                stmts.extend(desugar_block(f).stmts);
            }
            HirStmt::Block(HirBlock { stmts })
        }
        Expr::Defer { block, .. } => HirStmt::Defer(desugar_block(block)),
        Expr::Await { expr, .. } => HirStmt::Expr(desugar_expr(expr)),
        other => HirStmt::Expr(desugar_expr(other)),
    }
}

/// `for (pat in iterable) body` → `while` 循环 + 索引变量（范围）或迭代器调用
fn desugar_for(pattern: &Expr, iterable: &Expr, body: &Expr) -> HirStmt {
    // 仅支持 `Ident` 模式（最常见）；其余降级为占位
    let var_name = match pattern {
        Expr::Ident(n, _) => n.clone(),
        _ => "_".to_string(),
    };

    // 范围：`start..end` 或 `start..<end`
    if let Expr::Range {
        start,
        end,
        inclusive,
        ..
    } = iterable
    {
        let start_box = start
            .clone()
            .unwrap_or_else(|| Box::new(Expr::Literal(Literal::Int(0), Span::single(0, 1, 1))));
        let start_e = desugar_expr(&start_box);
        let end_box = end
            .clone()
            .unwrap_or_else(|| Box::new(Expr::Literal(Literal::Int(0), Span::single(0, 1, 1))));
        let end_e = desugar_expr(&end_box);
        let idx = format!("__for_idx_{}", var_name);
        let mut body_stmts = vec![HirStmt::Val {
            name: var_name.clone(),
            ty: None,
            init: Some(HirExpr::Var(idx.clone())),
        }];
        body_stmts.extend(desugar_block(body).stmts);
        body_stmts.push(HirStmt::Assign {
            target: HirExpr::Var(idx.clone()),
            value: HirExpr::Binary {
                op: HirBinOp::Add,
                lhs: Box::new(HirExpr::Var(idx.clone())),
                rhs: Box::new(HirExpr::Lit(Literal::Int(1))),
            },
        });
        // 循环条件：inclusive ? idx <= end : idx < end
        let cmp = if *inclusive {
            HirBinOp::Le
        } else {
            HirBinOp::Lt
        };
        return HirStmt::Block(HirBlock {
            stmts: vec![
                HirStmt::Var {
                    name: idx.clone(),
                    ty: None,
                    init: Some(start_e),
                },
                HirStmt::While {
                    cond: HirExpr::Binary {
                        op: cmp,
                        lhs: Box::new(HirExpr::Var(idx.clone())),
                        rhs: Box::new(end_e),
                    },
                    body: HirBlock { stmts: body_stmts },
                },
            ],
        });
    }

    // 迭代器形式（list/array）：通过内置 `__iter_*` 近似（VM 后续实现）
    let iter_e = desugar_expr(iterable);
    let idx = format!("__for_idx_{}", var_name);
    let len_call = HirExpr::Call {
        callee: "__size".into(),
        args: vec![iter_e.clone()],
    };
    let get_call = HirExpr::Call {
        callee: "__get".into(),
        args: vec![iter_e, HirExpr::Var(idx.clone())],
    };
    let mut body_stmts = vec![HirStmt::Val {
        name: var_name.clone(),
        ty: None,
        init: Some(get_call),
    }];
    body_stmts.extend(desugar_block(body).stmts);
    body_stmts.push(HirStmt::Assign {
        target: HirExpr::Var(idx.clone()),
        value: HirExpr::Binary {
            op: HirBinOp::Add,
            lhs: Box::new(HirExpr::Var(idx.clone())),
            rhs: Box::new(HirExpr::Lit(Literal::Int(1))),
        },
    });
    HirStmt::Block(HirBlock {
        stmts: vec![
            HirStmt::Var {
                name: idx.clone(),
                ty: None,
                init: Some(HirExpr::Lit(Literal::Int(0))),
            },
            HirStmt::While {
                cond: HirExpr::Binary {
                    op: HirBinOp::Lt,
                    lhs: Box::new(HirExpr::Var(idx.clone())),
                    rhs: Box::new(len_call),
                },
                body: HirBlock { stmts: body_stmts },
            },
        ],
    })
}

fn desugar_expr(e: &Expr) -> HirExpr {
    match e {
        Expr::Literal(l, _) => HirExpr::Lit(l.clone()),
        Expr::Ident(n, _) => HirExpr::Var(n.clone()),
        Expr::Binary { op, lhs, rhs, .. } => HirExpr::Binary {
            op: HirBinOp::from_ast(*op),
            lhs: Box::new(desugar_expr(lhs)),
            rhs: Box::new(desugar_expr(rhs)),
        },
        Expr::Unary { op, operand, .. } => {
            if *op == UnOp::Increment || *op == UnOp::Decrement {
                // ++/-- 作为语句语义，这里近似为自身值
                desugar_expr(operand)
            } else {
                HirExpr::Unary {
                    op: HirUnOp::from_ast(*op),
                    operand: Box::new(desugar_expr(operand)),
                }
            }
        }
        Expr::Call { callee, args, .. } => {
            let callee_name = match callee.as_ref() {
                Expr::Ident(n, _) => n.clone(),
                // 模块调用 `module.method(args)`：降级为 `module.method(args...)`
                // 与普通方法调用 `obj.method(args)` → `method(obj, args...)` 区分
                Expr::MemberAccess { object, name, .. } => {
                    // 检查是否为标准库模块调用（支持嵌套：aura.concurrent.spawn）
                    if let Expr::Ident(module_name, _) = object.as_ref() {
                        if is_std_module(module_name) {
                            let full_module = full_package_name(module_name);
                            return HirExpr::Call {
                                callee: format!("{}.{}", full_module, name),
                                args: args.iter().map(desugar_expr).collect(),
                            };
                        }
                    }
                    // 嵌套：aura.concurrent.spawn
                    if let Expr::MemberAccess { object: inner_obj, name: inner_name, .. } = object.as_ref() {
                        if let Expr::Ident(module_name, _) = inner_obj.as_ref() {
                            if is_std_module(&format!("aura.{}", inner_name)) {
                                let full_module = full_package_name(module_name);
                                let nested_name = format!("{}.{}", inner_name, name);
                                return HirExpr::Call {
                                    callee: format!("{}.{}", full_module, nested_name),
                                    args: args.iter().map(desugar_expr).collect(),
                                };
                            }
                        }
                    }
                    // 普通方法调用：降级为 method(obj, args...)
                    // 如果方法是内置方法，解析为完整原生函数名
                    let resolved_name = resolve_builtin_method(name).unwrap_or_else(|| name.clone());
                    let mut all_args = vec![desugar_expr(object)];
                    for a in args {
                        all_args.push(desugar_expr(a));
                    }
                    return HirExpr::Call {
                        callee: resolved_name,
                        args: all_args,
                    };
                }
                _ => "__call".to_string(),
            };
            HirExpr::Call {
                callee: callee_name,
                args: args.iter().map(desugar_expr).collect(),
            }
        }
        Expr::MemberAccess { object, name, .. } => HirExpr::Member {
            object: Box::new(desugar_expr(object)),
            name: name.clone(),
        },
        Expr::SafeAccess { object, name, .. } => {
            // 近似：安全调用降级为普通成员访问（完整空安全留待运行时）
            HirExpr::Member {
                object: Box::new(desugar_expr(object)),
                name: name.clone(),
            }
        }
        Expr::Index {
            container, index, ..
        } => HirExpr::Index {
            container: Box::new(desugar_expr(container)),
            index: Box::new(desugar_expr(index)),
        },
        Expr::New {
            type_name, args, ..
        } => {
            // P7.5: box expr → HirExpr::Box
            if type_name == "Box" && args.len() == 1 {
                return HirExpr::Box(Box::new(desugar_expr(&args[0])));
            }
            // P7.3: weak(ref) → HirExpr::WeakRef
            if type_name == "Weak" && args.len() == 1 {
                return HirExpr::WeakRef(Box::new(desugar_expr(&args[0])));
            }
            HirExpr::New {
                type_name: type_name.clone(),
                args: args.iter().map(desugar_expr).collect(),
            }
        }
        Expr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => HirExpr::If {
            cond: Box::new(desugar_expr(condition)),
            then_e: Box::new(desugar_block_or_expr(then_branch)),
            else_e: Box::new(match else_branch {
                Some(b) => desugar_block_or_expr(b),
                None => HirExpr::Lit(Literal::Null),
            }),
        },
        Expr::When { subject, arms, .. } => desugar_when(subject, arms),
        Expr::Block(_stmts, _) => HirExpr::Block(desugar_block(e)),
        Expr::Range { .. } => HirExpr::Call {
            callee: "__range".into(),
            args: vec![],
        },
        Expr::Elvis { lhs, rhs, .. } => {
            // `a ?: b` → if (a != null) a else b（用 EQ null 近似）
            HirExpr::If {
                cond: Box::new(HirExpr::Binary {
                    op: HirBinOp::Ne,
                    lhs: Box::new(desugar_expr(lhs)),
                    rhs: Box::new(HirExpr::Lit(Literal::Null)),
                }),
                then_e: Box::new(desugar_expr(lhs)),
                else_e: Box::new(desugar_expr(rhs)),
            }
        }
        Expr::AssertNonNull { expr, .. } => desugar_expr(expr),
        Expr::TypeCast { expr, .. } => desugar_expr(expr),
        Expr::Return { value, .. } => match value {
            Some(v) => desugar_expr(v),
            None => HirExpr::Lit(Literal::Null),
        },
        Expr::Lambda { .. } | Expr::Closure { .. } => HirExpr::Call {
            callee: "__lambda".into(),
            args: vec![],
        },
        Expr::Destructure { expr, .. } => desugar_expr(expr),
        Expr::Throw { value, .. } => HirExpr::Call {
            callee: "__throw".into(),
            args: vec![desugar_expr(value)],
        },
        Expr::Await { expr, .. } => HirExpr::Await(Box::new(desugar_expr(expr))),
        // P10.9: select 多路复用 — 降级为 `aura.concurrent.select(ch1, ch2)` 原生函数调用（最多 2 通道）
        Expr::Select { branches, .. } => {
            let ch_args: Vec<HirExpr> = branches.iter().take(2).map(|b| {
                match &b.pattern {
                    Expr::Call { callee, .. } => {
                        if let Expr::MemberAccess { object, .. } = callee.as_ref() {
                            desugar_expr(object)
                        } else {
                            desugar_expr(callee)
                        }
                    }
                    _ => desugar_expr(&b.pattern),
                }
            }).collect();
            // 补齐到 2 个参数
            let mut args = ch_args;
            while args.len() < 2 {
                args.push(HirExpr::Lit(Literal::Int(0)));
            }
            HirExpr::Call {
                callee: "aura.concurrent.select".into(),
                args,
            }
        }
        _other => HirExpr::Lit(Literal::Null), // 兜底（Try/Defer 等已在语句层处理）
    }
}

/// 将 `then/else` 分支（可能是块或表达式）统一为表达式
fn desugar_block_or_expr(e: &Expr) -> HirExpr {
    match e {
        Expr::Block(_, _) => HirExpr::Block(desugar_block(e)),
        other => desugar_expr(other),
    }
}

/// `when` 降级为嵌套 `if` 表达式
fn desugar_when(subject: &Option<Box<Expr>>, arms: &[WhenArm]) -> HirExpr {
    // 从后往前构造 else 链
    let mut result: Option<HirExpr> = None;
    for arm in arms.iter().rev() {
        // 跳过带守卫的复杂 arm（守卫在 P4 中近似忽略）
        let patterns = &arm.patterns;
        // 仅处理单模式（多模式用 `||` 近似，这里取第一个）
        let pat = patterns.first();
        let cond = match (subject, pat) {
            (Some(s), Some(p)) => {
                // 字面量/标识符模式 → 相等比较；`is T` → 类型检查内置
                match p {
                    Expr::Binary { op: BinOp::To, .. } => HirExpr::Lit(Literal::Bool(true)),
                    _ => HirExpr::Binary {
                        op: HirBinOp::Eq,
                        lhs: Box::new(desugar_expr(s)),
                        rhs: Box::new(desugar_expr(p)),
                    },
                }
            }
            (None, Some(p)) => desugar_expr(p), // 无 subject 时模式即条件
            _ => HirExpr::Lit(Literal::Bool(true)),
        };
        let then_e = desugar_block_or_expr(&arm.body);
        let else_e = match result.take() {
            Some(r) => r,
            None => HirExpr::Lit(Literal::Null),
        };
        result = Some(HirExpr::If {
            cond: Box::new(cond),
            then_e: Box::new(then_e),
            else_e: Box::new(else_e),
        });
    }
    result.unwrap_or(HirExpr::Lit(Literal::Null))
}

// ─────────────────────────────────────────────────────────────────────────────
// P9: 标准库模块检测与原生函数注册
// ─────────────────────────────────────────────────────────────────────────────

/// 解析内置方法名为完整原生函数名（如 `toString` → `aura.builtin.toString`）
fn resolve_builtin_method(name: &str) -> Option<String> {
    for (full_name, _) in std_native_functions() {
        if let Some(method_name) = full_name.split('.').last() {
            if method_name == name {
                return Some(full_name.to_string());
            }
        }
    }
    None
}

/// 判断名称是否为已知的标准库模块名（支持 aura. 前缀和短名）
fn is_std_module(name: &str) -> bool {
    // Kotlin 风格：aura.io, aura.math, ...
    if let Some(mod_name) = name.strip_prefix("aura.") {
        return matches!(
            mod_name,
            "io" | "math" | "string" | "collections" | "fs" | "net" | "json"
                | "time" | "test" | "builtin" | "env" | "process" | "random"
                | "encoding" | "ascii" | "console" | "path" | "assert" | "iter"
                | "concurrent"
        );
    }
    // 兼容短名：io, math, ...
    matches!(
        name,
        "io" | "math" | "string" | "collections" | "fs" | "net" | "json"
            | "time" | "test" | "builtin" | "env" | "process" | "random"
            | "encoding" | "ascii" | "console" | "path" | "assert" | "iter"
    )
}

/// 将模块名转换为完整包名（添加 aura. 前缀）
fn full_package_name(module: &str) -> String {
    if module == "aura" || module.starts_with("aura.") {
        module.to_string()
    } else {
        format!("aura.{}", module)
    }
}

/// 返回所有标准库原生函数的签名信息：(函数全名, [(参数名, 参数类型)])
fn std_native_functions() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        // ── std.io ──
        ("aura.io.println", vec![("msg", "String")]),
        ("aura.io.print", vec![("msg", "String")]),
        ("aura.io.readLine", vec![]),
        ("aura.io.readAll", vec![]),
        ("aura.io.flush", vec![]),
        ("aura.io.fileRead", vec![("path", "String")]),
        ("aura.io.fileWrite", vec![("path", "String"), ("content", "String")]),
        ("aura.io.writeFile", vec![("path", "String"), ("content", "String")]),
        ("aura.io.readFile", vec![("path", "String")]),
        ("aura.io.fileExists", vec![("path", "String")]),
        // ── std.math ──
        ("aura.math.abs", vec![("x", "Float")]),
        ("aura.math.min", vec![("a", "Int"), ("b", "Int")]),
        ("aura.math.max", vec![("a", "Int"), ("b", "Int")]),
        ("aura.math.ceil", vec![("x", "Float")]),
        ("aura.math.floor", vec![("x", "Float")]),
        ("aura.math.round", vec![("x", "Float")]),
        ("aura.math.trunc", vec![("x", "Float")]),
        ("aura.math.sqrt", vec![("x", "Float")]),
        ("aura.math.cbrt", vec![("x", "Float")]),
        ("aura.math.pow", vec![("base", "Float"), ("exp", "Float")]),
        ("aura.math.exp", vec![("x", "Float")]),
        ("aura.math.log", vec![("x", "Float")]),
        ("aura.math.log2", vec![("x", "Float")]),
        ("aura.math.log10", vec![("x", "Float")]),
        ("aura.math.sin", vec![("x", "Float")]),
        ("aura.math.cos", vec![("x", "Float")]),
        ("aura.math.tan", vec![("x", "Float")]),
        ("aura.math.asin", vec![("x", "Float")]),
        ("aura.math.acos", vec![("x", "Float")]),
        ("aura.math.atan", vec![("x", "Float")]),
        ("aura.math.atan2", vec![("y", "Float"), ("x", "Float")]),
        ("aura.math.PI", vec![]),
        ("aura.math.E", vec![]),
        ("aura.math.INT_MAX", vec![]),
        ("aura.math.INT_MIN", vec![]),
        ("aura.math.FLOAT_MAX", vec![]),
        ("aura.math.sign", vec![("x", "Float")]),
        ("aura.math.clamp", vec![("x", "Float"), ("lo", "Float"), ("hi", "Float")]),
        // ── std.string ──
        ("aura.string.contains", vec![("text", "String"), ("substr", "String")]),
        ("aura.string.startsWith", vec![("text", "String"), ("prefix", "String")]),
        ("aura.string.endsWith", vec![("text", "String"), ("suffix", "String")]),
        ("aura.string.split", vec![("text", "String"), ("sep", "String")]),
        ("aura.string.join", vec![("text", "String"), ("sep", "String")]),
        ("aura.string.replace", vec![("text", "String"), ("target", "String"), ("replacement", "String")]),
        ("aura.string.replaceAll", vec![("text", "String"), ("target", "String"), ("replacement", "String")]),
        ("aura.string.trim", vec![("text", "String")]),
        ("aura.string.trimStart", vec![("text", "String")]),
        ("aura.string.trimEnd", vec![("text", "String")]),
        ("aura.string.substring", vec![("text", "String"), ("start", "Int"), ("end", "Int")]),
        ("aura.string.substringBefore", vec![("text", "String"), ("sep", "String")]),
        ("aura.string.substringAfter", vec![("text", "String"), ("sep", "String")]),
        ("aura.string.toLowerCase", vec![("text", "String")]),
        ("aura.string.toUpperCase", vec![("text", "String")]),
        ("aura.string.length", vec![("text", "String")]),
        ("aura.string.isEmpty", vec![("text", "String")]),
        ("aura.string.format", vec![("template", "String")]),
        ("aura.string.repeat", vec![("n", "Int"), ("text", "String")]),
        ("aura.string.indexOf", vec![("text", "String"), ("substr", "String")]),
        ("aura.string.lastIndexOf", vec![("text", "String"), ("substr", "String")]),
        ("aura.string.padStart", vec![("text", "String"), ("length", "Int"), ("pad", "String")]),
        ("aura.string.padEnd", vec![("text", "String"), ("length", "Int"), ("pad", "String")]),
        ("aura.string.escape", vec![("text", "String")]),
        ("aura.string.unescape", vec![("text", "String")]),
        ("aura.string.splitLines", vec![("text", "String")]),
        ("aura.string.joinLines", vec![("text", "String")]),
        ("aura.string.countChar", vec![("text", "String"), ("char", "String")]),
        ("aura.string.first", vec![("text", "String")]),
        ("aura.string.last", vec![("text", "String")]),
        ("aura.string.isBlank", vec![("text", "String")]),
        ("aura.string.matches", vec![("text", "String"), ("regex", "String")]),
        ("aura.string.containsAny", vec![("text", "String"), ("patterns", "String")]),
        ("aura.string.containsAll", vec![("text", "String"), ("patterns", "String")]),
        // ── std.collections ──
        ("aura.collections.listOf", vec![]),
        ("aura.collections.mutableListOf", vec![]),
        ("aura.collections.emptyList", vec![]),
        ("aura.collections.arrayOf", vec![]),
        ("aura.collections.listContains", vec![("list", "List"), ("item", "Value")]),
        ("aura.collections.listIndexOf", vec![("list", "List"), ("item", "Value")]),
        ("aura.collections.listRemove", vec![("list", "List"), ("item", "Value")]),
        ("aura.collections.listReverse", vec![("list", "List")]),
        ("aura.collections.listSort", vec![("list", "List")]),
        ("aura.collections.listGet", vec![("list", "List"), ("index", "Int")]),
        ("aura.collections.listSet", vec![("list", "List"), ("index", "Int"), ("value", "Value")]),
        ("aura.collections.listInsert", vec![("list", "List"), ("index", "Int"), ("value", "Value")]),
        ("aura.collections.listSubList", vec![("list", "List"), ("from", "Int"), ("to", "Int")]),
        ("aura.collections.mapOf", vec![]),
        ("aura.collections.mutableMapOf", vec![]),
        ("aura.collections.emptyMap", vec![]),
        ("aura.collections.mapContains", vec![("map", "Map"), ("value", "Value")]),
        ("aura.collections.mapContainsKey", vec![("map", "Map"), ("key", "Value")]),
        ("aura.collections.mapContainsValue", vec![("map", "Map"), ("value", "Value")]),
        ("aura.collections.mapRemove", vec![("map", "Map"), ("key", "Value")]),
        ("aura.collections.mapKeys", vec![("map", "Map")]),
        ("aura.collections.mapValues", vec![("map", "Map")]),
        ("aura.collections.setOf", vec![]),
        ("aura.collections.mutableSetOf", vec![]),
        ("aura.collections.emptySet", vec![]),
        // ── std.fs ──
        ("aura.fs.exists", vec![("path", "String")]),
        ("aura.fs.isFile", vec![("path", "String")]),
        ("aura.fs.isDirectory", vec![("path", "String")]),
        ("aura.fs.readText", vec![("path", "String")]),
        ("aura.fs.writeText", vec![("path", "String"), ("content", "String")]),
        ("aura.fs.readBytes", vec![("path", "String")]),
        ("aura.fs.writeBytes", vec![("path", "String"), ("data", "List")]),
        ("aura.fs.delete", vec![("path", "String")]),
        ("aura.fs.mkdir", vec![("path", "String")]),
        ("aura.fs.mkdirP", vec![("path", "String")]),
        ("aura.fs.rename", vec![("old", "String"), ("new", "String")]),
        ("aura.fs.copy", vec![("src", "String"), ("dst", "String")]),
        ("aura.fs.listDir", vec![("path", "String")]),
        ("aura.fs.listFiles", vec![("path", "String")]),
        ("aura.fs.fileSize", vec![("path", "String")]),
        ("aura.fs.lastModified", vec![("path", "String")]),
        ("aura.fs.absolutePath", vec![("path", "String")]),
        ("aura.fs.homeDir", vec![]),
        ("aura.fs.tempDir", vec![]),
        ("aura.fs.currentDir", vec![]),
        ("aura.fs.walk", vec![("root", "String"), ("maxDepth", "Int")]),
        // ── std.net ──
        ("aura.net.tcpConnect", vec![("host", "String"), ("port", "Int")]),
        ("aura.net.tcpListen", vec![("port", "Int")]),
        ("aura.net.tcpSend", vec![("handle", "Int"), ("message", "String")]),
        ("aura.net.tcpRecv", vec![("handle", "Int")]),
        ("aura.net.tcpClose", vec![("handle", "Int")]),
        ("aura.net.udpSend", vec![("target", "String"), ("port", "Int"), ("message", "String")]),
        ("aura.net.udpRecv", vec![("handle", "Int")]),
        ("aura.net.udpClose", vec![("handle", "Int")]),
        ("aura.net.isHostReachable", vec![("host", "String")]),
        ("aura.net.getHostname", vec![]),
        ("aura.net.getLocalIp", vec![]),
        // ── std.json ──
        ("aura.json.parse", vec![("text", "String")]),
        ("aura.json.stringify", vec![("value", "Value"), ("pretty", "Bool")]),
        ("aura.json.isValid", vec![("text", "String")]),
        ("aura.json.get", vec![("obj", "Value"), ("key", "String")]),
        ("aura.json.set", vec![("obj", "Value"), ("key", "String"), ("value", "Value")]),
        ("aura.json.keys", vec![("obj", "Value")]),
        ("aura.json.values", vec![("obj", "Value")]),
        ("aura.json.length", vec![("obj", "Value")]),
        ("aura.json.contains", vec![("obj", "Value"), ("key", "String")]),
        ("aura.json.remove", vec![("obj", "Value"), ("key", "String")]),
        // ── std.time ──
        ("aura.time.now", vec![]),
        ("aura.time.epoch", vec![]),
        ("aura.time.currentTime", vec![]),
        ("aura.time.sleep", vec![("seconds", "Float")]),
        ("aura.time.duration", vec![("seconds", "Float")]),
        ("aura.time.toDateString", vec![("timestamp", "Int")]),
        ("aura.time.toTimeString", vec![("timestamp", "Int")]),
        ("aura.time.formatDate", vec![("timestamp", "Int"), ("pattern", "String")]),
        ("aura.time.diff", vec![("t1", "Float"), ("t2", "Float")]),
        ("aura.time.parseDate", vec![("text", "String")]),
        // ── std.test ──
        ("aura.test.assertTrue", vec![("condition", "Value"), ("message", "String")]),
        ("aura.test.assertFalse", vec![("condition", "Value"), ("message", "String")]),
        ("aura.test.assertEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.test.assertNotEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.test.assertNotNull", vec![("value", "Value"), ("message", "String")]),
        ("aura.test.assertNull", vec![("value", "Value"), ("message", "String")]),
        ("aura.test.assertContains", vec![("text", "String"), ("substr", "String"), ("message", "String")]),
        ("aura.test.assertNotContains", vec![("text", "String"), ("substr", "String"), ("message", "String")]),
        ("aura.test.assertThrows", vec![]),
        ("aura.test.assertGt", vec![("a", "Float"), ("b", "Float"), ("message", "String")]),
        ("aura.test.assertGte", vec![("a", "Float"), ("b", "Float"), ("message", "String")]),
        ("aura.test.assertLt", vec![("a", "Float"), ("b", "Float"), ("message", "String")]),
        ("aura.test.assertLte", vec![("a", "Float"), ("b", "Float"), ("message", "String")]),
        ("aura.test.assertApprox", vec![("a", "Float"), ("b", "Float"), ("epsilon", "Float"), ("message", "String")]),
        ("aura.test.assertArrayEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.test.assertMapEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.test.pass", vec![("message", "String")]),
        ("aura.test.fail", vec![("message", "String")]),
        // ── std.builtin ──
        ("aura.builtin.typeof", vec![("value", "Value")]),
        ("aura.builtin.typeOf", vec![("value", "Value")]),
        ("aura.builtin.isNull", vec![("value", "Value")]),
        ("aura.builtin.isNotNull", vec![("value", "Value")]),
        ("aura.builtin.isZero", vec![("value", "Value")]),
        ("aura.builtin.isPositive", vec![("value", "Value")]),
        ("aura.builtin.isNegative", vec![("value", "Value")]),
        ("aura.builtin.toString", vec![("value", "Value")]),
        ("aura.builtin.toInt", vec![("value", "Value")]),
        ("aura.builtin.toFloat", vec![("value", "Value")]),
        ("aura.builtin.toBool", vec![("value", "Value")]),
        ("aura.builtin.sizeOf", vec![("value", "Value")]),
        ("aura.builtin.hash", vec![("value", "Value")]),
        ("aura.builtin.compare", vec![("a", "Value"), ("b", "Value")]),
        ("aura.builtin.clone", vec![("value", "Value")]),
        ("aura.builtin.identity", vec![("value", "Value")]),
        // ── std.env ──
        ("aura.env.get", vec![("name", "String"), ("default", "String")]),
        ("aura.env.set", vec![("name", "String"), ("value", "String")]),
        ("aura.env.remove", vec![("name", "String")]),
        ("aura.env.has", vec![("name", "String")]),
        ("aura.env.keys", vec![]),
        ("aura.env.values", vec![]),
        ("aura.env.all", vec![]),
        ("aura.env.home", vec![]),
        ("aura.env.tmp", vec![]),
        ("aura.env.pwd", vec![]),
        ("aura.env.platform", vec![]),
        ("aura.env.os", vec![]),
        ("aura.env.arch", vec![]),
        // ── std.process ──
        ("aura.process.exit", vec![("code", "Int")]),
        ("aura.process.exitCode", vec![]),
        ("aura.process.args", vec![]),
        ("aura.process.arg", vec![("index", "Int")]),
        ("aura.process.argCount", vec![]),
        ("aura.process.pid", vec![]),
        ("aura.process.spawn", vec![("command", "String")]),
        ("aura.process.kill", vec![("pid", "Int")]),
        ("aura.process.wait", vec![("pid", "Int")]),
        ("aura.process.exitProcess", vec![("code", "Int")]),
        // ── std.random ──
        ("aura.random.nextInt", vec![]),
        ("aura.random.nextLong", vec![]),
        ("aura.random.nextFloat", vec![]),
        ("aura.random.nextDouble", vec![]),
        ("aura.random.nextBool", vec![]),
        ("aura.random.nextIntRange", vec![("min", "Int"), ("max", "Int")]),
        ("aura.random.nextFloatRange", vec![("min", "Float"), ("max", "Float")]),
        ("aura.random.choice", vec![]),
        ("aura.random.shuffle", vec![("list", "List")]),
        ("aura.random.seed", vec![]),
        ("aura.random.random", vec![]),
        // ── std.encoding ──
        ("aura.encoding.base64Encode", vec![("text", "String")]),
        ("aura.encoding.base64Decode", vec![("text", "String")]),
        ("aura.encoding.hexEncode", vec![("text", "String")]),
        ("aura.encoding.hexDecode", vec![("text", "String")]),
        ("aura.encoding.urlEncode", vec![("text", "String")]),
        ("aura.encoding.urlDecode", vec![("text", "String")]),
        ("aura.encoding.byteToHex", vec![("byte", "Int")]),
        ("aura.encoding.hexToByte", vec![("hex", "String")]),
        // ── std.ascii ──
        ("aura.ascii.isAlpha", vec![("text", "String")]),
        ("aura.ascii.isDigit", vec![("text", "String")]),
        ("aura.ascii.isAlphaNumeric", vec![("text", "String")]),
        ("aura.ascii.isWhitespace", vec![("text", "String")]),
        ("aura.ascii.isUpper", vec![("text", "String")]),
        ("aura.ascii.isLower", vec![("text", "String")]),
        ("aura.ascii.toUpper", vec![("text", "String")]),
        ("aura.ascii.toLower", vec![("text", "String")]),
        ("aura.ascii.codeAt", vec![("text", "String"), ("index", "Int")]),
        ("aura.ascii.charAt", vec![("text", "String"), ("index", "Int")]),
        ("aura.ascii.fromCode", vec![("code", "Int")]),
        ("aura.ascii.codePointAt", vec![("text", "String"), ("index", "Int")]),
        // ── std.console ──
        ("aura.console.clear", vec![]),
        ("aura.console.cursorUp", vec![("n", "Int")]),
        ("aura.console.cursorDown", vec![("n", "Int")]),
        ("aura.console.cursorLeft", vec![("n", "Int")]),
        ("aura.console.cursorRight", vec![("n", "Int")]),
        ("aura.console.cursorShow", vec![]),
        ("aura.console.cursorHide", vec![]),
        ("aura.console.reset", vec![]),
        ("aura.console.red", vec![("text", "String")]),
        ("aura.console.green", vec![("text", "String")]),
        ("aura.console.yellow", vec![("text", "String")]),
        ("aura.console.blue", vec![("text", "String")]),
        ("aura.console.magenta", vec![("text", "String")]),
        ("aura.console.cyan", vec![("text", "String")]),
        ("aura.console.white", vec![("text", "String")]),
        ("aura.console.bold", vec![("text", "String")]),
        ("aura.console.italic", vec![("text", "String")]),
        ("aura.console.underline", vec![("text", "String")]),
        ("aura.console.dim", vec![("text", "String")]),
        ("aura.console.inverse", vec![("text", "String")]),
        ("aura.console.size", vec![]),
        ("aura.console.width", vec![]),
        ("aura.console.height", vec![]),
        // ── std.path ──
        ("aura.path.join", vec![]),
        ("aura.path.dirname", vec![("path", "String")]),
        ("aura.path.basename", vec![("path", "String")]),
        ("aura.path.extname", vec![("path", "String")]),
        ("aura.path.relative", vec![("from", "String"), ("to", "String")]),
        ("aura.path.resolve", vec![("path", "String")]),
        ("aura.path.normalize", vec![("path", "String")]),
        ("aura.path.isAbsolute", vec![("path", "String")]),
        ("aura.path.isRelative", vec![("path", "String")]),
        ("aura.path.split", vec![("path", "String")]),
        ("aura.path.separators", vec![("path", "String")]),
        ("aura.path.fromUnix", vec![("path", "String")]),
        ("aura.path.fromWindows", vec![("path", "String")]),
        // ── std.assert ──
        ("aura.assert.assert", vec![("condition", "Value"), ("message", "String")]),
        ("aura.assert.assertTrue", vec![("condition", "Value"), ("message", "String")]),
        ("aura.assert.assertFalse", vec![("condition", "Value"), ("message", "String")]),
        ("aura.assert.assertEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.assert.assertNotEq", vec![("a", "Value"), ("b", "Value"), ("message", "String")]),
        ("aura.assert.assertNotNull", vec![("value", "Value"), ("message", "String")]),
        ("aura.assert.assertNull", vec![("value", "Value"), ("message", "String")]),
        ("aura.assert.debugAssert", vec![("condition", "Value"), ("message", "String")]),
        // ── std.iter ──
        ("aura.iter.sum", vec![("list", "List")]),
        ("aura.iter.avg", vec![("list", "List")]),
        ("aura.iter.min", vec![("list", "List")]),
        ("aura.iter.max", vec![("list", "List")]),
        ("aura.iter.product", vec![("list", "List")]),
        ("aura.iter.contains", vec![("list", "List"), ("item", "Value")]),
        ("aura.iter.indexOf", vec![("list", "List"), ("item", "Value")]),
        ("aura.iter.count", vec![("list", "List")]),
        ("aura.iter.every", vec![("list", "List"), ("predicate", "Value")]),
        ("aura.iter.some", vec![("list", "List"), ("predicate", "Value")]),
        ("aura.iter.flatMap", vec![("list", "List"), ("fn", "Value")]),
        ("aura.iter.zip", vec![]),
        ("aura.iter.unzip", vec![("list", "List")]),
        ("aura.iter.enumerate", vec![("list", "List")]),
        ("aura.iter.chain", vec![]),
        ("aura.iter.take", vec![("list", "List"), ("n", "Int")]),
        ("aura.iter.skip", vec![("list", "List"), ("n", "Int")]),
        ("aura.iter.dropWhile", vec![("list", "List"), ("predicate", "Value")]),
        ("aura.iter.takeWhile", vec![("list", "List"), ("predicate", "Value")]),
        ("aura.iter.distinct", vec![("list", "List")]),
        ("aura.iter.groupBy", vec![("list", "List"), ("keyFn", "Value")]),
        ("aura.iter.partition", vec![("list", "List"), ("predicate", "Value")]),
        ("aura.iter.fold", vec![("list", "List"), ("init", "Value"), ("fn", "Value")]),
        ("aura.iter.scan", vec![("list", "List"), ("init", "Value"), ("fn", "Value")]),
        ("aura.iter.toMap", vec![("list", "List"), ("keyFn", "Value"), ("valueFn", "Value")]),
        ("aura.iter.toList", vec![("value", "Value")]),
        ("aura.iter.range", vec![("from", "Int"), ("to", "Int")]),
        ("aura.iter.rangeTo", vec![("from", "Int"), ("to", "Int")]),
        ("aura.iter.rangeUntil", vec![("from", "Int"), ("to", "Int")]),
        ("aura.iter.repeatN", vec![("value", "Value"), ("count", "Int")]),
    ]
}
