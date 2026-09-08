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
use crate::codegen::opcode::{Const, FfiAbi};
use crate::span::Span;
use std::cell::RefCell;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// 导入解析映射（ImportResolution）
//
// 从 ImportDecl 构建，用于在 desugar_expr 中将短名/别名解析为完整原生函数名。
// 支持：
// - `import aura.concurrent.*` + `spawn(42)` → `aura.concurrent.spawn(42)`
// - `import aura.concurrent.spawn` + `spawn(42)` → `aura.concurrent.spawn(42)`
// - `import aura.concurrent.spawn as s` + `s(42)` → `aura.concurrent.spawn(42)`
// - `import aura.concurrent as cc` + `cc.spawn(42)` → `aura.concurrent.spawn(42)`
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct ImportResolution {
    /// 短名 → 完整原生函数名（通配/精确引入，无别名）
    /// 例如: "spawn" → "aura.concurrent.spawn"
    short_to_full: HashMap<String, String>,
    /// 别名 → 完整原生函数名（精确引入 + 别名）
    /// 例如: "s" → "aura.concurrent.spawn"
    alias_to_full: HashMap<String, String>,
    /// 别名 → 模块名（模块/通配导入 + 别名）
    /// 例如: "cc" → "aura.concurrent"
    alias_to_module: HashMap<String, String>,
}

impl ImportResolution {
    /// 查找短名映射（如 `spawn` → `aura.concurrent.spawn`）
    fn resolve_short_name(&self, name: &str) -> Option<&str> {
        self.short_to_full.get(name).map(|s| s.as_str())
    }
    /// 查找别名映射（如 `s` → `aura.concurrent.spawn`）
    fn resolve_alias(&self, name: &str) -> Option<&str> {
        self.alias_to_full.get(name).map(|s| s.as_str())
    }
    /// 查找模块别名（如 `cc` → `aura.concurrent`）
    fn resolve_module_alias(&self, name: &str) -> Option<&str> {
        self.alias_to_module.get(name).map(|s| s.as_str())
    }
}

thread_local! {
    static IMPORT_RESOLUTION: RefCell<Option<ImportResolution>> = RefCell::new(None);
    static TYPE_NAMES: RefCell<Vec<String>> = RefCell::new(Vec::new());
    /// sema 类型信息（表达式 span → 类型名），供接收者类型分派
    static SEMA_INFO: RefCell<Option<crate::sema::info::SemaInfo>> = const { RefCell::new(None) };
    /// 类/结构体成员表（AST 派生）
    static CLASS_TABLE: RefCell<HashMap<String, ClassEntry>> = RefCell::new(HashMap::new());
    /// 当前正在降级的类/结构体上下文（裸字段/裸方法改写用）
    static CLASS_CTX: RefCell<Option<ClassCtx>> = const { RefCell::new(None) };
    /// 当前正在降级的访问器属性名（`field` 上下文关键字改写用）
    static ACCESSOR_PROP: RefCell<Option<String>> = const { RefCell::new(None) };
    /// 函数参数表（函数名 → 参数列表），供默认参数填充和 vararg 打包
    static FUNCTION_PARAMS: RefCell<HashMap<String, Vec<HirParam>>> = RefCell::new(HashMap::new());
    /// extern interface 名称集合（用于识别接口方法调用）
    static INTERFACE_NAMES: RefCell<std::collections::HashSet<String>> = RefCell::new(std::collections::HashSet::new());
}

/// 判断名称是否为已知的结构体/类（用于检测构造器调用）
fn is_type_name(n: &str) -> bool {
    TYPE_NAMES.with(|r| r.borrow().iter().any(|t| t == n))
}

// ─────────────────────────────────────────────────────────────────────────────
// 类/结构体成员分派（P-K2）
//
// 让类方法/字段/companion/访问器/运算符在运行时真正工作：
// - 方法命名带类前缀 `Class.method`（消除跨类同名冲突）
// - 调用点按接收者静态类型分派（sema 类型信息 + 唯一候选兜底）
// - 类体裸字段 → `self.field`、裸方法调用 → `Class.method(self, ...)`
// - init 块/次构造函数合成 `Class.__ctorN`，构造调用时在字段赋值后执行
// - companion 成员合成 `Class.member` 函数，`Math.max(...)` / `Math.PI` 直达
// - 属性访问器合成 `Class.prop.get/set`，读写点改写为函数调用
// - `operator fun` 运算符重载：`a + b` → `Class.plus(a, b)`
// ─────────────────────────────────────────────────────────────────────────────

/// 类/结构体成员表条目（AST 派生）
#[derive(Default, Clone)]
struct ClassEntry {
    fields: Vec<String>,
    /// 字段名 → 字段类型（用于 AOT 字段访问）
    field_types: HashMap<String, HirType>,
    methods: std::collections::HashSet<String>,
    /// 声明为 open/abstract 的方法名（虚方法，动态分派）
    open_methods: std::collections::HashSet<String>,
    superclass: Option<String>,
    companion_methods: std::collections::HashSet<String>,
    companion_fields: std::collections::HashSet<String>,
    /// 属性名 → (有无 getter, 有无 setter)
    accessors: HashMap<String, (bool, bool)>,
    /// 运算符重载方法名（plus/minus/times/div/mod/eq/lt/gt/le/ge）
    operators: std::collections::HashSet<String>,
}

impl ClassEntry {
    /// 沿继承链收集全部字段名（祖先在前，自身在后）
    fn effective_fields(&self, table: &HashMap<String, ClassEntry>, start: &str) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        let mut cur = self.superclass.clone();
        while let Some(s) = cur {
            if let Some(e) = table.get(&s) {
                chain.push(s.clone());
                cur = e.superclass.clone();
            } else {
                break;
            }
        }
        let mut out = Vec::new();
        for a in chain.iter().rev() {
            if let Some(e) = table.get(a) {
                out.extend(e.fields.iter().cloned());
            }
        }
        out.extend(self.fields.iter().cloned());
        let _ = start;
        out
    }
}

/// 类/结构体方法体降级上下文
#[derive(Clone)]
struct ClassCtx {
    class: String,
    fields: Vec<String>,
    /// 作用域栈内的局部名（屏蔽同名字段）
    locals: Vec<std::collections::HashSet<String>>,
}

impl ClassCtx {
    fn is_local(&self, n: &str) -> bool {
        self.locals.iter().any(|s| s.contains(n))
    }
}

fn push_local_scope() {
    CLASS_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.locals.push(std::collections::HashSet::new());
        }
    });
}

fn pop_local_scope() {
    CLASS_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            ctx.locals.pop();
        }
    });
}

fn register_local(n: &str) {
    CLASS_CTX.with(|c| {
        if let Some(ctx) = c.borrow_mut().as_mut() {
            if let Some(top) = ctx.locals.last_mut() {
                top.insert(n.to_string());
            }
        }
    });
}

/// 裸标识符改写：类体中的字段访问 / 访问器中的 `field`
fn class_bare_ident(n: &str) -> Option<HirExpr> {
    // 访问器体内的 `field` → self.<prop>
    let acc = ACCESSOR_PROP.with(|a| a.borrow().clone());
    if let Some(prop) = acc {
        if n == "field" {
            return Some(HirExpr::Member {
                object: Box::new(HirExpr::Var("self".into())),
                name: prop,
            });
        }
    }
    let ctx = CLASS_CTX.with(|c| c.borrow().as_ref().cloned())?;
    if ctx.fields.iter().any(|f| f == n) && !ctx.is_local(n) {
        return Some(HirExpr::Member {
            object: Box::new(HirExpr::Var("self".into())),
            name: n.to_string(),
        });
    }
    None
}

/// 类上下文内的裸方法调用：`m(args)` → `Class.m(self, args)`（伴生方法不带 self）
fn bare_call_in_class(n: &str) -> Option<(String, bool)> {
    let ctx = CLASS_CTX.with(|c| c.borrow().as_ref().cloned())?;
    let table = CLASS_TABLE.with(|t| t.borrow().clone());
    let entry = table.get(&ctx.class)?;
    if entry.methods.contains(n) && !ctx.is_local(n) {
        return Some((format!("{}.{}", ctx.class, n), true));
    }
    if entry.companion_methods.contains(n) && !ctx.is_local(n) {
        return Some((format!("{}.{}", ctx.class, n), false));
    }
    None
}

/// 接收者静态类型（sema 优先；剥可空标记）
fn sema_expr_class(object: &Expr) -> Option<String> {
    SEMA_INFO.with(|s| {
        s.borrow().as_ref().and_then(|i| {
            let ty = i.expr_type(object)?;
            if i.expr_type(object).is_some() { Some(ty) } else { None }
        })
    })
}

/// 方法调用的接收者类解析：静态类型命中（含继承链）→ 全表唯一候选兜底
fn resolve_method_owner(object: &Expr, method: &str) -> Option<(String, ClassEntry)> {
    let table = CLASS_TABLE.with(|t| t.borrow().clone());
    if table.is_empty() {
        return None;
    }
    // 1) sema 静态类型 + 继承链查找
    if let Some(ty) = sema_expr_class(object) {
        let mut cur = Some(ty);
        while let Some(cn) = cur {
            if let Some(e) = table.get(&cn) {
                if e.methods.contains(method) {
                    return Some((cn, e.clone()));
                }
                cur = e.superclass.clone();
            } else {
                break;
            }
        }
        // 已知接收者类型但不是成员表中的类（如 List/String）→ 交给内置方法
        return None;
    }
    // 2) 兜底（无类型信息时）：整个成员表中唯一拥有该方法的类
    let cands: Vec<String> =
        table.iter().filter(|(_, e)| e.methods.contains(method)).map(|(n, _)| n.clone()).collect();
    if cands.len() == 1 {
        let n = cands.into_iter().next()?;
        let e = table.get(&n)?.clone();
        return Some((n, e));
    }
    None
}

/// companion 成员访问：`C.m(...)` / `C.f`（C 为类型名）→ 合成函数全名
fn companion_member_fn(class: &str, member: &str) -> Option<String> {
    let table = CLASS_TABLE.with(|t| t.borrow().clone());
    let e = table.get(class)?;
    if e.companion_methods.contains(member) || e.companion_fields.contains(member) {
        Some(format!("{}.{}", class, member))
    } else {
        None
    }
}

/// 访问器 getter 解析：`obj.prop` 且 prop 有 getter
fn resolve_accessor_get(object: &Expr, prop: &str) -> Option<String> {
    let table = CLASS_TABLE.with(|t| t.borrow().clone());
    if let Some(ty) = sema_expr_class(object) {
        // 已知接收者类型：仅当命中访问器时改写，否则交给 GetField/内置
        return table
            .get(&ty)
            .and_then(|e| e.accessors.get(prop).and_then(|(g, _)| g.then(|| ty.clone())));
    }
    // 唯一候选兜底（无类型信息时）
    let cands: Vec<String> = table
        .iter()
        .filter(|(_, e)| e.accessors.get(prop).map_or(false, |(g, _)| *g))
        .map(|(n, _)| n.clone())
        .collect();
    if cands.len() == 1 {
        return cands.into_iter().next();
    }
    None
}

/// 访问器 setter 解析：`obj.prop = v` 且 prop 有 setter
fn resolve_accessor_set(object: &Expr, prop: &str) -> Option<String> {
    let table = CLASS_TABLE.with(|t| t.borrow().clone());
    if let Some(ty) = sema_expr_class(object) {
        return table
            .get(&ty)
            .and_then(|e| e.accessors.get(prop).and_then(|(_, s)| s.then(|| ty.clone())));
    }
    let cands: Vec<String> = table
        .iter()
        .filter(|(_, e)| e.accessors.get(prop).map_or(false, |(_, s)| *s))
        .map(|(n, _)| n.clone())
        .collect();
    if cands.len() == 1 {
        return cands.into_iter().next();
    }
    None
}

/// 运算符重载：`a + b`（a 为重载类实例）→ `Class.plus(a, b)`
fn operator_method_for(op: BinOp, lhs: &Expr) -> Option<String> {
    let name = match op {
        BinOp::Add => "plus",
        BinOp::Sub => "minus",
        BinOp::Mul => "times",
        BinOp::Div => "div",
        BinOp::Mod => "mod",
        BinOp::Eq => "eq",
        BinOp::Ne => "ne",
        BinOp::Lt => "lt",
        BinOp::Gt => "gt",
        BinOp::Le => "le",
        BinOp::Ge => "ge",
        _ => return None,
    };
    let (class, entry) = resolve_method_owner(lhs, name)?;
    if entry.operators.contains(name) { Some(format!("{}.{}", class, name)) } else { None }
}

/// 从声明列表构建类/结构体成员表（owned，供 thread_local 使用）
fn build_class_table(program: &Program) -> HashMap<String, ClassEntry> {
    let mut table: HashMap<String, ClassEntry> = HashMap::new();
    for decl in &program.declarations {
        match decl {
            Decl::Class(c) => {
                let mut e = ClassEntry {
                    superclass: c.superclass.clone(),
                    ..Default::default()
                };
                for f in &c.fields {
                    e.fields.push(f.name.clone());
                    let default_ty = Box::new(Type::Named {
                        name: "Int".into(),
                        span: f.span,
                    });
                    let field_type = f.type_hint.as_ref().unwrap_or(&default_ty);
                    e.field_types.insert(f.name.clone(), HirType::from_ast(field_type));
                    if let Some(acc) = &f.accessors {
                        e.accessors
                            .insert(f.name.clone(), (acc.getter.is_some(), acc.setter.is_some()));
                    }
                }
                for m in &c.methods {
                    e.methods.insert(m.name.clone());
                    if m.modifiers.iter().any(|x| matches!(x, FnModifier::Operator)) {
                        e.operators.insert(m.name.clone());
                    }
                    if m.modifiers
                        .iter()
                        .any(|x| matches!(x, FnModifier::Open | FnModifier::Abstract))
                    {
                        e.open_methods.insert(m.name.clone());
                    }
                }
                for co in &c.companion_objects {
                    for m in &co.methods {
                        e.companion_methods.insert(m.name.clone());
                    }
                    for f in &co.fields {
                        e.companion_fields.insert(f.name.clone());
                    }
                }
                table.insert(c.name.clone(), e);
            }
            Decl::Struct(s) => {
                let mut e = ClassEntry::default();
                for f in &s.fields {
                    e.fields.push(f.name.clone());
                    let default_ty = Box::new(Type::Named {
                        name: "Int".into(),
                        span: f.span,
                    });
                    let field_type = f.type_hint.as_ref().unwrap_or(&default_ty);
                    e.field_types.insert(f.name.clone(), HirType::from_ast(field_type));
                    if let Some(acc) = &f.accessors {
                        e.accessors
                            .insert(f.name.clone(), (acc.getter.is_some(), acc.setter.is_some()));
                    }
                }
                for m in &s.methods {
                    e.methods.insert(m.name.clone());
                    if m.modifiers.iter().any(|x| matches!(x, FnModifier::Operator)) {
                        e.operators.insert(m.name.clone());
                    }
                    if m.modifiers
                        .iter()
                        .any(|x| matches!(x, FnModifier::Open | FnModifier::Abstract))
                    {
                        e.open_methods.insert(m.name.clone());
                    }
                }
                table.insert(s.name.clone(), e);
            }
            _ => {}
        }
    }
    table
}

/// 从声明列表构建函数参数表（函数名 → 参数列表），供默认参数填充和 vararg 打包
fn build_function_param_table(program: &Program) -> HashMap<String, Vec<HirParam>> {
    let mut table: HashMap<String, Vec<HirParam>> = HashMap::new();
    for decl in &program.declarations {
        match decl {
            Decl::Function(f) => {
                let params: Vec<HirParam> = f
                    .params
                    .iter()
                    .map(|p| HirParam {
                        name: p.name.clone(),
                        ty: HirType::from_ast_opt(&p.type_hint),
                        default_value: p.default_value.as_ref().map(|e| Box::new(desugar_expr(e))),
                        is_vararg: p.is_vararg,
                    })
                    .collect();
                table.insert(f.name.clone(), params);
            }
            Decl::Class(c) => {
                for m in &c.methods {
                    let params: Vec<HirParam> = m
                        .params
                        .iter()
                        .map(|p| HirParam {
                            name: p.name.clone(),
                            ty: HirType::from_ast_opt(&p.type_hint),
                            default_value: p
                                .default_value
                                .as_ref()
                                .map(|e| Box::new(desugar_expr(e))),
                            is_vararg: p.is_vararg,
                        })
                        .collect();
                    table.insert(format!("{}.{}", c.name, m.name), params);
                }
            }
            _ => {}
        }
    }
    table
}

/// tailrec 循环化：把块内尾自调用改写为参数重赋值 + continue
fn tailrec_rewrite(fname: &str, params: &[String], block: HirBlock) -> HirBlock {
    if let Some(stmts) = tailrec_stmts(fname, params, block.stmts.clone()) {
        return HirBlock {
            stmts: vec![
                HirStmt::While {
                    cond: HirExpr::Lit(Literal::Bool(true)),
                    body: HirBlock { stmts },
                },
            ],
        };
    }
    block
}

/// 递归改写块内语句；仅当发现尾自调用时返回 Some
fn tailrec_stmts(fname: &str, params: &[String], stmts: Vec<HirStmt>) -> Option<Vec<HirStmt>> {
    let mut any = false;
    let mut out = Vec::with_capacity(stmts.len());
    for s in stmts {
        match s {
            // return f(args)（尾位置自调用）→ 参数重赋值 + continue
            HirStmt::Return(Some(HirExpr::Call {
                callee,
                args,
            })) if callee == fname && args.len() == params.len() => {
                any = true;
                // 先求值全部实参（避免参数顺序覆盖）
                for (j, a) in args.into_iter().enumerate() {
                    out.push(HirStmt::Val {
                        name: format!("__tr{}", j),
                        ty: None,
                        init: Some(a),
                    });
                }
                for (i, p) in params.iter().enumerate() {
                    out.push(HirStmt::Assign {
                        target: HirExpr::Var(p.clone()),
                        value: HirExpr::Var(format!("__tr{}", i)),
                    });
                }
                out.push(HirStmt::Continue);
            }
            // return if (c) A else f(args) / return if (c) f(args) else A
            HirStmt::Return(Some(HirExpr::If {
                cond,
                then_e,
                else_e,
            })) => {
                let t_tail = tail_call_args(fname, params.len(), &then_e);
                let e_tail = tail_call_args(fname, params.len(), &else_e);
                if t_tail.is_some() || e_tail.is_some() {
                    any = true;
                    let (loop_stmts) =
                        build_tail_if(fname, params, *cond, *then_e, t_tail, *else_e, e_tail);
                    out.push(HirStmt::While {
                        cond: HirExpr::Lit(Literal::Bool(true)),
                        body: HirBlock {
                            stmts: loop_stmts,
                        },
                    });
                } else {
                    out.push(HirStmt::Return(Some(HirExpr::If {
                        cond,
                        then_e,
                        else_e,
                    })));
                }
            }
            // if 语句：递归处理分支块
            HirStmt::If {
                cond,
                then_b,
                else_b,
            } => {
                let (tb, tchg) = tailrec_block(fname, params, then_b);
                let (eb, echg) = match else_b {
                    Some(b) => {
                        let (b, ch) = tailrec_block(fname, params, b);
                        (Some(b), ch)
                    }
                    None => (None, false),
                };
                any |= tchg || echg;
                out.push(HirStmt::If {
                    cond,
                    then_b: tb,
                    else_b: eb,
                });
            }
            other => out.push(other),
        }
    }
    any.then_some(out)
}

fn tailrec_block(fname: &str, params: &[String], b: HirBlock) -> (HirBlock, bool) {
    match tailrec_stmts(fname, params, b.stmts.clone()) {
        Some(stmts) => (HirBlock { stmts }, true),
        None => (b, false),
    }
}

/// 若 e 是对 fname 的自调用且实参数匹配，返回实参列表
fn tail_call_args(fname: &str, arity: usize, e: &HirExpr) -> Option<Vec<HirExpr>> {
    match e {
        HirExpr::Call {
            callee,
            args,
        } if callee == fname && args.len() == arity => Some(args.clone()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_tail_if(
    _fname: &str,
    params: &[String],
    cond: HirExpr,
    then_e: HirExpr,
    t_tail: Option<Vec<HirExpr>>,
    else_e: HirExpr,
    e_tail: Option<Vec<HirExpr>>,
) -> Vec<HirStmt> {
    let mut out = Vec::new();
    // 生成「参数重赋值 + continue」语句序列
    let emit_continue = |out: &mut Vec<HirStmt>, args: Vec<HirExpr>| {
        for (j, a) in args.into_iter().enumerate() {
            out.push(HirStmt::Val {
                name: format!("__tr{}", j),
                ty: None,
                init: Some(a),
            });
        }
        for (i, p) in params.iter().enumerate() {
            out.push(HirStmt::Assign {
                target: HirExpr::Var(p.clone()),
                value: HirExpr::Var(format!("__tr{}", i)),
            });
        }
        out.push(HirStmt::Continue);
    };
    let then_b = match t_tail {
        Some(args) => {
            let mut s = Vec::new();
            emit_continue(&mut s, args);
            HirBlock { stmts: s }
        }
        None => HirBlock {
            stmts: vec![HirStmt::Return(Some(then_e))],
        },
    };
    let else_b = match e_tail {
        Some(args) => {
            let mut s = Vec::new();
            emit_continue(&mut s, args);
            Some(HirBlock { stmts: s })
        }
        None => Some(HirBlock {
            stmts: vec![HirStmt::Return(Some(else_e))],
        }),
    };
    out.push(HirStmt::If {
        cond,
        then_b,
        else_b,
    });
    out
}

/// 从 ImportDecl 列表构建 ImportResolution
fn build_import_resolution(imports: &[ImportDecl]) -> ImportResolution {
    let mut r = ImportResolution::default();

    for imp in imports {
        let path = &imp.path;
        // 非 aura.* 命名空间跳过
        if !path.starts_with("aura.") {
            continue;
        }

        // 判断路径深度：aura.xxx = 2（模块），aura.xxx.yyy = 3（函数）
        let parts: Vec<&str> = path.split('.').collect();
        let is_function = parts.len() == 3;

        match &imp.alias {
            Some(alias) if is_function => {
                // import aura.concurrent.spawn as s → alias "s" → full name
                r.alias_to_full.insert(alias.clone(), path.clone());
            }
            Some(alias) => {
                // import aura.concurrent as cc / import aura.concurrent.* as cc
                r.alias_to_module.insert(alias.clone(), path.clone());
            }
            None if imp.wildcard => {
                // import aura.concurrent.* → 所有函数短名 → 完整名
                let short_names = crate::std::decl::module_functions(path);
                for sn in short_names {
                    let full = format!("{}.{}", path, sn);
                    r.short_to_full.insert(sn, full);
                }
            }
            None if is_function => {
                // import aura.concurrent.spawn → 短名 spawn → 完整名
                let short_name = parts.last().unwrap_or(&"").to_string();
                r.short_to_full.insert(short_name, path.clone());
            }
            None => {
                // import aura.concurrent → 模块引用，无需映射（调用时用完整路径）
            }
        }
    }

    r
}

/// HIR 类型（降级阶段仅保留最简单的形式：基本类型名 / 命名类型）
#[derive(Debug, Clone, PartialEq)]
pub enum HirType {
    /// 基本/命名类型名（如 `Int`、`String`、`Player`）
    Named(String),
    /// 可空包装
    Nullable(Box<HirType>),
    /// 原始指针类型（P8.6）：`Pointer<T>` 映射为 C 的 `T*`
    Pointer(Box<HirType>),
    /// 函数类型（Fix 3）：`(A, B) -> R`
    Function { params: Box<Vec<HirType>>, return_type: Box<HirType> },
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
            // Fix 3: 函数类型
            Type::Function {
                params,
                return_type,
                ..
            } => HirType::Function {
                params: Box::new(
                    params
                        .iter()
                        .map(|p| HirType::from_ast_opt(&p.type_hint).unwrap_or(HirType::Unknown))
                        .collect(),
                ),
                return_type: Box::new(
                    HirType::from_ast_opt(return_type).unwrap_or(HirType::Unknown),
                ),
            },
            // Fix 4: 防御性处理 Type::Generic 中 Pointer<T> 的降级路径
            Type::Generic {
                name, args, ..
            } if name == "Pointer" && args.len() == 1 => {
                HirType::Pointer(Box::new(HirType::from_ast(&args[0])))
            }
            // 参数化类型：List<Int>, Map<String, Int> 等 → Named("List<Int>")
            Type::Generic {
                name, args, ..
            } => {
                let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
                HirType::Named(format!("{}<{}>", name, args_str.join(", ")))
            }
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
    /// 闭包/lambda（Fix 4）：`fun(x: Int) => x * 2` 或 `fun(x: Int) { return x * 2 }`
    Lambda {
        params: Vec<HirParam>,
        body: HirBlock,
    },
    /// 虚方法调用（P-K2）：open 方法经接收者运行时类型分派。
    /// `args[0]` 为接收者（self），发射时额外再压一次接收者供 vtable 查找。
    CallVirtual {
        recv: Box<HirExpr>,
        name: String,
        args: Vec<HirExpr>,
    },
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
    /// 默认值表达式（当调用方未提供该参数时使用）
    pub default_value: Option<Box<HirExpr>>,
    /// 是否为 vararg 可变参数
    pub is_vararg: bool,
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
    /// FFI ABI 标记（仅 `is_native == true` 时有意义）
    pub ffi_abi: FfiAbi,
    /// FFI 库名（对应 `extern "<abi>" "<lib>"`）
    pub ffi_lib: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirStruct {
    pub name: String,
    pub fields: Vec<(String, HirType)>,
    /// 字段默认值（与 fields 一一对应，None 表示无默认值）
    pub default_values: Vec<Option<Box<HirExpr>>>,
    /// 合成构造函数（按实参数量）：`Class.__ctorN`，New 降级时在字段默认值之后调用。
    /// 空 = 保持既有位置式字段赋值行为（struct / 无构造器类）。
    pub synth_ctors: Vec<usize>,
    /// 父类名（仅 class；struct 为 None）
    pub superclass: Option<String>,
    /// 本类声明为 open/abstract 的方法名（虚方法表槽位来源）
    pub virtual_methods: Vec<String>,
    /// 是否为引用语义类（class）；struct 为 false
    pub is_class: bool,
}

/// HIR 枚举定义（Fix 2 — 补全枚举支持）
#[derive(Debug, Clone, PartialEq)]
pub struct HirEnum {
    pub name: String,
    /// 变体列表：(变体名, 关联值类型列表)
    pub variants: Vec<(String, Vec<HirType>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirProgram {
    pub functions: Vec<HirFunction>,
    pub structs: Vec<HirStruct>,
    /// 枚举定义（Fix 2）
    pub enums: Vec<HirEnum>,
    /// 原生函数签名集合（extern "c" / 内置）
    pub natives: Vec<HirFunction>,
    /// FFI 常量（P8.1）：extern 块中的 `val` 声明
    pub constants: Vec<(String, Const)>,
    /// 顶层语句（脚本模式：无 main 时，顶层语句会被包装为隐式 main）
    pub top_level_statements: Option<HirBlock>,
    /// 类型别名表：别名 → 目标类型（用于 AOT 解析 typealias）
    pub type_aliases: HashMap<String, HirType>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 降级：AST → HIR
// ─────────────────────────────────────────────────────────────────────────────

/// 降级入口（无 sema 类型信息；接收者分派退化为唯一候选启发式）
pub fn desugar_program(program: &Program) -> HirProgram {
    desugar_program_with(program, None)
}

/// 降级入口（带 sema 类型信息：方法调用/运算符重载/访问器按接收者静态类型分派）
pub fn desugar_program_with(
    program: &Program,
    info: Option<&crate::sema::info::SemaInfo>,
) -> HirProgram {
    // 建立类/结构体成员表并挂到 thread_local
    let table = build_class_table(program);
    CLASS_TABLE.with(|t| *t.borrow_mut() = table);
    SEMA_INFO.with(|s| *s.borrow_mut() = info.cloned());
    let result = desugar_program_impl(program);
    // 清理 thread_local，避免跨编译残留
    CLASS_TABLE.with(|t| t.borrow_mut().clear());
    SEMA_INFO.with(|s| *s.borrow_mut() = None);
    CLASS_CTX.with(|c| *c.borrow_mut() = None);
    ACCESSOR_PROP.with(|a| *a.borrow_mut() = None);
    FUNCTION_PARAMS.with(|f| f.borrow_mut().clear());
    result
}

fn desugar_program_impl(program: &Program) -> HirProgram {
    // 构建导入解析映射（供 desugar_expr 解析短名/别名调用）
    let resolution = build_import_resolution(&program.imports);
    IMPORT_RESOLUTION.with(|r| {
        *r.borrow_mut() = Some(resolution);
    });

    // 预收集所有类型名（供 desugar_expr 检测构造器调用）
    let type_names: Vec<String> = program
        .declarations
        .iter()
        .flat_map(|d| match d {
            Decl::Struct(s) => Some(s.name.clone()),
            Decl::Class(c) => Some(c.name.clone()),
            Decl::Enum(e) => Some(e.name.clone()),
            Decl::Actor(a) => Some(a.name.clone()),
            _ => None,
        })
        .collect();
    TYPE_NAMES.with(|r| {
        *r.borrow_mut() = type_names;
    });

    // 预收集所有函数参数（供默认参数填充和 vararg 打包）
    let fn_params = build_function_param_table(program);
    FUNCTION_PARAMS.with(|f| *f.borrow_mut() = fn_params);

    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut natives = Vec::new();
    let mut constants = Vec::new();
    let mut top_level_stmts = Vec::new();
    let mut type_aliases = HashMap::new();
    let ast_classes: std::collections::HashMap<String, &ClassDecl> = program
        .declarations
        .iter()
        .filter_map(|d| match d {
            Decl::Class(c) => Some((c.name.clone(), c)),
            _ => None,
        })
        .collect();

    // 收集顶层语句（脚本模式）
    for stmt in &program.top_level_statements {
        let hir_stmt = desugar_stmt(stmt);
        top_level_stmts.push(hir_stmt);
    }

    for decl in &program.declarations {
        match decl {
            Decl::Function(f) => functions.push(desugar_fn(f)),
            Decl::Struct(s) => {
                structs.push(HirStruct {
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
                    default_values: s
                        .fields
                        .iter()
                        .map(|f| f.default_value.as_ref().map(|e| Box::new(desugar_expr(e))))
                        .collect(),
                    synth_ctors: Vec::new(),
                    superclass: None,
                    virtual_methods: Vec::new(),
                    is_class: false,
                });
                // struct 方法：类前缀命名 + 类上下文（裸字段/裸方法改写，修复运行时分派）
                for m in &s.methods {
                    functions.push(desugar_class_method(m, &s.name, true));
                }
                // 属性访问器合成（get/set 函数）
                functions.extend(synthesize_accessors(&s.name, &s.fields));
            }
            Decl::Extern(e) => {
                // P8-Rust: 根据 abi 字符串确定 FFI ABI 类型
                let abi = match e.abi.as_str() {
                    "rust" | "Rust" => FfiAbi::Rust,
                    _ => FfiAbi::C,
                };
                for f in &e.functions {
                    natives.push(HirFunction {
                        name: f.name.clone(),
                        params: f
                            .params
                            .iter()
                            .map(|p| HirParam {
                                name: p.name.clone(),
                                ty: HirType::from_ast_opt(&p.type_hint),
                                default_value: p
                                    .default_value
                                    .as_ref()
                                    .map(|e| Box::new(desugar_expr(e))),
                                is_vararg: p.is_vararg,
                            })
                            .collect(),
                        ret: HirType::from_ast_opt(&f.return_type),
                        body: HirBlock {
                            stmts: vec![],
                        },
                        is_native: true,
                        type_params: vec![],
                        /* ffi fields set below */
                        ffi_abi: abi,
                        ffi_lib: e.library.clone(),
                    });
                }
                // P8.1: 处理 extern 块中的常量
                for stmt in &e.constants {
                    if let Stmt::Val {
                        name,
                        initializer,
                        ..
                    } = stmt
                    {
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
            Decl::ExternInterface(e) => {
                // extern interface: 绑定到 AOT 动态库的函数接口
                INTERFACE_NAMES.with(|n| n.borrow_mut().insert(e.name.clone()));
                for f in &e.functions {
                    if f.name == "loadLibrary" {
                        continue; // loadLibrary 是内部方法，不生成原生函数
                    }
                    natives.push(HirFunction {
                        name: format!("{}.{}", e.name, f.name),
                        params: f
                            .params
                            .iter()
                            .map(|p| HirParam {
                                name: p.name.clone(),
                                ty: HirType::from_ast_opt(&p.type_hint),
                                default_value: p
                                    .default_value
                                    .as_ref()
                                    .map(|e| Box::new(desugar_expr(e))),
                                is_vararg: p.is_vararg,
                            })
                            .collect(),
                        ret: HirType::from_ast_opt(&f.return_type),
                        body: HirBlock {
                            stmts: vec![],
                        },
                        is_native: true,
                        type_params: vec![],
                        ffi_abi: FfiAbi::Aura,
                        ffi_lib: e.lib_path.clone(),
                    });
                }
            }
            // class：保留类型布局（字段），方法展开为独立函数
            // value class 与 class 在 HIR 层面均映射为 HirStruct（值布局）
            Decl::Class(c) => {
                // 有效字段 = 继承链祖先字段 + 自身字段（祖先在前，供位置式构造兼容）
                let mut eff_fields: Vec<(String, HirType, Option<&Expr>)> = Vec::new();
                let mut chain: Vec<&ClassDecl> = Vec::new();
                let mut cur = c.superclass.clone();
                while let Some(sn) = cur {
                    if let Some(sc) = ast_classes.get(&sn) {
                        chain.push(sc);
                        cur = sc.superclass.clone();
                    } else {
                        break;
                    }
                }
                for sc in chain.iter().rev() {
                    for f in &sc.fields {
                        eff_fields.push((
                            f.name.clone(),
                            HirType::from_ast_opt(&f.type_hint).unwrap_or(HirType::Unknown),
                            f.default_value.as_deref(),
                        ));
                    }
                }
                for f in &c.fields {
                    eff_fields.push((
                        f.name.clone(),
                        HirType::from_ast_opt(&f.type_hint).unwrap_or(HirType::Unknown),
                        f.default_value.as_deref(),
                    ));
                }
                structs.push(HirStruct {
                    name: c.name.clone(),
                    fields: eff_fields.iter().map(|(n, t, _)| (n.clone(), t.clone())).collect(),
                    default_values: eff_fields
                        .iter()
                        .map(|(_, _, dv)| dv.map(|e| Box::new(desugar_expr(e))))
                        .collect(),
                    synth_ctors: {
                        let mut arities: Vec<usize> =
                            c.constructors.iter().map(|x| x.params.len()).collect();
                        // init 块存在且无 0 参显式构造函数 → 合成 __ctor0（仅执行 init 块，
                        // Kotlin 语义：init 块随构造执行）
                        if !c.init_blocks.is_empty()
                            && !c.constructors.iter().any(|x| x.params.is_empty())
                        {
                            arities.push(0);
                        }
                        arities
                    },
                    superclass: c.superclass.clone(),
                    virtual_methods: c
                        .methods
                        .iter()
                        .filter(|m| {
                            m.modifiers
                                .iter()
                                .any(|x| matches!(x, FnModifier::Open | FnModifier::Abstract))
                        })
                        .map(|m| m.name.clone())
                        .collect(),
                    is_class: true,
                });
                // 方法：`Class.method` 命名 + 类上下文（裸字段/裸方法改写）
                for m in &c.methods {
                    functions.push(desugar_class_method(m, &c.name, true));
                }
                // companion 成员：方法 → `Class.name`（无 self）；字段 → 零参读取函数
                for co in &c.companion_objects {
                    for m in &co.methods {
                        functions.push(desugar_class_method(m, &c.name, false));
                    }
                    for f in &co.fields {
                        let init = f
                            .default_value
                            .as_ref()
                            .map(|e| desugar_expr(e))
                            .unwrap_or(HirExpr::Lit(Literal::Null));
                        functions.push(HirFunction {
                            name: format!("{}.{}", c.name, f.name),
                            params: vec![],
                            ret: HirType::from_ast_opt(&f.type_hint),
                            body: HirBlock {
                                stmts: vec![HirStmt::Return(Some(init))],
                            },
                            is_native: false,
                            type_params: vec![],
                            ffi_abi: FfiAbi::None,
                            ffi_lib: None,
                        });
                    }
                }
                // 次构造函数 / init 构造函数 → `Class.__ctorN`（N = 实参数量）
                // 构造函数体/init 块在类上下文中降级（裸字段 → self.field）
                let ctor_fields = {
                    let table = CLASS_TABLE.with(|t| t.borrow().clone());
                    table
                        .get(&c.name)
                        .map(|e| e.effective_fields(&table, &c.name))
                        .unwrap_or_default()
                };
                let prev_ctor_ctx = CLASS_CTX.with(|cell| {
                    let prev = cell.borrow().clone();
                    *cell.borrow_mut() = Some(ClassCtx {
                        class: c.name.clone(),
                        fields: ctor_fields,
                        locals: vec![std::collections::HashSet::new()],
                    });
                    prev
                });
                for ctor in &c.constructors {
                    let arity = ctor.params.len();
                    let mut body_stmts: Vec<HirStmt> = Vec::new();
                    // 委托调用：super(...)/this(...) → 对应 __ctorN(self, args)
                    if let Some(d) = &ctor.delegation {
                        let target_class = match d.target {
                            CtorDelegationTarget::Super => c.superclass.clone(),
                            CtorDelegationTarget::This => Some(c.name.clone()),
                        };
                        if let Some(tc) = target_class {
                            let mut dargs = vec![HirExpr::Var("self".into())];
                            for a in &d.args {
                                dargs.push(desugar_expr(a));
                            }
                            body_stmts.push(HirStmt::Expr(HirExpr::Call {
                                callee: format!("{}.__ctor{}", tc, d.args.len()),
                                args: dargs,
                            }));
                        }
                    }
                    // init 块先于构造函数体执行
                    for b in &c.init_blocks {
                        body_stmts.extend(desugar_block(b).stmts);
                    }
                    if let Some(b) = &ctor.body {
                        body_stmts.extend(desugar_block(b).stmts);
                    }
                    let mut params = vec![HirParam {
                        name: "self".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }];
                    params.extend(ctor.params.iter().map(|p| HirParam {
                        name: p.name.clone(),
                        ty: HirType::from_ast_opt(&p.type_hint),
                        default_value: p.default_value.as_ref().map(|e| Box::new(desugar_expr(e))),
                        is_vararg: p.is_vararg,
                    }));
                    functions.push(HirFunction {
                        name: format!("{}.__ctor{}", c.name, arity),
                        params,
                        ret: Some(HirType::Named("Unit".into())),
                        body: HirBlock {
                            stmts: body_stmts,
                        },
                        is_native: false,
                        type_params: vec![],
                        ffi_abi: FfiAbi::None,
                        ffi_lib: None,
                    });
                }
                // 仅 init 块（无 0 参显式构造函数）→ 合成 `Class.__ctor0`
                if !c.init_blocks.is_empty() && !c.constructors.iter().any(|x| x.params.is_empty())
                {
                    let mut body_stmts: Vec<HirStmt> = Vec::new();
                    for b in &c.init_blocks {
                        body_stmts.extend(desugar_block(b).stmts);
                    }
                    functions.push(HirFunction {
                        name: format!("{}.__ctor0", c.name),
                        params: vec![HirParam {
                            name: "self".into(),
                            ty: Some(HirType::Named("Any".into())),
                            default_value: None,
                            is_vararg: false,
                        }],
                        ret: Some(HirType::Named("Unit".into())),
                        body: HirBlock {
                            stmts: body_stmts,
                        },
                        is_native: false,
                        type_params: vec![],
                        ffi_abi: FfiAbi::None,
                        ffi_lib: None,
                    });
                }
                // 属性访问器合成
                CLASS_CTX.with(|cell| *cell.borrow_mut() = prev_ctor_ctx);
                functions.extend(synthesize_accessors(&c.name, &c.fields));
            }
            Decl::Interface(_) => {}
            Decl::Enum(e) => {
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        (
                            v.name.clone(),
                            v.fields
                                .iter()
                                .map(|f| {
                                    HirType::from_ast_opt(&f.type_hint).unwrap_or(HirType::Unknown)
                                })
                                .collect(),
                        )
                    })
                    .collect();
                enums.push(HirEnum {
                    name: e.name.clone(),
                    variants,
                });
            }
            Decl::Actor(a) => {
                for m in &a.methods {
                    functions.push(desugar_fn(m));
                }
            }
            Decl::TypeAlias(a) => {
                type_aliases.insert(a.name.clone(), HirType::from_ast(&a.aliased_type));
            }
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
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }

    // Phase 1: 注册所有 prelude 函数为原生函数（17 个免import内置）
    for &name in crate::std::decl::PRELUDE_NAMES {
        if !natives.iter().any(|n| n.name == name) {
            // 推断参数和返回类型
            let (params, ret) = match name {
                "println" => (
                    vec![HirParam {
                        name: "message".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Unit".into())),
                ),
                "print" => (
                    vec![HirParam {
                        name: "message".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Unit".into())),
                ),
                "puts" => (
                    vec![HirParam {
                        name: "message".into(),
                        ty: Some(HirType::Named("String".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Unit".into())),
                ),
                "abs" | "sqrt" | "pow" => (
                    vec![HirParam {
                        name: "x".into(),
                        ty: Some(HirType::Named("Float".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Float".into())),
                ),
                "toInt" => (
                    vec![HirParam {
                        name: "x".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Int".into())),
                ),
                "toFloat" => (
                    vec![HirParam {
                        name: "x".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Float".into())),
                ),
                "toStr" | "toString" => (
                    vec![HirParam {
                        name: "x".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("String".into())),
                ),
                "clock" => (vec![], Some(HirType::Named("Float".into()))),
                "strlen" => (
                    vec![HirParam {
                        name: "s".into(),
                        ty: Some(HirType::Named("String".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Int".into())),
                ),
                "CString" => (
                    vec![HirParam {
                        name: "s".into(),
                        ty: Some(HirType::Named("String".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Any".into())),
                ),
                "CStr" => (
                    vec![HirParam {
                        name: "p".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("String".into())),
                ),
                "ptrIsNull" => (
                    vec![HirParam {
                        name: "p".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Boolean".into())),
                ),
                "ptrToInt" => (
                    vec![HirParam {
                        name: "p".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Int".into())),
                ),
                "intToPtr" => (
                    vec![HirParam {
                        name: "i".into(),
                        ty: Some(HirType::Named("Int".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Any".into())),
                ),
                "makeCallback" => (
                    vec![HirParam {
                        name: "fn".into(),
                        ty: Some(HirType::Named("Any".into())),
                        default_value: None,
                        is_vararg: false,
                    }],
                    Some(HirType::Named("Any".into())),
                ),
                "listOf" => (
                    (0..10)
                        .map(|i| HirParam {
                            name: format!("item{}", i).into(),
                            ty: Some(HirType::Named("Value".into())),
                            default_value: None,
                            is_vararg: false,
                        })
                        .collect(),
                    Some(HirType::Named("Any".into())),
                ),
                _ => (vec![], Some(HirType::Named("Any".into()))),
            };

            natives.push(HirFunction {
                name: name.into(),
                params,
                ret,
                body: HirBlock {
                    stmts: vec![],
                },
                is_native: true,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
            });
        }
    }

    // P7.6: 注册 malloc/free 为原生函数
    if !natives.iter().any(|n| n.name == "malloc") {
        natives.push(HirFunction {
            name: "malloc".into(),
            params: vec![HirParam {
                name: "size".into(),
                ty: Some(HirType::Named("Int".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }

    // P8.5: 注册 CString/CStr 为原生函数
    for &name in &[
        "CString", "CStr",
    ] {
        if !natives.iter().any(|n| n.name == name) {
            natives.push(HirFunction {
                name: name.into(),
                params: vec![HirParam {
                    name: "s".into(),
                    ty: Some(HirType::Named("String".into())),
                    default_value: None,
                    is_vararg: false,
                }],
                ret: Some(HirType::Pointer(Box::new(HirType::Named("Char".into())))),
                body: HirBlock {
                    stmts: vec![],
                },
                is_native: true,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
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
                params: params
                    .iter()
                    .map(|&(pn, pt)| HirParam {
                        name: pn.into(),
                        ty: Some(HirType::from_ast_str(pt)),
                        default_value: None,
                        is_vararg: false,
                    })
                    .collect(),
                ret: Some(HirType::from_ast_str(ret)),
                body: HirBlock {
                    stmts: vec![],
                },
                is_native: true,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
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
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Pointer(Box::new(HirType::Named("Int".into())))),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    if !natives.iter().any(|n| n.name == "free") {
        natives.push(HirFunction {
            name: "free".into(),
            params: vec![HirParam {
                name: "ptr".into(),
                ty: Some(HirType::Named("Any".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
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
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.send(actor, msg) — 向 Actor 发送消息
    if !natives.iter().any(|n| n.name == "aura.concurrent.send") {
        natives.push(HirFunction {
            name: "aura.concurrent.send".into(),
            params: vec![
                HirParam {
                    name: "actor".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
                HirParam {
                    name: "msg".into(),
                    ty: Some(HirType::Named("Any".into())),
                    default_value: None,
                    is_vararg: false,
                },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.ask(actor, msg) — 向 Actor 请求响应
    if !natives.iter().any(|n| n.name == "aura.concurrent.ask") {
        natives.push(HirFunction {
            name: "aura.concurrent.ask".into(),
            params: vec![
                HirParam {
                    name: "actor".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
                HirParam {
                    name: "msg".into(),
                    ty: Some(HirType::Named("Any".into())),
                    default_value: None,
                    is_vararg: false,
                },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.newChannel(bound) — 创建 Channel
    if !natives.iter().any(|n| n.name == "aura.concurrent.newChannel") {
        natives.push(HirFunction {
            name: "aura.concurrent.newChannel".into(),
            params: vec![HirParam {
                name: "bound".into(),
                ty: Some(HirType::Named("Int".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.channelSend(ch, val) — 发送值到 Channel
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelSend") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelSend".into(),
            params: vec![
                HirParam {
                    name: "ch".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
                HirParam {
                    name: "val".into(),
                    ty: Some(HirType::Named("Any".into())),
                    default_value: None,
                    is_vararg: false,
                },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.channelRecv(ch) — 从 Channel 接收值（阻塞）
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelRecv") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelRecv".into(),
            params: vec![HirParam {
                name: "ch".into(),
                ty: Some(HirType::Named("Int".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.channelTryRecv(ch) — 从 Channel 接收值（非阻塞）
    if !natives.iter().any(|n| n.name == "aura.concurrent.channelTryRecv") {
        natives.push(HirFunction {
            name: "aura.concurrent.channelTryRecv".into(),
            params: vec![HirParam {
                name: "ch".into(),
                ty: Some(HirType::Named("Int".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.select(ch1, ch2) — select 多路复用（最多 2 通道）
    if !natives.iter().any(|n| n.name == "aura.concurrent.select") {
        natives.push(HirFunction {
            name: "aura.concurrent.select".into(),
            params: vec![
                HirParam {
                    name: "ch1".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
                HirParam {
                    name: "ch2".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
            ],
            ret: Some(HirType::Named("Any".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.spawnActor(name) — 创建 Actor 实例（返回 actor ID）
    if !natives.iter().any(|n| n.name == "aura.concurrent.spawnActor") {
        natives.push(HirFunction {
            name: "aura.concurrent.spawnActor".into(),
            params: vec![HirParam {
                name: "name".into(),
                ty: Some(HirType::Named("String".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Int".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.supervise(parent, child) — 建立监督关系
    if !natives.iter().any(|n| n.name == "aura.concurrent.supervise") {
        natives.push(HirFunction {
            name: "aura.concurrent.supervise".into(),
            params: vec![
                HirParam {
                    name: "parent".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
                HirParam {
                    name: "child".into(),
                    ty: Some(HirType::Named("Int".into())),
                    default_value: None,
                    is_vararg: false,
                },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        });
    }
    // aura.concurrent.actorAlive(id) — 检查 Actor 是否存活
    if !natives.iter().any(|n| n.name == "aura.concurrent.actorAlive") {
        natives.push(HirFunction {
            name: "aura.concurrent.actorAlive".into(),
            params: vec![HirParam {
                name: "id".into(),
                ty: Some(HirType::Named("Int".into())),
                default_value: None,
                is_vararg: false,
            }],
            ret: Some(HirType::Named("Boolean".into())),
            body: HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
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
                    default_value: None,
                    is_vararg: false,
                })
                .collect();
            let ret =
                params.iter().any(|(_, pt)| *pt != "Unit").then(|| HirType::Named("Any".into()));
            natives.push(HirFunction {
                name: name.into(),
                params: param_defs,
                ret,
                body: HirBlock {
                    stmts: vec![],
                },
                is_native: true,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
            });
        }
    }

    HirProgram {
        functions,
        structs,
        enums,
        natives,
        constants,
        type_aliases,
        top_level_statements: if top_level_stmts.is_empty() {
            None
        } else {
            Some(HirBlock {
                stmts: top_level_stmts,
            })
        },
    }
}

fn desugar_fn(f: &FnDecl) -> HirFunction {
    desugar_fn_with_self(f, false, None)
}

/// 降级函数，可选添加隐式 self 参数（方法需要）；`name_override` 供类方法使用
fn desugar_fn_with_self(f: &FnDecl, is_method: bool, name_override: Option<String>) -> HirFunction {
    let mut body = match &f.body {
        Some(b) => desugar_block(b),
        None => HirBlock {
            stmts: vec![],
        },
    };
    // 表达式体函数：`fun f() = expr` 被解析为仅含一条表达式语句的块，
    // 需将该表达式作为返回值（Kotlin 语义：块末表达式即返回值）。
    if body.stmts.len() == 1 {
        if let HirStmt::Expr(e) = &body.stmts[0] {
            let e = e.clone();
            body.stmts = vec![HirStmt::Return(Some(e))];
        }
    }
    let mut params: Vec<HirParam> = Vec::new();
    // 方法添加隐式 self 参数
    if is_method {
        params.push(HirParam {
            name: "self".to_string(),
            ty: Some(HirType::Named("Any".into())),
            default_value: None,
            is_vararg: false,
        });
    }
    params.extend(f.params.iter().map(|p| HirParam {
        name: p.name.clone(),
        ty: HirType::from_ast_opt(&p.type_hint),
        default_value: p.default_value.as_ref().map(|e| Box::new(desugar_expr(e))),
        is_vararg: p.is_vararg,
    }));
    let fname = name_override.unwrap_or_else(|| f.name.clone());
    // P2：tailrec 循环化（仅当存在尾自调用时改写，否则保持递归语义）
    if f.modifiers.iter().any(|m| matches!(m, FnModifier::Tailrec)) {
        let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
        body = tailrec_rewrite(&fname, &param_names, body);
    }
    HirFunction {
        name: fname,
        params,
        ret: HirType::from_ast_opt(&f.return_type),
        body,
        is_native: false,
        type_params: f.type_params.iter().map(|t| t.name.clone()).collect(),
        ffi_abi: FfiAbi::None,
        ffi_lib: None,
    }
}

/// 类/结构体方法降级：`Class.method` 命名 + 类上下文（裸字段/裸方法改写）+ tailrec 循环化。
/// `with_self = false` 用于 companion 方法（无接收者）。
fn desugar_class_method(f: &FnDecl, class: &str, with_self: bool) -> HirFunction {
    let full_name = format!("{}.{}", class, f.name);
    let fields = {
        let table = CLASS_TABLE.with(|t| t.borrow().clone());
        table.get(class).map(|e| e.effective_fields(&table, class)).unwrap_or_default()
    };
    let prev = CLASS_CTX.with(|c| {
        c.borrow_mut().replace(ClassCtx {
            class: class.to_string(),
            fields,
            locals: vec![std::collections::HashSet::new()],
        })
    });
    // 参数进入局部作用域（屏蔽同名字段）
    let param_names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    for p in &param_names {
        register_local(p);
    }
    let mut hir = desugar_fn_with_self(f, with_self, Some(full_name.clone()));
    pop_local_scope();
    CLASS_CTX.with(|c| *c.borrow_mut() = prev);
    // tailrec（参数含 self）
    if f.modifiers.iter().any(|m| matches!(m, FnModifier::Tailrec)) {
        let mut ps = Vec::new();
        if with_self {
            ps.push("self".to_string());
        }
        ps.extend(param_names);
        hir.body = tailrec_rewrite(&full_name, &ps, hir.body);
    }
    hir
}

fn self_hir_param() -> HirParam {
    HirParam {
        name: "self".into(),
        ty: Some(HirType::Named("Any".into())),
        default_value: None,
        is_vararg: false,
    }
}

/// 末尾纯表达式语句转为 Return（访问器体 `= expr` / 块体末表达式）
fn block_with_trailing_return(mut b: HirBlock) -> HirBlock {
    if let Some(last) = b.stmts.last() {
        if matches!(last, HirStmt::Expr(_)) {
            if let Some(HirStmt::Expr(e)) = b.stmts.pop() {
                b.stmts.push(HirStmt::Return(Some(e)));
            }
        }
    }
    b
}

/// 为带访问器的字段合成 `Class.prop.get/set` 函数。
/// 访问器体内 `field` 上下文关键字经 CLASS_CTX/ACCESSOR_PROP 改写为 `self.<prop>`。
fn synthesize_accessors(class: &str, fields: &[StructField]) -> Vec<HirFunction> {
    let mut out = Vec::new();
    let field_names = {
        let table = CLASS_TABLE.with(|t| t.borrow().clone());
        table
            .get(class)
            .map(|e| e.effective_fields(&table, class))
            .unwrap_or_else(|| fields.iter().map(|f| f.name.clone()).collect())
    };
    for f in fields {
        let Some(acc) = &f.accessors else { continue };
        // getter
        if let Some(g) = &acc.getter {
            let prev_prop = ACCESSOR_PROP.with(|a| a.borrow_mut().replace(f.name.clone()));
            let prev_ctx = CLASS_CTX.with(|c| {
                c.borrow_mut().replace(ClassCtx {
                    class: class.to_string(),
                    fields: field_names.clone(),
                    locals: vec![std::collections::HashSet::new()],
                })
            });
            let body = block_with_trailing_return(desugar_block(&g.body));
            CLASS_CTX.with(|c| *c.borrow_mut() = prev_ctx);
            ACCESSOR_PROP.with(|a| *a.borrow_mut() = prev_prop);
            out.push(HirFunction {
                name: format!("{}.{}.get", class, f.name),
                params: vec![self_hir_param()],
                ret: HirType::from_ast_opt(&f.type_hint),
                body,
                is_native: false,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
            });
        }
        // setter
        if let Some(st) = &acc.setter {
            let param_name =
                st.param.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| "value".into());
            let param_ty = st
                .param
                .as_ref()
                .and_then(|p| HirType::from_ast_opt(&p.type_hint))
                .or_else(|| HirType::from_ast_opt(&f.type_hint));
            let prev_prop = ACCESSOR_PROP.with(|a| a.borrow_mut().replace(f.name.clone()));
            let prev_ctx = CLASS_CTX.with(|c| {
                c.borrow_mut().replace(ClassCtx {
                    class: class.to_string(),
                    fields: field_names.clone(),
                    locals: vec![std::collections::HashSet::new()],
                })
            });
            register_local(&param_name);
            let body = desugar_block(&st.body);
            pop_local_scope();
            CLASS_CTX.with(|c| *c.borrow_mut() = prev_ctx);
            ACCESSOR_PROP.with(|a| *a.borrow_mut() = prev_prop);
            out.push(HirFunction {
                name: format!("{}.{}.set", class, f.name),
                params: vec![
                    self_hir_param(),
                    HirParam {
                        name: param_name,
                        ty: param_ty,
                        default_value: None,
                        is_vararg: false,
                    },
                ],
                ret: Some(HirType::Named("Unit".into())),
                body,
                is_native: false,
                type_params: vec![],
                ffi_abi: FfiAbi::None,
                ffi_lib: None,
            });
        }
    }
    out
}

fn desugar_block(b: &Expr) -> HirBlock {
    // 类上下文局部作用域（裸字段改写需要区分局部变量与字段）
    push_local_scope();
    let r = desugar_block_inner(b);
    pop_local_scope();
    r
}

fn desugar_block_inner(b: &Expr) -> HirBlock {
    // 表达式位置上的块：`{ stmt* }` 或 `{ stmt*; lastExpr }`
    let stmts = match b {
        Expr::Block(stmts, _) => stmts,
        other => {
            return HirBlock {
                stmts: vec![HirStmt::Expr(desugar_expr(other))],
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
        } => {
            register_local(name);
            HirStmt::Val {
                name: name.clone(),
                ty: HirType::from_ast_opt(type_hint),
                init: initializer.as_ref().map(|e| desugar_expr(e)),
            }
        }
        Stmt::Var {
            name,
            type_hint,
            initializer,
            ..
        } => {
            register_local(name);
            HirStmt::Var {
                name: name.clone(),
                ty: HirType::from_ast_opt(type_hint),
                init: initializer.as_ref().map(|e| desugar_expr(e)),
            }
        }
        Stmt::Destructure {
            patterns,
            expr,
            ..
        } => {
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
            condition,
            body,
            ..
        } => HirStmt::While {
            cond: desugar_expr(condition),
            body: desugar_block(body),
        },
        Expr::DoWhile {
            condition,
            body,
            ..
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
        Expr::Assign {
            target,
            value,
            ..
        } => {
            // 访问器 setter：obj.prop = v → Class.prop.set(obj, v)
            if let Expr::MemberAccess {
                object,
                name,
                ..
            } = target.as_ref()
            {
                if let Some(class) = resolve_accessor_set(object, name) {
                    return HirStmt::Expr(HirExpr::Call {
                        callee: format!("{}.{}.set", class, name),
                        args: vec![
                            desugar_expr(object),
                            desugar_expr(value),
                        ],
                    });
                }
            }
            HirStmt::Assign {
                target: desugar_expr(target),
                value: desugar_expr(value),
            }
        }
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
        Expr::Ident(n, _) => {
            register_local(n);
            n.clone()
        }
        _ => "_".to_string(),
    };

    // 范围：`start..end` 或 `start..<end`
    // 将自增放在循环体开头，这样 `continue` 跳过后续代码时自增已执行，
    // 避免 `continue` 跳过尾部自增导致无限循环。
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
        // 初始值 = start - 1，循环体内先自增再赋值
        let init_e = HirExpr::Binary {
            op: HirBinOp::Sub,
            lhs: Box::new(start_e),
            rhs: Box::new(HirExpr::Lit(Literal::Int(1))),
        };
        // 循环条件：inclusive → idx < end；exclusive → idx < end - 1
        let cmp = if *inclusive {
            HirExpr::Binary {
                op: HirBinOp::Lt,
                lhs: Box::new(HirExpr::Var(idx.clone())),
                rhs: Box::new(end_e),
            }
        } else {
            HirExpr::Binary {
                op: HirBinOp::Lt,
                lhs: Box::new(HirExpr::Var(idx.clone())),
                rhs: Box::new(HirExpr::Binary {
                    op: HirBinOp::Sub,
                    lhs: Box::new(end_e),
                    rhs: Box::new(HirExpr::Lit(Literal::Int(1))),
                }),
            }
        };
        let mut body_stmts = vec![
            // 先自增
            HirStmt::Assign {
                target: HirExpr::Var(idx.clone()),
                value: HirExpr::Binary {
                    op: HirBinOp::Add,
                    lhs: Box::new(HirExpr::Var(idx.clone())),
                    rhs: Box::new(HirExpr::Lit(Literal::Int(1))),
                },
            },
            // 再赋值给循环变量
            HirStmt::Val {
                name: var_name.clone(),
                ty: None,
                init: Some(HirExpr::Var(idx.clone())),
            },
        ];
        body_stmts.extend(desugar_block(body).stmts);
        return HirStmt::Block(HirBlock {
            stmts: vec![
                HirStmt::Var {
                    name: idx.clone(),
                    ty: None,
                    init: Some(init_e),
                },
                HirStmt::While {
                    cond: cmp,
                    body: HirBlock {
                        stmts: body_stmts,
                    },
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
        args: vec![
            iter_e,
            HirExpr::Var(idx.clone()),
        ],
    };
    let mut body_stmts = vec![
        HirStmt::Val {
            name: var_name.clone(),
            ty: None,
            init: Some(get_call),
        },
    ];
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
                body: HirBlock {
                    stmts: body_stmts,
                },
            },
        ],
    })
}

/// 辅助函数：在 thread-local 中查找导入解析，返回克隆的字符串（避免生命周期问题）
fn lookup_import_short(n: &str) -> Option<String> {
    IMPORT_RESOLUTION.with(|r| {
        r.borrow().as_ref().and_then(|ir| {
            ir.resolve_short_name(n).or_else(|| ir.resolve_alias(n)).map(|s| s.to_string())
        })
    })
}

fn lookup_import_module_alias(n: &str) -> Option<String> {
    IMPORT_RESOLUTION.with(|r| {
        r.borrow().as_ref().and_then(|ir| ir.resolve_module_alias(n).map(|s| s.to_string()))
    })
}

fn desugar_expr(e: &Expr) -> HirExpr {
    match e {
        Expr::Literal(l, _) => HirExpr::Lit(l.clone()),
        // 类上下文：裸字段 → self.field；访问器 `field` → self.<prop>
        Expr::Ident(n, _) => class_bare_ident(n).unwrap_or(HirExpr::Var(n.clone())),
        Expr::Binary {
            op,
            lhs,
            rhs,
            ..
        } => {
            // 运算符重载：`a + b`（a 为重载类实例）→ `Class.plus(a, b)`
            if let Some(callee) = operator_method_for(*op, lhs) {
                return HirExpr::Call {
                    callee,
                    args: vec![
                        desugar_expr(lhs),
                        desugar_expr(rhs),
                    ],
                };
            }
            HirExpr::Binary {
                op: HirBinOp::from_ast(*op),
                lhs: Box::new(desugar_expr(lhs)),
                rhs: Box::new(desugar_expr(rhs)),
            }
        }
        Expr::Unary {
            op,
            operand,
            ..
        } => {
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
        Expr::Call {
            callee,
            args,
            ..
        } => {
            // 构造器调用检测：Type(args) → HirExpr::New
            if let Expr::Ident(n, _) = callee.as_ref() {
                if is_type_name(n) {
                    return HirExpr::New {
                        type_name: n.clone(),
                        args: args.iter().map(desugar_expr).collect(),
                    };
                }
            }
            // 类上下文内的裸方法调用：m(args) → Class.m(self, args)（companion 方法不带 self）
            if let Expr::Ident(n, _) = callee.as_ref() {
                if let Some((full, insert_self)) = bare_call_in_class(n) {
                    let mut all_args = Vec::new();
                    if insert_self {
                        all_args.push(HirExpr::Var("self".into()));
                    }
                    for a in args {
                        all_args.push(desugar_expr(a));
                    }
                    return HirExpr::Call {
                        callee: full,
                        args: all_args,
                    };
                }
            }
            let callee_name = match callee.as_ref() {
                Expr::Ident(n, _) => {
                    // 检查导入解析：短名/别名 → 完整原生函数名
                    // import aura.concurrent.* + spawn(42) → aura.concurrent.spawn(42)
                    // import aura.concurrent.spawn as s + s(42) → aura.concurrent.spawn(42)
                    lookup_import_short(n).unwrap_or_else(|| n.clone())
                }
                // 模块调用 `module.method(args)`：降级为 `module.method(args...)`
                // 与普通方法调用 `obj.method(args)` → `method(obj, args...)` 区分
                Expr::MemberAccess {
                    object,
                    name,
                    ..
                } => {
                    // 检查模块别名：import aura.concurrent as cc + cc.spawn(42)
                    if let Expr::Ident(module_name, _) = object.as_ref() {
                        if let Some(am) = lookup_import_module_alias(module_name) {
                            return HirExpr::Call {
                                callee: format!("{}.{}", am, name),
                                args: args.iter().map(desugar_expr).collect(),
                            };
                        }
                        // 检查是否为标准库模块调用（支持嵌套：aura.concurrent.spawn）
                        if is_std_module(module_name) {
                            let full_module = full_package_name(module_name);
                            return HirExpr::Call {
                                callee: format!("{}.{}", full_module, name),
                                args: args.iter().map(desugar_expr).collect(),
                            };
                        }
                    }
                    // 嵌套：aura.concurrent.spawn
                    if let Expr::MemberAccess {
                        object: inner_obj,
                        name: inner_name,
                        ..
                    } = object.as_ref()
                    {
                        if let Expr::Ident(module_name, _) = inner_obj.as_ref() {
                            // 检查嵌套模块别名
                            if let Some(am) = lookup_import_module_alias(module_name) {
                                let nested_name = format!("{}.{}", inner_name, name);
                                return HirExpr::Call {
                                    callee: format!("{}.{}", am, nested_name),
                                    args: args.iter().map(desugar_expr).collect(),
                                };
                            }
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
                    // companion 成员调用：C.m(args) / C.f（C 为类型名）
                    if let Expr::Ident(obj_name, _) = object.as_ref() {
                        if is_type_name(obj_name) {
                            if let Some(fname) = companion_member_fn(obj_name, name) {
                                return HirExpr::Call {
                                    callee: fname,
                                    args: args.iter().map(desugar_expr).collect(),
                                };
                            }
                        }
                        // extern interface 方法调用：Utils.add(3, 4) → Utils.add(3, 4)
                        if INTERFACE_NAMES.with(|n| n.borrow().contains(obj_name)) {
                            return HirExpr::Call {
                                callee: format!("{}.{}", obj_name, name),
                                args: args.iter().map(desugar_expr).collect(),
                            };
                        }
                    }
                    // 类方法分派：接收者静态类型（含继承链）→ Class.method(self, args)；
                    // open/abstract 方法（可被子类重写）→ CallVirtual 动态分派
                    if let Some((class, _)) = resolve_method_owner(object, name) {
                        let mut all_args = vec![desugar_expr(object)];
                        for a in args {
                            all_args.push(desugar_expr(a));
                        }
                        // 默认参数填充
                        let params_opt = FUNCTION_PARAMS
                            .with(|f| f.borrow().get(&format!("{}.{}", class, name)).cloned());
                        if let Some(params) = params_opt {
                            let required = params
                                .iter()
                                .filter(|p| p.default_value.is_none() && !p.is_vararg)
                                .count();
                            if all_args.len() < params.len() + 1 && all_args.len() >= required + 1 {
                                let mut defaults: Vec<HirExpr> = Vec::new();
                                for i in all_args.len()..params.len() + 1 {
                                    if let Some(ref dv) = params[i - 1].default_value {
                                        defaults.push(dv.as_ref().clone());
                                    }
                                }
                                all_args.extend(defaults);
                            }
                        }
                        let is_virtual = CLASS_TABLE
                            .with(|t| t.borrow().values().any(|e| e.open_methods.contains(name)));
                        if is_virtual {
                            return HirExpr::CallVirtual {
                                recv: Box::new(desugar_expr(object)),
                                name: name.clone(),
                                args: all_args,
                            };
                        }
                        return HirExpr::Call {
                            callee: format!("{}.{}", class, name),
                            args: all_args,
                        };
                    }
                    // 普通方法调用：降级为 method(obj, args...)
                    // 如果方法是内置方法，解析为完整原生函数名
                    let resolved_name =
                        resolve_builtin_method(name).unwrap_or_else(|| name.clone());
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
            // 默认参数填充 + vararg 打包
            let mut all_args: Vec<HirExpr> = args.iter().map(desugar_expr).collect();
            let params_opt = FUNCTION_PARAMS.with(|f| f.borrow().get(&callee_name).cloned());
            if let Some(params) = params_opt {
                // 默认参数填充：补充有默认值但未提供的参数
                let required =
                    params.iter().filter(|p| p.default_value.is_none() && !p.is_vararg).count();
                if all_args.len() < params.len() && all_args.len() >= required {
                    let mut defaults: Vec<HirExpr> = Vec::new();
                    for i in all_args.len()..params.len() {
                        if let Some(ref dv) = params[i].default_value {
                            defaults.push(dv.as_ref().clone());
                        }
                    }
                    all_args.extend(defaults);
                }
                // vararg 打包：如果最后一个参数是 vararg 且实参多于声明参数，打包为数组
                if let Some(last_param) = params.last() {
                    if last_param.is_vararg {
                        let named_count = params.len() - 1; // vararg 参数本身不计入命名参数
                        if all_args.len() > named_count {
                            let vararg_exprs: Vec<HirExpr> = all_args[named_count..].to_vec();
                            // 用 listOf(...) 创建数组
                            let array_expr = HirExpr::Call {
                                callee: "listOf".to_string(),
                                args: vararg_exprs,
                            };
                            all_args.truncate(named_count);
                            all_args.push(array_expr);
                        } else if all_args.len() == named_count {
                            // 无 vararg 实参：传空数组
                            all_args.push(HirExpr::Call {
                                callee: "listOf".to_string(),
                                args: vec![],
                            });
                        }
                    }
                }
            }
            HirExpr::Call {
                callee: callee_name,
                args: all_args,
            }
        }
        Expr::NamedArg { value, .. } => desugar_expr(value),
        Expr::MemberAccess {
            object,
            name,
            ..
        } => {
            // companion 字段读取：C.f → Class.f()（零参函数）
            if let Expr::Ident(obj_name, _) = object.as_ref() {
                if is_type_name(obj_name) {
                    if let Some(fname) = companion_member_fn(obj_name, name) {
                        return HirExpr::Call {
                            callee: fname,
                            args: vec![],
                        };
                    }
                }
            }
            // 访问器 getter：obj.prop → Class.prop.get(obj)
            if let Some(class) = resolve_accessor_get(object, name) {
                return HirExpr::Call {
                    callee: format!("{}.{}.get", class, name),
                    args: vec![desugar_expr(object)],
                };
            }
            HirExpr::Member {
                object: Box::new(desugar_expr(object)),
                name: name.clone(),
            }
        }
        Expr::SafeAccess {
            object,
            name,
            ..
        } => {
            // 近似：安全调用降级为普通成员访问（完整空安全留待运行时）
            HirExpr::Member {
                object: Box::new(desugar_expr(object)),
                name: name.clone(),
            }
        }
        Expr::Index {
            container,
            index,
            ..
        } => HirExpr::Index {
            container: Box::new(desugar_expr(container)),
            index: Box::new(desugar_expr(index)),
        },
        Expr::New {
            type_name,
            args,
            ..
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
        Expr::When {
            subject,
            arms,
            ..
        } => desugar_when(subject, arms),
        Expr::Block(_stmts, _) => HirExpr::Block(desugar_block(e)),
        Expr::Range { .. } => HirExpr::Call {
            callee: "__range".into(),
            args: vec![],
        },
        Expr::Elvis {
            lhs, rhs, ..
        } => {
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
        // Fix 4: Lambda/Closure → HirExpr::Lambda（不再降级为 __lambda 调用）
        Expr::Lambda {
            params,
            body,
            ..
        } => {
            let hir_params = params
                .iter()
                .map(|p| HirParam {
                    name: p.name.clone(),
                    ty: HirType::from_ast_opt(&p.type_hint),
                    default_value: p.default_value.as_ref().map(|e| Box::new(desugar_expr(e))),
                    is_vararg: p.is_vararg,
                })
                .collect();
            // lambda 参数进入局部作用域（屏蔽同名字段）
            push_local_scope();
            for p in params {
                register_local(&p.name);
            }
            let hir_body = desugar_block(body);
            pop_local_scope();
            HirExpr::Lambda {
                params: hir_params,
                body: hir_body,
            }
        }
        Expr::Closure {
            params,
            body,
            ..
        } => {
            let hir_params = params
                .iter()
                .map(|p| HirParam {
                    name: p.name.clone(),
                    ty: HirType::from_ast_opt(&p.type_hint),
                    default_value: p.default_value.as_ref().map(|e| Box::new(desugar_expr(e))),
                    is_vararg: p.is_vararg,
                })
                .collect();
            // lambda 参数进入局部作用域（屏蔽同名字段）
            push_local_scope();
            for p in params {
                register_local(&p.name);
            }
            let hir_body = desugar_block(body);
            pop_local_scope();
            HirExpr::Lambda {
                params: hir_params,
                body: hir_body,
            }
        }
        Expr::Destructure { expr, .. } => desugar_expr(expr),
        Expr::Throw { value, .. } => HirExpr::Call {
            callee: "__throw".into(),
            args: vec![desugar_expr(value)],
        },
        Expr::Await { expr, .. } => HirExpr::Await(Box::new(desugar_expr(expr))),
        Expr::This(_) => HirExpr::Var("self".to_string()),
        // P10.9: select 多路复用 — 降级为 `aura.concurrent.select(ch1, ch2)` 原生函数调用（最多 2 通道）
        Expr::Select {
            branches, ..
        } => {
            let ch_args: Vec<HirExpr> = branches
                .iter()
                .take(2)
                .map(|b| match &b.pattern {
                    Expr::Call { callee, .. } => {
                        if let Expr::MemberAccess { object, .. } = callee.as_ref() {
                            desugar_expr(object)
                        } else {
                            desugar_expr(callee)
                        }
                    }
                    _ => desugar_expr(&b.pattern),
                })
                .collect();
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
                    Expr::Binary {
                        op: BinOp::To,
                        ..
                    } => HirExpr::Lit(Literal::Bool(true)),
                    Expr::Ident(name, span) if name.starts_with("__is__") => {
                        // is T → 调用 aura_isOfType(value, "T")
                        let type_name = &name[6..];
                        HirExpr::Call {
                            callee: "aura_isOfType".to_string(),
                            args: vec![
                                desugar_expr(s),
                                HirExpr::Lit(Literal::String(type_name.to_string())),
                            ],
                        }
                    }
                    Expr::Ident(name, _) if name == "__else__" => {
                        // else → 默认分支（始终为 true）
                        HirExpr::Lit(Literal::Bool(true))
                    }
                    Expr::InRange { range, .. } => {
                        // in start..end 或 in start..<end → 范围检查
                        let (start_e, end_e, inclusive) = match range.as_ref() {
                            Expr::Range {
                                start,
                                end,
                                inclusive,
                                ..
                            } => (start.clone(), end.clone(), *inclusive),
                            _ => (
                                Some(Box::new(Expr::Literal(
                                    Literal::Int(0),
                                    Span::single(0, 1, 1),
                                ))),
                                Some(Box::new(Expr::Literal(
                                    Literal::Int(0),
                                    Span::single(0, 1, 1),
                                ))),
                                true,
                            ),
                        };
                        let start_expr = desugar_expr(
                            start_e
                                .as_deref()
                                .unwrap_or(&Expr::Literal(Literal::Int(0), Span::single(0, 1, 1))),
                        );
                        let end_expr = desugar_expr(
                            end_e
                                .as_deref()
                                .unwrap_or(&Expr::Literal(Literal::Int(0), Span::single(0, 1, 1))),
                        );
                        let subject_expr = desugar_expr(s);
                        // start <= subject && subject <= end (inclusive)
                        // 或 start <= subject && subject < end (exclusive)
                        let lower = HirExpr::Binary {
                            op: HirBinOp::Ge,
                            lhs: Box::new(subject_expr.clone()),
                            rhs: Box::new(start_expr),
                        };
                        let upper = if inclusive {
                            HirExpr::Binary {
                                op: HirBinOp::Le,
                                lhs: Box::new(subject_expr),
                                rhs: Box::new(end_expr),
                            }
                        } else {
                            HirExpr::Binary {
                                op: HirBinOp::Lt,
                                lhs: Box::new(subject_expr),
                                rhs: Box::new(end_expr),
                            }
                        };
                        HirExpr::Binary {
                            op: HirBinOp::And,
                            lhs: Box::new(lower),
                            rhs: Box::new(upper),
                        }
                    }
                    _ => HirExpr::Binary {
                        op: HirBinOp::Eq,
                        lhs: Box::new(desugar_expr(s)),
                        rhs: Box::new(desugar_expr(p)),
                    },
                }
            }
            (None, Some(p)) => {
                // 无 subject 时模式即条件；但 `__else__` 是特殊标识符
                if let Expr::Ident(name, _) = p {
                    if name == "__else__" {
                        HirExpr::Lit(Literal::Bool(true))
                    } else {
                        desugar_expr(p)
                    }
                } else {
                    desugar_expr(p)
                }
            }
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
// 脚本模式：隐式 main 合成
// ─────────────────────────────────────────────────────────────────────────────

/// 若程序没有 main 函数，合成一个
/// 将顶层语句包装为隐式 main 函数体
///
/// 返回：true 表示合成了隐式 main，false 表示已有 main 或无顶层语句
pub fn synthesize_main_if_missing(hir: &mut HirProgram) -> bool {
    // 2. 获取顶层语句
    let body = hir.top_level_statements.take();

    match body {
        Some(block) if !block.stmts.is_empty() => {
            // 1. 检查是否已有 main 函数
            if let Some(main_idx) = hir.functions.iter().position(|f| f.name == "main") {
                // Bug fix: 已有 main + 顶层语句 → 将顶层语句前置到 main 体首，
                // 使顶层 val/var 在 main 内可见（此前顶层语句被孤立，运行时值丢失）。
                hir.functions[main_idx].body.stmts.splice(0..0, block.stmts.into_iter());
                false
            } else {
                // 3. 合成 main 函数
                let main_func = HirFunction {
                    name: "main".into(),
                    params: vec![],
                    ret: Some(HirType::Named("Unit".into())),
                    body: block,
                    is_native: false,
                    type_params: vec![],
                    ffi_abi: FfiAbi::None,
                    ffi_lib: None,
                };

                // 4. 插入到 functions 开头（确保 entry=0 指向 main）
                hir.functions.insert(0, main_func);
                true
            }
        }
        _ => {
            // 无顶层语句，无需合成
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// P9: 标准库模块检测与原生函数注册
// ─────────────────────────────────────────────────────────────────────────────

/// 解析内置方法名为完整原生函数名（如 `toString` → `toString` prelu，或 `aura.builtin.xxx`）
fn resolve_builtin_method(name: &str) -> Option<String> {
    // 优先检查 prelu 函数（免import，始终可用）
    if crate::std::decl::is_prelude(name) {
        return Some(name.to_string());
    }
    // 其次检查 std 命名空间函数
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
            "io" | "math"
                | "string"
                | "collections"
                | "fs"
                | "net"
                | "json"
                | "time"
                | "test"
                | "builtin"
                | "env"
                | "process"
                | "random"
                | "encoding"
                | "ascii"
                | "console"
                | "path"
                | "assert"
                | "iter"
                | "concurrent"
        );
    }
    // 兼容短名：io, math, ...
    matches!(
        name,
        "io" | "math"
            | "string"
            | "collections"
            | "fs"
            | "net"
            | "json"
            | "time"
            | "test"
            | "builtin"
            | "env"
            | "process"
            | "random"
            | "encoding"
            | "ascii"
            | "console"
            | "path"
            | "assert"
            | "iter"
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
        // ── type check ──
        (
            "aura_isOfType",
            vec![
                ("value", "Any"),
                ("typeName", "String"),
            ],
        ),
        // ── std.io ──
        ("aura.io.println", vec![("msg", "String")]),
        ("aura.io.print", vec![("msg", "String")]),
        ("aura.io.readLine", vec![]),
        ("aura.io.readAll", vec![]),
        ("aura.io.flush", vec![]),
        ("aura.io.fileRead", vec![("path", "String")]),
        (
            "aura.io.fileWrite",
            vec![
                ("path", "String"),
                ("content", "String"),
            ],
        ),
        (
            "aura.io.writeFile",
            vec![
                ("path", "String"),
                ("content", "String"),
            ],
        ),
        ("aura.io.readFile", vec![("path", "String")]),
        ("aura.io.fileExists", vec![("path", "String")]),
        // ── std.math ──
        ("aura.math.abs", vec![("x", "Float")]),
        (
            "aura.math.min",
            vec![
                ("a", "Int"),
                ("b", "Int"),
            ],
        ),
        (
            "aura.math.max",
            vec![
                ("a", "Int"),
                ("b", "Int"),
            ],
        ),
        ("aura.math.ceil", vec![("x", "Float")]),
        ("aura.math.floor", vec![("x", "Float")]),
        ("aura.math.round", vec![("x", "Float")]),
        ("aura.math.trunc", vec![("x", "Float")]),
        ("aura.math.sqrt", vec![("x", "Float")]),
        ("aura.math.cbrt", vec![("x", "Float")]),
        (
            "aura.math.pow",
            vec![
                ("base", "Float"),
                ("exp", "Float"),
            ],
        ),
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
        (
            "aura.math.atan2",
            vec![
                ("y", "Float"),
                ("x", "Float"),
            ],
        ),
        ("aura.math.PI", vec![]),
        ("aura.math.E", vec![]),
        ("aura.math.INT_MAX", vec![]),
        ("aura.math.INT_MIN", vec![]),
        ("aura.math.FLOAT_MAX", vec![]),
        ("aura.math.sign", vec![("x", "Float")]),
        (
            "aura.math.clamp",
            vec![
                ("x", "Float"),
                ("lo", "Float"),
                ("hi", "Float"),
            ],
        ),
        // ── std.string ──
        (
            "aura.string.contains",
            vec![
                ("text", "String"),
                ("substr", "String"),
            ],
        ),
        (
            "aura.string.startsWith",
            vec![
                ("text", "String"),
                ("prefix", "String"),
            ],
        ),
        (
            "aura.string.endsWith",
            vec![
                ("text", "String"),
                ("suffix", "String"),
            ],
        ),
        (
            "aura.string.split",
            vec![
                ("text", "String"),
                ("sep", "String"),
            ],
        ),
        (
            "aura.string.join",
            vec![
                ("text", "String"),
                ("sep", "String"),
            ],
        ),
        (
            "aura.string.replace",
            vec![
                ("text", "String"),
                ("target", "String"),
                ("replacement", "String"),
            ],
        ),
        (
            "aura.string.replaceAll",
            vec![
                ("text", "String"),
                ("target", "String"),
                ("replacement", "String"),
            ],
        ),
        ("aura.string.trim", vec![("text", "String")]),
        ("aura.string.trimStart", vec![("text", "String")]),
        ("aura.string.trimEnd", vec![("text", "String")]),
        (
            "aura.string.substring",
            vec![
                ("text", "String"),
                ("start", "Int"),
                ("end", "Int"),
            ],
        ),
        (
            "aura.string.substringBefore",
            vec![
                ("text", "String"),
                ("sep", "String"),
            ],
        ),
        (
            "aura.string.substringAfter",
            vec![
                ("text", "String"),
                ("sep", "String"),
            ],
        ),
        ("aura.string.toLowerCase", vec![("text", "String")]),
        ("aura.string.toUpperCase", vec![("text", "String")]),
        ("aura.string.length", vec![("text", "String")]),
        ("aura.string.isEmpty", vec![("text", "String")]),
        ("aura.string.format", vec![("template", "String")]),
        (
            "aura.string.repeat",
            vec![
                ("n", "Int"),
                ("text", "String"),
            ],
        ),
        (
            "aura.string.indexOf",
            vec![
                ("text", "String"),
                ("substr", "String"),
            ],
        ),
        (
            "aura.string.lastIndexOf",
            vec![
                ("text", "String"),
                ("substr", "String"),
            ],
        ),
        (
            "aura.string.padStart",
            vec![
                ("text", "String"),
                ("length", "Int"),
                ("pad", "String"),
            ],
        ),
        (
            "aura.string.padEnd",
            vec![
                ("text", "String"),
                ("length", "Int"),
                ("pad", "String"),
            ],
        ),
        ("aura.string.escape", vec![("text", "String")]),
        ("aura.string.unescape", vec![("text", "String")]),
        ("aura.string.splitLines", vec![("text", "String")]),
        ("aura.string.joinLines", vec![("text", "String")]),
        (
            "aura.string.countChar",
            vec![
                ("text", "String"),
                ("char", "String"),
            ],
        ),
        ("aura.string.first", vec![("text", "String")]),
        ("aura.string.last", vec![("text", "String")]),
        ("aura.string.isBlank", vec![("text", "String")]),
        (
            "aura.string.matches",
            vec![
                ("text", "String"),
                ("regex", "String"),
            ],
        ),
        (
            "aura.string.containsAny",
            vec![
                ("text", "String"),
                ("patterns", "String"),
            ],
        ),
        (
            "aura.string.containsAll",
            vec![
                ("text", "String"),
                ("patterns", "String"),
            ],
        ),
        // ── std.collections ──
        // listOf 注册 10 个参数（可变参数），VM 按实际栈深弹出
        (
            "listOf",
            vec![
                ("item0", "Value"),
                ("item1", "Value"),
                ("item2", "Value"),
                ("item3", "Value"),
                ("item4", "Value"),
                ("item5", "Value"),
                ("item6", "Value"),
                ("item7", "Value"),
                ("item8", "Value"),
                ("item9", "Value"),
            ],
        ),
        (
            "aura.collections.listOf",
            vec![
                ("item0", "Value"),
                ("item1", "Value"),
                ("item2", "Value"),
                ("item3", "Value"),
                ("item4", "Value"),
                ("item5", "Value"),
                ("item6", "Value"),
                ("item7", "Value"),
                ("item8", "Value"),
                ("item9", "Value"),
            ],
        ),
        ("aura.collections.mutableListOf", vec![]),
        ("aura.collections.emptyList", vec![]),
        ("aura.collections.arrayOf", vec![]),
        (
            "aura.collections.listContains",
            vec![
                ("list", "List"),
                ("item", "Value"),
            ],
        ),
        (
            "aura.collections.listIndexOf",
            vec![
                ("list", "List"),
                ("item", "Value"),
            ],
        ),
        (
            "aura.collections.listRemove",
            vec![
                ("list", "List"),
                ("item", "Value"),
            ],
        ),
        ("aura.collections.listReverse", vec![("list", "List")]),
        ("aura.collections.listSort", vec![("list", "List")]),
        (
            "aura.collections.listGet",
            vec![
                ("list", "List"),
                ("index", "Int"),
            ],
        ),
        (
            "aura.collections.listSet",
            vec![
                ("list", "List"),
                ("index", "Int"),
                ("value", "Value"),
            ],
        ),
        (
            "aura.collections.listInsert",
            vec![
                ("list", "List"),
                ("index", "Int"),
                ("value", "Value"),
            ],
        ),
        (
            "aura.collections.listSubList",
            vec![
                ("list", "List"),
                ("from", "Int"),
                ("to", "Int"),
            ],
        ),
        ("aura.collections.mapOf", vec![]),
        ("aura.collections.mutableMapOf", vec![]),
        ("aura.collections.emptyMap", vec![]),
        (
            "aura.collections.mapContains",
            vec![
                ("map", "Map"),
                ("value", "Value"),
            ],
        ),
        (
            "aura.collections.mapContainsKey",
            vec![
                ("map", "Map"),
                ("key", "Value"),
            ],
        ),
        (
            "aura.collections.mapContainsValue",
            vec![
                ("map", "Map"),
                ("value", "Value"),
            ],
        ),
        (
            "aura.collections.mapRemove",
            vec![
                ("map", "Map"),
                ("key", "Value"),
            ],
        ),
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
        (
            "aura.fs.writeText",
            vec![
                ("path", "String"),
                ("content", "String"),
            ],
        ),
        ("aura.fs.readBytes", vec![("path", "String")]),
        (
            "aura.fs.writeBytes",
            vec![
                ("path", "String"),
                ("data", "List"),
            ],
        ),
        ("aura.fs.delete", vec![("path", "String")]),
        ("aura.fs.mkdir", vec![("path", "String")]),
        ("aura.fs.mkdirP", vec![("path", "String")]),
        (
            "aura.fs.rename",
            vec![
                ("old", "String"),
                ("new", "String"),
            ],
        ),
        (
            "aura.fs.copy",
            vec![
                ("src", "String"),
                ("dst", "String"),
            ],
        ),
        ("aura.fs.listDir", vec![("path", "String")]),
        ("aura.fs.listFiles", vec![("path", "String")]),
        ("aura.fs.fileSize", vec![("path", "String")]),
        ("aura.fs.lastModified", vec![("path", "String")]),
        ("aura.fs.absolutePath", vec![("path", "String")]),
        ("aura.fs.homeDir", vec![]),
        ("aura.fs.tempDir", vec![]),
        ("aura.fs.currentDir", vec![]),
        (
            "aura.fs.walk",
            vec![
                ("root", "String"),
                ("maxDepth", "Int"),
            ],
        ),
        // ── std.net ──
        (
            "aura.net.tcpConnect",
            vec![
                ("host", "String"),
                ("port", "Int"),
            ],
        ),
        ("aura.net.tcpListen", vec![("port", "Int")]),
        (
            "aura.net.tcpSend",
            vec![
                ("handle", "Int"),
                ("message", "String"),
            ],
        ),
        ("aura.net.tcpRecv", vec![("handle", "Int")]),
        ("aura.net.tcpClose", vec![("handle", "Int")]),
        (
            "aura.net.udpSend",
            vec![
                ("target", "String"),
                ("port", "Int"),
                ("message", "String"),
            ],
        ),
        ("aura.net.udpRecv", vec![("handle", "Int")]),
        ("aura.net.udpClose", vec![("handle", "Int")]),
        ("aura.net.isHostReachable", vec![("host", "String")]),
        ("aura.net.getHostname", vec![]),
        ("aura.net.getLocalIp", vec![]),
        // ── std.json ──
        ("aura.json.parse", vec![("text", "String")]),
        (
            "aura.json.stringify",
            vec![
                ("value", "Value"),
                ("pretty", "Bool"),
            ],
        ),
        ("aura.json.isValid", vec![("text", "String")]),
        (
            "aura.json.get",
            vec![
                ("obj", "Value"),
                ("key", "String"),
            ],
        ),
        (
            "aura.json.set",
            vec![
                ("obj", "Value"),
                ("key", "String"),
                ("value", "Value"),
            ],
        ),
        ("aura.json.keys", vec![("obj", "Value")]),
        ("aura.json.values", vec![("obj", "Value")]),
        ("aura.json.length", vec![("obj", "Value")]),
        (
            "aura.json.contains",
            vec![
                ("obj", "Value"),
                ("key", "String"),
            ],
        ),
        (
            "aura.json.remove",
            vec![
                ("obj", "Value"),
                ("key", "String"),
            ],
        ),
        // ── std.time ──
        ("aura.time.now", vec![]),
        ("aura.time.epoch", vec![]),
        ("aura.time.currentTime", vec![]),
        ("aura.time.sleep", vec![("seconds", "Float")]),
        ("aura.time.duration", vec![("seconds", "Float")]),
        ("aura.time.toDateString", vec![("timestamp", "Int")]),
        ("aura.time.toTimeString", vec![("timestamp", "Int")]),
        (
            "aura.time.formatDate",
            vec![
                ("timestamp", "Int"),
                ("pattern", "String"),
            ],
        ),
        (
            "aura.time.diff",
            vec![
                ("t1", "Float"),
                ("t2", "Float"),
            ],
        ),
        ("aura.time.parseDate", vec![("text", "String")]),
        // ── std.test ──
        (
            "aura.test.assertTrue",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertFalse",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertNotEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertNotNull",
            vec![
                ("value", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertNull",
            vec![
                ("value", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertContains",
            vec![
                ("text", "String"),
                ("substr", "String"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertNotContains",
            vec![
                ("text", "String"),
                ("substr", "String"),
                ("message", "String"),
            ],
        ),
        ("aura.test.assertThrows", vec![]),
        (
            "aura.test.assertGt",
            vec![
                ("a", "Float"),
                ("b", "Float"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertGte",
            vec![
                ("a", "Float"),
                ("b", "Float"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertLt",
            vec![
                ("a", "Float"),
                ("b", "Float"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertLte",
            vec![
                ("a", "Float"),
                ("b", "Float"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertApprox",
            vec![
                ("a", "Float"),
                ("b", "Float"),
                ("epsilon", "Float"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertArrayEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.test.assertMapEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
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
        (
            "aura.builtin.compare",
            vec![
                ("a", "Value"),
                ("b", "Value"),
            ],
        ),
        ("aura.builtin.clone", vec![("value", "Value")]),
        ("aura.builtin.identity", vec![("value", "Value")]),
        // ── std.env ──
        (
            "aura.env.get",
            vec![
                ("name", "String"),
                ("default", "String"),
            ],
        ),
        (
            "aura.env.set",
            vec![
                ("name", "String"),
                ("value", "String"),
            ],
        ),
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
        (
            "aura.random.nextIntRange",
            vec![
                ("min", "Int"),
                ("max", "Int"),
            ],
        ),
        (
            "aura.random.nextFloatRange",
            vec![
                ("min", "Float"),
                ("max", "Float"),
            ],
        ),
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
        (
            "aura.ascii.codeAt",
            vec![
                ("text", "String"),
                ("index", "Int"),
            ],
        ),
        (
            "aura.ascii.charAt",
            vec![
                ("text", "String"),
                ("index", "Int"),
            ],
        ),
        ("aura.ascii.fromCode", vec![("code", "Int")]),
        (
            "aura.ascii.codePointAt",
            vec![
                ("text", "String"),
                ("index", "Int"),
            ],
        ),
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
        (
            "aura.path.relative",
            vec![
                ("from", "String"),
                ("to", "String"),
            ],
        ),
        ("aura.path.resolve", vec![("path", "String")]),
        ("aura.path.normalize", vec![("path", "String")]),
        ("aura.path.isAbsolute", vec![("path", "String")]),
        ("aura.path.isRelative", vec![("path", "String")]),
        ("aura.path.split", vec![("path", "String")]),
        ("aura.path.separators", vec![("path", "String")]),
        ("aura.path.fromUnix", vec![("path", "String")]),
        ("aura.path.fromWindows", vec![("path", "String")]),
        // ── std.assert ──
        (
            "aura.assert.assert",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertTrue",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertFalse",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertNotEq",
            vec![
                ("a", "Value"),
                ("b", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertNotNull",
            vec![
                ("value", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.assertNull",
            vec![
                ("value", "Value"),
                ("message", "String"),
            ],
        ),
        (
            "aura.assert.debugAssert",
            vec![
                ("condition", "Value"),
                ("message", "String"),
            ],
        ),
        // ── std.iter ──
        ("aura.iter.sum", vec![("list", "List")]),
        ("aura.iter.avg", vec![("list", "List")]),
        ("aura.iter.min", vec![("list", "List")]),
        ("aura.iter.max", vec![("list", "List")]),
        ("aura.iter.product", vec![("list", "List")]),
        (
            "aura.iter.contains",
            vec![
                ("list", "List"),
                ("item", "Value"),
            ],
        ),
        (
            "aura.iter.indexOf",
            vec![
                ("list", "List"),
                ("item", "Value"),
            ],
        ),
        ("aura.iter.count", vec![("list", "List")]),
        (
            "aura.iter.every",
            vec![
                ("list", "List"),
                ("predicate", "Value"),
            ],
        ),
        (
            "aura.iter.some",
            vec![
                ("list", "List"),
                ("predicate", "Value"),
            ],
        ),
        (
            "aura.iter.flatMap",
            vec![
                ("list", "List"),
                ("fn", "Value"),
            ],
        ),
        ("aura.iter.zip", vec![]),
        ("aura.iter.unzip", vec![("list", "List")]),
        ("aura.iter.enumerate", vec![("list", "List")]),
        ("aura.iter.chain", vec![]),
        (
            "aura.iter.take",
            vec![
                ("list", "List"),
                ("n", "Int"),
            ],
        ),
        (
            "aura.iter.skip",
            vec![
                ("list", "List"),
                ("n", "Int"),
            ],
        ),
        (
            "aura.iter.dropWhile",
            vec![
                ("list", "List"),
                ("predicate", "Value"),
            ],
        ),
        (
            "aura.iter.takeWhile",
            vec![
                ("list", "List"),
                ("predicate", "Value"),
            ],
        ),
        ("aura.iter.distinct", vec![("list", "List")]),
        (
            "aura.iter.groupBy",
            vec![
                ("list", "List"),
                ("keyFn", "Value"),
            ],
        ),
        (
            "aura.iter.partition",
            vec![
                ("list", "List"),
                ("predicate", "Value"),
            ],
        ),
        (
            "aura.iter.fold",
            vec![
                ("list", "List"),
                ("init", "Value"),
                ("fn", "Value"),
            ],
        ),
        (
            "aura.iter.scan",
            vec![
                ("list", "List"),
                ("init", "Value"),
                ("fn", "Value"),
            ],
        ),
        (
            "aura.iter.toMap",
            vec![
                ("list", "List"),
                ("keyFn", "Value"),
                ("valueFn", "Value"),
            ],
        ),
        ("aura.iter.toList", vec![("value", "Value")]),
        (
            "aura.iter.range",
            vec![
                ("from", "Int"),
                ("to", "Int"),
            ],
        ),
        (
            "aura.iter.rangeTo",
            vec![
                ("from", "Int"),
                ("to", "Int"),
            ],
        ),
        (
            "aura.iter.rangeUntil",
            vec![
                ("from", "Int"),
                ("to", "Int"),
            ],
        ),
        (
            "aura.iter.repeatN",
            vec![
                ("value", "Value"),
                ("count", "Int"),
            ],
        ),
    ]
}
