//! 语义检查器（P3 核心）
//!
//! 单遍遍历 AST：
//! 1. 收集顶层声明到符号表（函数签名、类型定义）
//! 2. 检查函数体：推断表达式类型，验证类型匹配
//! 3. 空安全检查（nullable 上的非法操作）
//! 4. 生成诊断错误（带 Span）

use crate::Span;
use crate::ast::*;
use crate::errors::{CompileError, ErrorSeverity};
use crate::sema::info::SemaInfo;
use crate::sema::symbol::{ParamSym, Symbol, SymbolKind, SymbolTable, ast_type_to_ty};
use crate::sema::ty::Ty;
use std::collections::{HashMap, HashSet};

/// 语义分析结果
pub struct SemanticResult {
    pub errors: Vec<CompileError>,
    pub symbols: SymbolTable,
    /// sema → codegen 信息通道（表达式类型等）
    pub info: SemaInfo,
}

/// 类的继承相关属性标记（Kotlin：类默认 final，仅 open / abstract / sealed 可被继承）
#[derive(Debug, Clone, Copy, Default)]
struct ClassAttrs {
    is_open: bool,
    is_abstract: bool,
}

/// 语义检查器
pub struct Checker {
    symbols: SymbolTable,
    errors: Vec<CompileError>,
    /// 变量类型环境：作用域栈（与符号表同步）
    var_env: Vec<HashMap<String, Ty>>,
    /// 当前函数的返回类型
    current_fn_return: Option<Ty>,
    /// 当前函数是否有返回值（用于 return 检查）
    in_loop_depth: usize,

    // ── P3 增强：类型层级与约束信息 ──
    /// 枚举变体：枚举名 -> 变体名列表（用于 when 穷举性检查）
    enum_variants: HashMap<String, Vec<String>>,
    /// 泛型类型参数约束：类型/函数名 -> 第 i 个类型参数的约束类型列表（无约束为 None）
    generic_bounds: HashMap<String, Vec<Option<Ty>>>,
    /// 类的成员可见性：键 `"Type.member"` -> 可见性（用于 private 成员访问检查）
    member_visibility: HashMap<String, Visibility>,
    /// 类型 -> 其方法名集合（用于 override 检查）
    class_methods: HashMap<String, Vec<String>>,
    /// 密封类型集合（sealed class/struct），用于 P3.9 继承检查
    sealed_types: HashSet<String>,
    /// 接口类型集合（用于区分 superclass 是类还是接口）
    interface_types: HashSet<String>,
    /// 类的继承属性：类名 -> open/abstract 标记（P0 继承开放性检查）
    class_attrs: HashMap<String, ClassAttrs>,
    /// 类名 -> 标记为 open 的方法名集合（override 合法性检查）
    open_methods: HashMap<String, HashSet<String>>,
    /// 类/结构体名 -> 运算符重载方法名集合（operator fun，P-K2）
    operator_methods: HashMap<String, HashSet<String>>,
    /// 类名 -> 父类名（子类赋值兼容检查，P-K2）
    superclasses: HashMap<String, String>,
    /// 当前正在检查的类型上下文（用于 private 成员访问判断）
    current_type: Option<String>,

    // ── P3.10 增强：suspend 函数追踪（Phase 1 await 语义修正） ──
    /// 当前是否处于 suspend/async 函数体内
    is_in_suspend_fn: bool,
    /// 被标记为 suspend/async 的函数名集合（用于调用检查）
    suspend_functions: HashSet<String>,

    /// sema → codegen 信息通道（表达式类型记录）
    info: SemaInfo,
}

impl Checker {
    pub fn new() -> Self {
        let mut symbols = SymbolTable::new();
        // 预置基础类型
        for t in [
            "Int", "Long", "Short", "Byte", "Float", "Double", "Boolean", "Char", "String", "Any",
            "Nothing", "Unit",
        ] {
            symbols.register_type(
                t,
                Ty::from_ast(&Type::Named {
                    name: t.to_string(),
                    span: Span::single(0, 1, 1),
                }),
            );
        }

        // 预置内置函数（std 内置，简化版）
        let builtin_span = Span::single(0, 1, 1);
        let _ = symbols.insert_function(
            "println",
            vec![ParamSym {
                name: "message".into(),
                ty: Ty::Any,
                has_default: false,
                is_vararg: false,
            }],
            Ty::Unit,
            Visibility::Public,
            builtin_span,
        );
        let _ = symbols.insert_function(
            "print",
            vec![ParamSym {
                name: "message".into(),
                ty: Ty::Any,
                has_default: false,
                is_vararg: false,
            }],
            Ty::Unit,
            Visibility::Public,
            builtin_span,
        );
        let _ = symbols.insert_function(
            "listOf",
            // 注册 10 个默认参数，使 listOf 接受 0-10 个任意类型参数
            (0..10)
                .map(|i| ParamSym {
                    name: format!("item{}", i),
                    ty: Ty::Any,
                    has_default: true,
                    is_vararg: false,
                })
                .collect(),
            Ty::List(Box::new(Ty::Any)),
            Visibility::Public,
            builtin_span,
        );

        // P10: 并发运行时内置函数（aura.concurrent.* 命名空间）
        for (name, params, ret) in [
            ("aura.concurrent.spawn", vec![("expr", Ty::Any)], Ty::Int),
            (
                "aura.concurrent.send",
                vec![
                    ("actor", Ty::Int),
                    ("msg", Ty::Any),
                ],
                Ty::Unit,
            ),
            (
                "aura.concurrent.ask",
                vec![
                    ("actor", Ty::Int),
                    ("msg", Ty::Any),
                ],
                Ty::Any,
            ),
            (
                "aura.concurrent.newChannel",
                vec![("bound", Ty::Int)],
                Ty::Int,
            ),
            (
                "aura.concurrent.channelSend",
                vec![
                    ("ch", Ty::Int),
                    ("val", Ty::Any),
                ],
                Ty::Unit,
            ),
            (
                "aura.concurrent.channelRecv",
                vec![("ch", Ty::Int)],
                Ty::Any,
            ),
            (
                "aura.concurrent.channelTryRecv",
                vec![("ch", Ty::Int)],
                Ty::Any,
            ),
            (
                "aura.concurrent.select",
                vec![
                    ("ch1", Ty::Int),
                    ("ch2", Ty::Int),
                ],
                Ty::Any,
            ),
            (
                "aura.concurrent.spawnActor",
                vec![("name", Ty::String)],
                Ty::Int,
            ),
            (
                "aura.concurrent.supervise",
                vec![
                    ("parent", Ty::Int),
                    ("child", Ty::Int),
                ],
                Ty::Unit,
            ),
            (
                "aura.concurrent.actorAlive",
                vec![("id", Ty::Int)],
                Ty::Boolean,
            ),
        ] {
            let _ = symbols.insert_function(
                name,
                params
                    .into_iter()
                    .map(|(pn, pt)| ParamSym {
                        name: pn.into(),
                        ty: pt,
                        has_default: false,
                        is_vararg: false,
                    })
                    .collect(),
                ret,
                Visibility::Public,
                builtin_span,
            );
        }

        // AOT 直调内置函数（Demo 3）
        let _ = symbols.insert_function(
            "load_shared_library",
            vec![ParamSym {
                name: "path".into(),
                ty: Ty::String,
                has_default: false,
                is_vararg: false,
            }],
            Ty::Int,
            Visibility::Public,
            builtin_span,
        );
        let _ = symbols.insert_function(
            "call_func",
            vec![
                ParamSym {
                    name: "module_id".into(),
                    ty: Ty::Int,
                    has_default: false,
                    is_vararg: false,
                },
                ParamSym {
                    name: "func_idx".into(),
                    ty: Ty::Int,
                    has_default: false,
                    is_vararg: false,
                },
                ParamSym {
                    name: "args".into(),
                    ty: Ty::Any,
                    has_default: false,
                    is_vararg: false,
                },
            ],
            Ty::Any,
            Visibility::Public,
            builtin_span,
        );
        let _ = symbols.insert_function(
            "unload_shared_library",
            vec![ParamSym {
                name: "module_id".into(),
                ty: Ty::Int,
                has_default: false,
                is_vararg: false,
            }],
            Ty::Unit,
            Visibility::Public,
            builtin_span,
        );

        let mut var_env = Vec::new();
        var_env.push(HashMap::new());

        Self {
            symbols,
            errors: Vec::new(),
            var_env,
            current_fn_return: None,
            in_loop_depth: 0,
            enum_variants: HashMap::new(),
            generic_bounds: HashMap::new(),
            member_visibility: HashMap::new(),
            class_methods: HashMap::new(),
            sealed_types: HashSet::new(),
            interface_types: HashSet::new(),
            class_attrs: HashMap::new(),
            open_methods: HashMap::new(),
            operator_methods: HashMap::new(),
            superclasses: HashMap::new(),
            current_type: None,
            is_in_suspend_fn: false,
            suspend_functions: HashSet::new(),
            info: SemaInfo::default(),
        }
    }

    /// 分析整个程序
    pub fn analyze(&mut self, program: &Program) {
        // 第一遍：收集所有声明（函数签名、类型）—— 允许前向引用
        for decl in &program.declarations {
            self.collect_declaration(decl);
        }
        // 第一遍补充：顶层 val/var（脚本模式）—— 注册为全局变量
        // Bug fix: 此前 top_level_statements 完全被 sema 忽略，导致
        // `val a = 42` + `fun main() { println(a) }` 报 "unresolved reference 'a'"。
        for stmt in &program.top_level_statements {
            self.collect_top_level_stmt(stmt);
        }
        // 第二遍：检查顶层 val/var 初始化器类型（在函数体之前，以便函数体能看到正确类型）
        for stmt in &program.top_level_statements {
            self.check_top_level_stmt(stmt);
        }
        // 第二遍：检查函数体
        for decl in &program.declarations {
            self.check_declaration(decl);
        }
    }

    pub fn into_result(self) -> SemanticResult {
        SemanticResult {
            errors: self.errors,
            symbols: self.symbols,
            info: self.info,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 第一遍：收集声明
    // ═══════════════════════════════════════════════════════════════════════

    fn collect_declaration(&mut self, decl: &Decl) {
        match decl {
            Decl::Function(f) => {
                self.record_generic_bounds(&f.name, &f.type_params);
                let params = f
                    .params
                    .iter()
                    .map(|p| {
                        let mut ty = p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any);
                        // vararg 参数类型为元素类型的数组
                        if p.is_vararg {
                            ty = Ty::Array(Box::new(ty));
                        }
                        ParamSym {
                            name: p.name.clone(),
                            ty,
                            has_default: p.default_value.is_some(),
                            is_vararg: p.is_vararg,
                        }
                    })
                    .collect();
                let ret = f.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                if crate::std::decl::is_prelude(&f.name) {
                    self.report(
                        f.span,
                        format!("cannot redefine prelude function '{}'", f.name),
                    );
                } else if let Err(dup) = self.symbols.insert_function(
                    f.name.clone(),
                    params,
                    ret.clone(),
                    f.visibility,
                    f.span,
                ) {
                    self.report(f.span, format!("duplicate function '{}'", dup));
                }
                // Phase 1: 追踪 suspend/async 函数
                if f.modifiers.iter().any(|m| matches!(m, FnModifier::Suspend | FnModifier::Async))
                {
                    self.suspend_functions.insert(f.name.clone());
                }
                let _ = ret;
            }
            Decl::Struct(s) => {
                self.symbols.register_type(s.name.clone(), Ty::Named(s.name.clone()));
                self.record_generic_bounds(&s.name, &s.type_params);
                self.record_members(&s.name, &s.fields, &s.methods);
                if s.sealed {
                    self.sealed_types.insert(s.name.clone());
                }
                // P-K2：struct 运算符重载方法表
                let mut ops = HashSet::new();
                for m in &s.methods {
                    if m.modifiers.iter().any(|x| matches!(x, FnModifier::Operator)) {
                        ops.insert(m.name.clone());
                    }
                }
                self.operator_methods.insert(s.name.clone(), ops);
                // 收集 struct 方法（作为函数）
                for m in &s.methods {
                    let params = m
                        .params
                        .iter()
                        .map(|p| ParamSym {
                            name: p.name.clone(),
                            ty: p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any),
                            has_default: p.default_value.is_some(),
                            is_vararg: p.is_vararg,
                        })
                        .collect();
                    let ret = m.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                    let full_name = format!("{}.{}", s.name, m.name);
                    let _ = self.symbols.insert_function(
                        full_name.clone(),
                        params,
                        ret,
                        m.visibility,
                        m.span,
                    );
                    // Phase 1: 追踪 suspend 方法
                    if m.modifiers
                        .iter()
                        .any(|mod_| matches!(mod_, FnModifier::Suspend | FnModifier::Async))
                    {
                        self.suspend_functions.insert(full_name);
                    }
                }
            }
            Decl::Enum(e) => {
                self.symbols.register_type(e.name.clone(), Ty::Named(e.name.clone()));
                self.enum_variants.insert(
                    e.name.clone(),
                    e.variants.iter().map(|v| v.name.clone()).collect(),
                );
            }
            Decl::Class(c) => {
                self.symbols.register_type(c.name.clone(), Ty::Named(c.name.clone()));
                self.record_generic_bounds(&c.name, &c.type_params);
                self.record_members(&c.name, &c.fields, &c.methods);
                // 收集 class 方法（作为函数）— 与 struct 一致
                for m in &c.methods {
                    let params = m
                        .params
                        .iter()
                        .map(|p| ParamSym {
                            name: p.name.clone(),
                            ty: p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any),
                            has_default: p.default_value.is_some(),
                            is_vararg: p.is_vararg,
                        })
                        .collect();
                    let ret = m.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                    let full_name = format!("{}.{}", c.name, m.name);
                    let _ = self.symbols.insert_function(
                        full_name.clone(),
                        params,
                        ret,
                        m.visibility,
                        m.span,
                    );
                    // Phase 1: 追踪 suspend 方法
                    if m.modifiers
                        .iter()
                        .any(|mod_| matches!(mod_, FnModifier::Suspend | FnModifier::Async))
                    {
                        self.suspend_functions.insert(full_name);
                    }
                }
                if c.sealed {
                    self.sealed_types.insert(c.name.clone());
                }

                // P0：open/abstract 类属性（继承开放性检查用）
                let is_open = c.modifiers.iter().any(|m| matches!(m, ClassModifier::Open));
                let is_abstract = c.modifiers.iter().any(|m| matches!(m, ClassModifier::Abstract));
                self.class_attrs.insert(
                    c.name.clone(),
                    ClassAttrs {
                        is_open,
                        is_abstract,
                    },
                );
                // P0：open/abstract 方法集合（override 合法性检查用；abstract 方法天然可重写）
                let mut open_set = HashSet::new();
                for m in &c.methods {
                    if m.modifiers
                        .iter()
                        .any(|x| matches!(x, FnModifier::Open | FnModifier::Abstract))
                    {
                        open_set.insert(m.name.clone());
                    }
                }
                self.open_methods.insert(c.name.clone(), open_set);
                // P-K2：运算符重载方法表 + 继承链
                let mut ops = HashSet::new();
                for m in &c.methods {
                    if m.modifiers.iter().any(|x| matches!(x, FnModifier::Operator)) {
                        ops.insert(m.name.clone());
                    }
                }
                self.operator_methods.insert(c.name.clone(), ops);
                if let Some(sc) = &c.superclass {
                    self.superclasses.insert(c.name.clone(), sc.clone());
                }
                // P0/P1：abstract 方法校验（必须位于 abstract 类且无方法体）
                for m in &c.methods {
                    if m.modifiers.iter().any(|x| matches!(x, FnModifier::Abstract)) {
                        if m.body.is_some() {
                            self.report(
                                m.span,
                                format!("abstract function '{}' cannot have a body", m.name),
                            );
                        }
                        if !is_abstract {
                            self.report(
                                m.span,
                                format!(
                                    "abstract function '{}' is only allowed inside an abstract class",
                                    m.name
                                ),
                            );
                        }
                    }
                }
                // P1：伴生对象（Kotlin：一个类最多一个 companion object）
                if c.companion_objects.len() > 1 {
                    self.report(
                        c.span,
                        format!("class '{}' can have only one companion object", c.name),
                    );
                }
                for co in &c.companion_objects {
                    // 伴生成员并入类成员表（不可重复调用 record_members，其会覆盖 class_methods）
                    for f in &co.fields {
                        self.member_visibility
                            .insert(format!("{}.{}", c.name, f.name), f.visibility);
                    }
                    if let Some(set) = self.class_methods.get_mut(&c.name) {
                        for m in &co.methods {
                            self.member_visibility
                                .insert(format!("{}.{}", c.name, m.name), m.visibility);
                            set.push(m.name.clone());
                        }
                    }
                    // 伴生方法注册为 `Class.method` 函数（可经类名调用）
                    for m in &co.methods {
                        let params = m
                            .params
                            .iter()
                            .map(|p| ParamSym {
                                name: p.name.clone(),
                                ty: p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any),
                                has_default: p.default_value.is_some(),
                                is_vararg: p.is_vararg,
                            })
                            .collect();
                        let ret = m.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                        let full_name = format!("{}.{}", c.name, m.name);
                        let _ = self.symbols.insert_function(
                            full_name,
                            params,
                            ret,
                            m.visibility,
                            m.span,
                        );
                    }
                }

                // 修饰符组合校验
                let has_value = c.modifiers.iter().any(|m| *m == ClassModifier::Value);
                if has_value && c.superclass.is_some() {
                    self.report(
                        c.span,
                        format!(
                            "value class '{}' cannot have a superclass: value types have no vtable and cannot participate in inheritance",
                            c.name
                        ),
                    );
                }
            }
            Decl::Interface(i) => {
                self.symbols.register_type(i.name.clone(), Ty::Named(i.name.clone()));
                self.interface_types.insert(i.name.clone());
                self.record_generic_bounds(&i.name, &i.type_params);
                self.record_members(&i.name, &[], &i.methods);
            }
            Decl::Actor(a) => {
                self.symbols.register_type(a.name.clone(), Ty::Named(a.name.clone()));
                self.record_members(&a.name, &a.fields, &a.methods);
                // Phase 1: 追踪 suspend 方法
                for m in &a.methods {
                    let full_name = format!("{}.{}", a.name, m.name);
                    if m.modifiers
                        .iter()
                        .any(|mod_| matches!(mod_, FnModifier::Suspend | FnModifier::Async))
                    {
                        self.suspend_functions.insert(full_name);
                    }
                }
            }
            Decl::TypeAlias(t) => {
                let target = ast_type_to_ty(t.aliased_type.as_ref());
                self.symbols.register_type(t.name.clone(), target);
            }
            Decl::Extern(e) => {
                // P8-Rust: extern "rust" 块给出 Rust 侧标注提示
                if e.abi == "rust" || e.abi == "Rust" {
                    self.report_warning(
                        e.span,
                        "`extern \"rust\"` 块的函数需在 Rust 侧使用 #[no_mangle] extern \"C\"，\
                         否则 ABI 不稳定，可能导致调用失败",
                    );
                }
                for f in &e.functions {
                    let params = f
                        .params
                        .iter()
                        .map(|p| ParamSym {
                            name: p.name.clone(),
                            ty: p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any),
                            has_default: false,
                            is_vararg: p.is_vararg,
                        })
                        .collect();
                    let ret = f.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                    let _ = self.symbols.insert_function(
                        f.name.clone(),
                        params,
                        ret,
                        f.visibility,
                        f.span,
                    );
                }
            }
            Decl::ExternInterface(e) => {
                // extern interface: 校验必须包含 default fun loadLibrary()
                let has_load_library = e.functions.iter().any(|f| {
                    f.name == "loadLibrary" && f.modifiers.iter().any(|m| m == &FnModifier::Default)
                });
                if !has_load_library {
                    self.report(
                        e.span,
                        format!(
                            "extern interface `{}` 必须包含 `default fun loadLibrary(): String = \"...\"` 方法",
                            e.name
                        ),
                    );
                }
                // 注册接口函数到符号表
                for f in &e.functions {
                    if f.name == "loadLibrary" {
                        continue; // loadLibrary 是内部方法，不注册为接口函数
                    }
                    let qualified_name = format!("{}.{}", e.name, f.name);
                    let params = f
                        .params
                        .iter()
                        .map(|p| ParamSym {
                            name: p.name.clone(),
                            ty: p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any),
                            has_default: false,
                            is_vararg: p.is_vararg,
                        })
                        .collect();
                    let ret = f.return_type.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Unit);
                    let _ = self.symbols.insert_function(
                        qualified_name,
                        params,
                        ret,
                        f.visibility,
                        f.span,
                    );
                }
            }
            _ => {}
        }
    }

    // ── P3 增强：类型层级 / 约束信息收集 ──

    fn record_generic_bounds(&mut self, name: &str, params: &[TypeParam]) {
        if params.is_empty() {
            return;
        }
        let bounds: Vec<Option<Ty>> = params
            .iter()
            .map(
                |p| {
                    if p.bounds.is_empty() { None } else { Some(ast_type_to_ty(&p.bounds[0])) }
                },
            )
            .collect();
        self.generic_bounds.insert(name.to_string(), bounds);
    }

    fn record_members(&mut self, type_name: &str, fields: &[StructField], methods: &[FnDecl]) {
        for f in fields {
            self.member_visibility.insert(format!("{}.{}", type_name, f.name), f.visibility);
        }
        let mut mnames = Vec::new();
        for m in methods {
            self.member_visibility.insert(format!("{}.{}", type_name, m.name), m.visibility);
            mnames.push(m.name.clone());
        }
        self.class_methods.insert(type_name.to_string(), mnames);
    }

    /// 将 AST 类型转换为语义类型，并校验泛型实参是否满足声明约束（P3.7）
    fn check_type(&mut self, ty: &Type) -> Ty {
        if let Type::Generic {
            name, args, ..
        } = ty
        {
            if let Some(bounds) = self.generic_bounds.get(name) {
                let bounds = bounds.clone();
                for (i, arg) in args.iter().enumerate() {
                    if let Some(Some(bound)) = bounds.get(i) {
                        let bound = bound.clone();
                        let arg_ty = ast_type_to_ty(arg);
                        if arg_ty != Ty::Any && arg_ty != Ty::Error && !arg_ty.can_assign_to(&bound)
                        {
                            self.report(
                                arg.span(),
                                format!(
                                    "type argument '{}' does not satisfy bound '{}'",
                                    arg_ty.name(),
                                    bound.name()
                                ),
                            );
                        }
                    }
                }
            }
        }
        let resolved = ast_type_to_ty(ty);
        // Bug fix: 解析类型别名（typealias）—— Ty::Named("Answer") -> Ty::Int
        // 此前 check_type 仅做 AST→Ty 转换，从不查询 symbols.types 索引，
        // 导致 `typealias Answer = Int` 后 `val a: Answer = 42` 误报 type mismatch。
        match resolved {
            Ty::Named(ref name) if let Some(target) = self.symbols.lookup_type(name) => {
                if *target != Ty::Named(name.clone()) { target.clone() } else { resolved }
            }
            other => other,
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 第二遍：检查声明体
    // ═══════════════════════════════════════════════════════════════════════

    fn check_declaration(&mut self, decl: &Decl) {
        match decl {
            Decl::Function(f) => {
                self.check_function_body(f);
            }
            Decl::Struct(s) => {
                // struct 字段类型检查 + 方法体
                let saved_type = self.current_type.take();
                self.current_type = Some(s.name.clone());
                self.symbols.enter_scope(true);
                for field in &s.fields {
                    let ft =
                        field.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                    if let Some(def) = &field.default_value {
                        let dt = self.check_expr(def);
                        if !dt.can_assign_to(&ft) {
                            self.report(
                                def.span(),
                                format!(
                                    "default value type mismatch: expected '{}', got '{}'",
                                    ft.name(),
                                    dt.name()
                                ),
                            );
                        }
                    }
                    // 属性访问器（get/set）：`field` 引用底层字段
                    if let Some(acc) = &field.accessors {
                        if let Some(g) = &acc.getter {
                            self.check_accessor(g, ft.clone());
                        }
                        if let Some(st) = &acc.setter {
                            self.check_accessor(st, ft.clone());
                        }
                    }
                    let name = format!("{}.{}", s.name, field.name);
                    self.define_var_env(&name, ft, field.is_mutable);
                }
                for m in &s.methods {
                    self.check_function_body(m);
                }
                // init 块 / 次构造函数 / 伴生对象（struct 与 class 成员模型一致）
                for b in &s.init_blocks {
                    self.check_expr(b);
                }
                for ctor in &s.constructors {
                    self.check_constructor(ctor);
                }
                for co in &s.companion_objects {
                    for b in &co.init_blocks {
                        self.check_expr(b);
                    }
                    for m in &co.methods {
                        self.check_function_body(m);
                    }
                }
                self.symbols.exit_scope();
                self.current_type = saved_type;
            }
            Decl::Class(c) => {
                let saved_type = self.current_type.take();
                self.current_type = Some(c.name.clone());
                self.symbols.enter_scope(true);
                for field in &c.fields {
                    let ft =
                        field.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                    // 属性访问器（get/set）：`field` 引用底层字段
                    if let Some(acc) = &field.accessors {
                        if let Some(g) = &acc.getter {
                            self.check_accessor(g, ft.clone());
                        }
                        if let Some(st) = &acc.setter {
                            self.check_accessor(st, ft.clone());
                        }
                    }
                    let name = format!("{}.{}", c.name, field.name);
                    self.define_var_env(&name, ft, field.is_mutable);
                }
                // companion 字段登记为 `Class.field`（类名静态访问）
                for co in &c.companion_objects {
                    for f in &co.fields {
                        let ft =
                            f.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                        let name = format!("{}.{}", c.name, f.name);
                        self.define_var_env(&name, ft, f.is_mutable);
                    }
                }
                // P0：继承开放性检查（Kotlin：类默认 final，仅 open/abstract/sealed 可被继承；
                // 接口天然可被实现）
                if let Some(sn) = &c.superclass {
                    if !self.interface_types.contains(sn) && !self.sealed_types.contains(sn) {
                        if let Some(&attrs) = self.class_attrs.get(sn) {
                            if !attrs.is_open && !attrs.is_abstract {
                                self.report(
                                    c.span,
                                    format!(
                                        "cannot inherit from non-open class '{}' (mark it 'open' to allow subclassing)",
                                        sn
                                    ),
                                );
                            }
                        }
                    }
                }
                // override 一致性检查（P3.9）+ open 合法性检查（P0）
                let super_methods: Vec<String> = c
                    .superclass
                    .as_ref()
                    .and_then(|sn| self.class_methods.get(sn).cloned())
                    .unwrap_or_default();
                for m in &c.methods {
                    let has_override =
                        m.modifiers.iter().any(|x| matches!(x, FnModifier::Override));
                    let base_has = super_methods.contains(&m.name);
                    // 基类方法是否 open（接口方法天然 open，可被重写）
                    let base_open = if c
                        .superclass
                        .as_ref()
                        .map_or(false, |sn| self.interface_types.contains(sn))
                    {
                        true
                    } else {
                        c.superclass
                            .as_ref()
                            .and_then(|sn| self.open_methods.get(sn))
                            .map_or(false, |s| s.contains(&m.name))
                    };
                    if has_override && !base_has {
                        self.report(
                            m.span,
                            format!(
                                "'{}' is marked 'override' but no matching method in base class",
                                m.name
                            ),
                        );
                    } else if has_override && base_has && !base_open {
                        self.report(
                            m.span,
                            format!(
                                "'{}' in base class is not open; only open or abstract methods can be overridden",
                                m.name
                            ),
                        );
                    } else if !has_override && base_has {
                        self.report_warning(
                            m.span,
                            format!(
                                "'{}' overrides a method in base class but is missing the 'override' modifier",
                                m.name
                            ),
                        );
                    }
                }
                // 接口实现完整性检查（P3.9）：实现的每个接口，其声明的抽象方法都必须由本类提供
                let mut implemented = c.implementations.clone();
                if let Some(sn) = &c.superclass {
                    if self.interface_types.contains(sn) {
                        implemented.push(sn.clone());
                    }
                }
                let own_methods = self.class_methods.get(&c.name).cloned().unwrap_or_default();
                // 仅当 superclass 是类（而非接口）时，其方法才算“已继承”；
                // 接口方法是抽象声明，必须由本类自行实现，不能视为已继承。
                let inherited =
                    if c.superclass.as_ref().map_or(false, |sn| !self.interface_types.contains(sn))
                    {
                        c.superclass
                            .as_ref()
                            .and_then(|sn| self.class_methods.get(sn).cloned())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                for iface in &implemented {
                    let iface_methods = self.class_methods.get(iface).cloned().unwrap_or_default();
                    if iface_methods.is_empty() {
                        continue;
                    }
                    for m in &iface_methods {
                        if !own_methods.contains(m) && !inherited.contains(m) {
                            self.report(
                                c.span,
                                format!(
                                    "class '{}' does not implement interface '{}' method '{}'",
                                    c.name, iface, m
                                ),
                            );
                        }
                    }
                }
                // P0/P1：init 块（字段已在作用域内，可裸引用成员）
                for b in &c.init_blocks {
                    self.check_expr(b);
                }
                // P1：次构造函数 / init 构造函数
                for ctor in &c.constructors {
                    self.check_constructor(ctor);
                }
                // P1：伴生对象成员
                for co in &c.companion_objects {
                    for b in &co.init_blocks {
                        self.check_expr(b);
                    }
                    for m in &co.methods {
                        self.check_function_body(m);
                    }
                }
                for m in &c.methods {
                    self.check_function_body(m);
                }
                self.symbols.exit_scope();
                self.current_type = saved_type;
            }
            Decl::Actor(a) => {
                let saved_type = self.current_type.take();
                self.current_type = Some(a.name.clone());
                self.symbols.enter_scope(true);
                for field in &a.fields {
                    let ft =
                        field.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                    let name = format!("{}.{}", a.name, field.name);
                    self.define_var_env(&name, ft, field.is_mutable);
                }
                for m in &a.methods {
                    self.check_function_body(m);
                }
                self.symbols.exit_scope();
                self.current_type = saved_type;
            }
            Decl::Interface(i) => {
                for m in &i.methods {
                    self.check_function_body(m);
                }
            }
            Decl::Extern(_)
            | Decl::ExternInterface(_)
            | Decl::Annotation(_)
            | Decl::Enum(_)
            | Decl::TypeAlias(_) => {}
            Decl::Import(imp) => {
                self.expand_import(imp);
            }
        }
    }

    /// 展开 import 声明到符号表
    ///
    /// 支持语法：
    /// - `import aura.math.*` — 通配：把模块所有函数加到符号表（短名）
    /// - `import aura.math` — 模块：注册模块名（调用时用 aura.math.sin）
    /// - `import aura.math.sin` — 精确：只加指定函数（短名）
    /// - `import aura.math.sin as s` — 精确引入并别名：用别名调用
    /// - `import aura.math as m` — 别名：用别名注册模块
    /// - `import aura.math.* as m` — 通配+别名：用别名注册模块
    fn expand_import(&mut self, imp: &ImportDecl) {
        let module_path = imp.path.clone();

        // 检查是否是 aura.* 命名空间
        if !module_path.starts_with("aura.") {
            // 非 std 模块，跳过（未来支持第三方库）
            return;
        }

        match &imp.alias {
            Some(alias) => {
                // import aura.math as m / import aura.math.* as m
                // 注册别名到符号表，调用时用 m.sin(...)
                self.symbols.insert_module_alias(alias.clone(), module_path);
            }
            None => {
                if imp.wildcard {
                    // import aura.math.*
                    // 把模块所有函数加到符号表（短名）
                    let short_names = crate::std::decl::module_functions(&module_path);
                    for short_name in short_names {
                        let _ = self.symbols.insert_function(
                            short_name.clone(),
                            vec![], // 参数类型未知，用 Any
                            Ty::Any,
                            Visibility::Public,
                            imp.span,
                        );
                    }
                } else if module_path.split('.').count() == 3 {
                    // import aura.math.sin — 精确引入函数
                    let short_name = module_path.split('.').last().unwrap_or("").to_string();
                    let _ = self.symbols.insert_function(
                        short_name,
                        vec![],
                        Ty::Any,
                        Visibility::Public,
                        imp.span,
                    );
                } else {
                    // import aura.math — 模块引用
                    // 注册模块名，调用时用 aura.math.sin(...)
                    self.symbols.insert_module(module_path.clone());
                }
            }
        }
    }

    fn check_function_body(&mut self, f: &FnDecl) {
        self.symbols.enter_scope(true);
        self.var_env.push(HashMap::new());

        // P2：tailrec 校验（Kotlin：标记 tailrec 的函数必须存在自递归调用）
        if f.modifiers.iter().any(|m| matches!(m, FnModifier::Tailrec)) {
            let has_recursion = f.body.as_ref().map_or(false, |b| Self::expr_calls(b, &f.name));
            if !has_recursion {
                self.report(
                    f.span,
                    format!(
                        "function '{}' is marked 'tailrec' but contains no recursive call",
                        f.name
                    ),
                );
            }
        }

        // Phase 1: 追踪 suspend 状态
        let saved_suspend = self.is_in_suspend_fn;
        self.is_in_suspend_fn =
            f.modifiers.iter().any(|m| matches!(m, FnModifier::Suspend | FnModifier::Async));

        let saved_ret = self.current_fn_return.take();
        self.current_fn_return =
            Some(f.return_type.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Unit));

        // 参数进入作用域
        for p in &f.params {
            let mut pt = p.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
            // vararg 参数类型为元素类型的数组
            if p.is_vararg {
                pt = Ty::Array(Box::new(pt));
            }
            let _ = self.symbols.insert(Symbol::new(
                p.name.clone(),
                SymbolKind::Variable {
                    is_mutable: true,
                },
                Visibility::Private,
                p.span,
            ));
            self.define_var_env(&p.name, pt, true);
        }

        if let Some(body) = &f.body {
            let body_ty = self.check_expr(body);
            let expected = self.current_fn_return.clone().unwrap_or(Ty::Unit);
            // 非 Unit 返回类型的函数，其 body 必须能赋值给返回类型（或显式 return）
            if expected != Ty::Unit && body_ty.can_assign_to(&expected) {
                // ok
            }
        }

        self.current_fn_return = saved_ret;
        self.is_in_suspend_fn = saved_suspend;
        self.var_env.pop();
        self.symbols.exit_scope();
    }

    /// 检查次构造函数 / init 构造函数体（参数进入作用域，成员可裸引用）
    fn check_constructor(&mut self, ctor: &ConstructorDecl) {
        self.symbols.enter_scope(true);
        self.var_env.push(HashMap::new());
        for p in &ctor.params {
            let pt = p.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
            let _ = self.symbols.insert(Symbol::new(
                p.name.clone(),
                SymbolKind::Variable {
                    is_mutable: true,
                },
                Visibility::Private,
                p.span,
            ));
            self.define_var_env(&p.name, pt, true);
        }
        if let Some(d) = &ctor.delegation {
            for a in &d.args {
                self.check_expr(a);
            }
        }
        if let Some(body) = &ctor.body {
            self.check_expr(body);
        }
        self.var_env.pop();
        self.symbols.exit_scope();
    }

    /// 检查属性访问器体：`field` 上下文关键字绑定为底层字段（Kotlin 软关键字）；
    /// setter 参数未显式标注类型时继承属性类型（Kotlin 语义）
    fn check_accessor(&mut self, acc: &AccessorDecl, field_ty: Ty) {
        self.symbols.enter_scope(true);
        self.var_env.push(HashMap::new());
        self.define_var_env("field", field_ty.clone(), true);
        if let Some(p) = &acc.param {
            let pt = p.type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(field_ty.clone());
            let _ = self.symbols.insert(Symbol::new(
                p.name.clone(),
                SymbolKind::Variable {
                    is_mutable: true,
                },
                Visibility::Private,
                p.span,
            ));
            self.define_var_env(&p.name, pt, true);
        }
        self.check_expr(&acc.body);
        self.var_env.pop();
        self.symbols.exit_scope();
    }

    /// 递归遍历表达式树，检查是否存在对 `name` 的直接自调用（tailrec 校验用）
    fn expr_calls(e: &Expr, name: &str) -> bool {
        match e {
            Expr::Call {
                callee,
                args,
                ..
            } => {
                let direct = matches!(callee.as_ref(), Expr::Ident(n, _) if n == name);
                direct
                    || Self::expr_calls(callee, name)
                    || args.iter().any(|a| Self::expr_calls(a, name))
            }
            Expr::Literal(..) | Expr::Ident(..) | Expr::This(_) => false,
            Expr::Break { .. } | Expr::Continue { .. } => false,
            Expr::Assign {
                target,
                value,
                ..
            } => Self::expr_calls(target, name) || Self::expr_calls(value, name),
            Expr::Binary {
                lhs, rhs, ..
            }
            | Expr::Elvis {
                lhs, rhs, ..
            } => Self::expr_calls(lhs, name) || Self::expr_calls(rhs, name),
            Expr::Unary {
                operand, ..
            }
            | Expr::AssertNonNull {
                expr: operand,
                ..
            }
            | Expr::Await {
                expr: operand,
                ..
            }
            | Expr::TypeCast {
                expr: operand,
                ..
            } => Self::expr_calls(operand, name),
            Expr::NamedArg { value, .. } => Self::expr_calls(value, name),
            Expr::MemberAccess { object, .. } | Expr::SafeAccess { object, .. } => {
                Self::expr_calls(object, name)
            }
            Expr::Index {
                container,
                index,
                ..
            } => Self::expr_calls(container, name) || Self::expr_calls(index, name),
            Expr::Lambda { body, .. } | Expr::Closure { body, .. } => Self::expr_calls(body, name),
            Expr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                Self::expr_calls(condition, name)
                    || Self::expr_calls(then_branch, name)
                    || else_branch.as_ref().is_some_and(|b| Self::expr_calls(b, name))
            }
            Expr::When {
                subject,
                arms,
                ..
            } => {
                subject.as_ref().is_some_and(|s| Self::expr_calls(s, name))
                    || arms.iter().any(|a| {
                        a.patterns.iter().any(|p| Self::expr_calls(p, name))
                            || a.guard.as_ref().is_some_and(|g| Self::expr_calls(g, name))
                            || Self::expr_calls(&a.body, name)
                    })
            }
            Expr::For {
                pattern,
                iterable,
                body,
                ..
            } => {
                Self::expr_calls(pattern, name)
                    || Self::expr_calls(iterable, name)
                    || Self::expr_calls(body, name)
            }
            Expr::While {
                condition,
                body,
                ..
            }
            | Expr::DoWhile {
                condition,
                body,
                ..
            } => Self::expr_calls(condition, name) || Self::expr_calls(body, name),
            Expr::Return { value, .. } => value.as_ref().is_some_and(|v| Self::expr_calls(v, name)),
            Expr::Throw { value, .. } => Self::expr_calls(value, name),
            Expr::Try {
                block,
                catches,
                finally,
                ..
            } => {
                Self::expr_calls(block, name)
                    || catches.iter().any(|c| Self::expr_calls(&c.body, name))
                    || finally.as_ref().is_some_and(|f| Self::expr_calls(f, name))
            }
            Expr::New { args, .. } => args.iter().any(|a| Self::expr_calls(a, name)),
            Expr::Destructure {
                patterns,
                expr,
                ..
            } => patterns.iter().any(|p| Self::expr_calls(p, name)) || Self::expr_calls(expr, name),
            Expr::Range {
                start, end, ..
            } => {
                start.as_ref().is_some_and(|s| Self::expr_calls(s, name))
                    || end.as_ref().is_some_and(|e| Self::expr_calls(e, name))
            }
            Expr::InRange { range, .. } => Self::expr_calls(range, name),
            Expr::Defer { block, .. } => Self::expr_calls(block, name),
            Expr::Select {
                branches, ..
            } => branches
                .iter()
                .any(|b| Self::expr_calls(&b.pattern, name) || Self::expr_calls(&b.body, name)),
            Expr::Block(stmts, _) => Self::stmts_calls(stmts, name),
        }
    }

    /// 递归遍历语句列表（配合 `expr_calls`）
    fn stmts_calls(stmts: &[Stmt], name: &str) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::Expr(e) => Self::expr_calls(e, name),
            Stmt::Val {
                initializer,
                ..
            }
            | Stmt::Var {
                initializer,
                ..
            } => initializer.as_ref().is_some_and(|e| Self::expr_calls(e, name)),
            Stmt::Destructure {
                patterns,
                expr,
                ..
            } => patterns.iter().any(|p| Self::expr_calls(p, name)) || Self::expr_calls(expr, name),
            Stmt::Block(ss, _) => Self::stmts_calls(ss, name),
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 类型环境辅助
    // ═══════════════════════════════════════════════════════════════════════

    fn define_var_env(&mut self, name: &str, ty: Ty, _mutable: bool) {
        if let Some(scope) = self.var_env.last_mut() {
            scope.insert(name.to_string(), ty);
        }
        let _ = self.symbols.insert(Symbol::new(
            name,
            SymbolKind::Variable {
                is_mutable: _mutable,
            },
            Visibility::Private,
            Span::single(0, 1, 1),
        ));
    }

    fn lookup_var_ty(&self, name: &str) -> Option<Ty> {
        for scope in self.var_env.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        // 兜底：函数名
        if let Some(fns) = self.symbols.lookup_function(name) {
            if let Some(SymbolKind::Function {
                return_type,
                ..
            }) = fns.first().map(|s| &s.kind)
            {
                return Some(Ty::Function {
                    params: vec![],
                    ret: Box::new(return_type.clone()),
                });
            }
        }
        None
    }

    fn define_local(&mut self, name: &str, ty: Ty) {
        self.define_var_env(name, ty, true);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 诊断
    // ═══════════════════════════════════════════════════════════════════════

    fn report(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(CompileError::new(msg, span));
    }

    fn report_warning(&mut self, span: Span, msg: impl Into<String>) {
        self.errors.push(CompileError {
            message: msg.into(),
            span,
            severity: ErrorSeverity::Warning,
        });
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 表达式类型推断
    // ═══════════════════════════════════════════════════════════════════════

    pub fn check_expr(&mut self, expr: &Expr) -> Ty {
        let ty = self.check_expr_impl(expr);
        // 记录表达式类型（span 键控），供 codegen 降级期做接收者类型分派
        let sp = expr.span();
        self.info.expr_types.insert((sp.start, sp.end), ty.name().to_string());
        ty
    }

    fn check_expr_impl(&mut self, expr: &Expr) -> Ty {
        match expr {
            Expr::Literal(lit, _) => self.literal_type(lit),
            Expr::Ident(name, span) => self.check_ident(name, *span),
            Expr::Assign {
                target,
                value,
                span,
            } => {
                let target_ty = self.check_expr(target);
                let value_ty = self.check_expr(value);
                if !value_ty.can_assign_to(&target_ty) {
                    self.report(
                        *span,
                        format!(
                            "type mismatch in assignment: cannot assign '{}' to '{}'",
                            value_ty.name(),
                            target_ty.name()
                        ),
                    );
                }
                target_ty
            }
            Expr::Binary {
                op,
                lhs,
                rhs,
                span,
            } => self.check_binary(*op, lhs, rhs, *span),
            Expr::Unary {
                op,
                operand,
                span,
            } => self.check_unary(*op, operand, *span),
            Expr::Call {
                callee,
                args,
                span,
            } => self.check_call(callee, args, *span),
            Expr::NamedArg {
                value,
                span,
                ..
            } => {
                let vt = self.check_expr(value);
                // NamedArg 的类型是其值表达式的时间类型
                vt
            }
            Expr::This(span) => {
                // this 引用当前类型
                if let Some(type_name) = &self.current_type {
                    Ty::Named(type_name.clone())
                } else {
                    self.report(*span, "this can only be used inside a class/struct/actor");
                    Ty::Error
                }
            }
            Expr::MemberAccess {
                object,
                name,
                span,
            } => self.check_member(object, name, *span),
            Expr::SafeAccess {
                object,
                name,
                span,
            } => {
                let obj_ty = self.check_expr(object);
                if !obj_ty.is_nullable() {
                    self.report_warning(
                        *span,
                        format!(
                            "unnecessary safe access on non-nullable '{}'",
                            obj_ty.name()
                        ),
                    );
                }
                // 对可空对象安全访问：内部成员类型提升为可空
                let member_ty = match obj_ty.non_null() {
                    Ty::Named(type_name) => {
                        let field = format!("{}.{}", type_name, name);
                        self.lookup_var_ty(&field).unwrap_or_else(|| {
                            self.report(
                                *span,
                                format!("unresolved member '{}' on type '{}'", name, type_name),
                            );
                            Ty::Error
                        })
                    }
                    Ty::String => match name.as_str() {
                        "length" | "size" | "toInt" | "toFloat" => Ty::Int,
                        "isEmpty" | "isNotEmpty" => Ty::Boolean,
                        _ => {
                            self.report(*span, format!("unresolved member '{}' on String", name));
                            Ty::Error
                        }
                    },
                    Ty::List(elem) => match name.as_str() {
                        "size" => Ty::Int,
                        "isEmpty" => Ty::Boolean,
                        "first" | "last" => (**elem).clone(),
                        _ => {
                            self.report(*span, format!("unresolved member '{}' on List", name));
                            Ty::Error
                        }
                    },
                    Ty::Array(elem) => match name.as_str() {
                        "size" => Ty::Int,
                        "isEmpty" => Ty::Boolean,
                        "first" | "last" => (**elem).clone(),
                        _ => {
                            self.report(*span, format!("unresolved member '{}' on Array", name));
                            Ty::Error
                        }
                    },
                    Ty::Any => Ty::Any,
                    _ => {
                        self.report(
                            *span,
                            format!("cannot access member '{}' on '{}'", name, obj_ty.name()),
                        );
                        Ty::Error
                    }
                };
                if member_ty != Ty::Unit && member_ty != Ty::Error {
                    Ty::Nullable(Box::new(member_ty))
                } else {
                    member_ty
                }
            }
            Expr::Index {
                container,
                index,
                span,
            } => {
                let container_ty = self.check_expr(container);
                let idx_ty = self.check_expr(index);
                if !idx_ty.is_integer() && idx_ty != Ty::Any {
                    self.report(*span, format!("index must be Int, got '{}'", idx_ty.name()));
                }
                match container_ty {
                    Ty::List(elem) => *elem,
                    Ty::Array(elem) => *elem,
                    Ty::Map(_, v) => *v,
                    Ty::String => Ty::Char,
                    Ty::Nullable(inner) => match *inner {
                        Ty::List(elem) => *elem,
                        Ty::String => Ty::Char,
                        _ => Ty::Error,
                    },
                    _ => {
                        self.report(
                            *span,
                            format!("cannot index into '{}'", container_ty.name()),
                        );
                        Ty::Error
                    }
                }
            }
            Expr::Lambda {
                params,
                body,
                span,
            } => self.check_lambda(params, body, *span),
            Expr::Closure {
                params,
                body,
                span,
            } => self.check_lambda(params, body, *span),
            Expr::If {
                condition,
                then_branch,
                else_branch,
                span,
            } => self.check_if(condition, then_branch, else_branch, *span),
            Expr::When {
                subject,
                arms,
                span,
            } => self.check_when(subject, arms, *span),
            Expr::Block(stmts, _) => self.check_block(stmts),
            Expr::For {
                pattern,
                iterable,
                body,
                span,
            } => self.check_for(pattern, iterable, body, *span),
            Expr::While {
                condition,
                body,
                span,
            } => self.check_while(condition, body, *span),
            Expr::DoWhile {
                condition,
                body,
                span,
            } => self.check_dowhile(condition, body, *span),
            Expr::Return {
                value,
                span,
            } => self.check_return(value, *span),
            Expr::Break { span } => {
                if self.in_loop_depth == 0 {
                    self.report(*span, "'break' outside of a loop");
                }
                Ty::Nothing
            }
            Expr::Continue { span } => {
                if self.in_loop_depth == 0 {
                    self.report(*span, "'continue' outside of a loop");
                }
                Ty::Nothing
            }
            Expr::Throw {
                value,
                span,
            } => {
                let vt = self.check_expr(value);
                if !vt.is_string() && !vt.can_assign_to(&Ty::Named("Exception".into())) {
                    self.report(*span, format!("cannot throw value of type '{}'", vt.name()));
                }
                Ty::Nothing
            }
            Expr::Try {
                block,
                catches,
                finally,
                span,
            } => self.check_try(block, catches, finally, *span),
            Expr::New {
                type_name,
                args,
                span,
            } => self.check_new(type_name, args, *span),
            Expr::Destructure {
                patterns,
                expr,
                span,
            } => self.check_destructure(patterns, expr, *span),
            Expr::TypeCast {
                expr,
                type_name,
                span: _,
            } => {
                let vt = self.check_expr(expr);
                let tt = ast_type_to_ty(type_name);
                // as 转换无运行时检查（简化）
                let _ = vt;
                tt
            }
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => {
                let st = start.as_deref().map(|e| self.check_expr(e)).unwrap_or(Ty::Int);
                let et = end.as_deref().map(|e| self.check_expr(e)).unwrap_or(Ty::Int);
                if !st.is_numeric() && st != Ty::Any {
                    self.report(
                        *span,
                        format!("range start must be numeric, got '{}'", st.name()),
                    );
                }
                if !et.is_numeric() && et != Ty::Any {
                    self.report(
                        *span,
                        format!("range end must be numeric, got '{}'", et.name()),
                    );
                }
                let _ = inclusive;
                Ty::Named("IntRange".into())
            }
            Expr::InRange {
                range,
                span,
            } => {
                let rt = self.check_expr(range);
                if !rt.name().contains("Range") && rt != Ty::Any {
                    self.report(
                        *span,
                        format!("'in' pattern requires a range, got '{}'", rt.name()),
                    );
                }
                Ty::Boolean
            }
            Expr::Elvis {
                lhs,
                rhs,
                span,
            } => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                if !lt.is_nullable() {
                    self.report_warning(*span, "elvis on non-nullable left side");
                }
                let non_null_l = lt.non_null().clone();
                if !rt.can_assign_to(&non_null_l) && non_null_l != Ty::Any {
                    self.report(
                        *span,
                        format!(
                            "elvis branches mismatch: '{}' vs '{}'",
                            non_null_l.name(),
                            rt.name()
                        ),
                    );
                }
                non_null_l
            }
            Expr::AssertNonNull { expr, span } => {
                let et = self.check_expr(expr);
                let _ = span;
                if !et.is_nullable() {
                    // '!!' on non-nullable is a warning
                }
                et.non_null().clone()
            }
            Expr::Defer { block, .. } => {
                self.check_expr(block);
                Ty::Unit
            }
            Expr::Await { expr, span } => {
                let et = self.check_expr(expr);
                if !self.is_in_suspend_fn {
                    self.report(*span, "await can only be used in suspend/async functions");
                }
                et
            }
            Expr::Select {
                branches, ..
            } => {
                // select 多路复用：检查所有分支的 pattern 表达式
                for branch in branches {
                    self.check_expr(&branch.pattern);
                    self.check_expr(branch.body.as_ref());
                }
                Ty::Any
            }
        }
    }

    // ── 子检查器 ──

    fn literal_type(&self, lit: &Literal) -> Ty {
        match lit {
            Literal::Int(_) => Ty::Int,
            Literal::Float(_) => Ty::Float,
            Literal::String(_) => Ty::String,
            Literal::Char(_) => Ty::Char,
            Literal::Bool(_) => Ty::Boolean,
            Literal::Null => Ty::Nullable(Box::new(Ty::Nothing)),
        }
    }

    fn check_ident(&mut self, name: &str, span: Span) -> Ty {
        if let Some(t) = self.lookup_var_ty(name) {
            return t;
        }
        // Fallback: if inside a class/struct/actor, look for class field
        if let Some(type_name) = &self.current_type {
            let field_name = format!("{}.{}", type_name, name);
            if let Some(t) = self.lookup_var_ty(&field_name) {
                return t;
            }
        }
        if let Some(fns) = self.symbols.lookup_function(name) {
            if let Some(sym) = fns.first() {
                if let SymbolKind::Function {
                    params,
                    return_type,
                } = &sym.kind
                {
                    return Ty::Function {
                        params: params.iter().map(|p| p.ty.clone()).collect(),
                        ret: Box::new(return_type.clone()),
                    };
                }
            }
        }
        // P-K2：类/结构体名引用（companion 访问 `MathUtil.PI` / `MathUtil.max(...)`）
        if self.symbols.lookup_type(name).is_some() {
            return Ty::Named(name.to_string());
        }
        self.report(span, format!("unresolved reference '{}'", name));
        Ty::Error
    }

    /// 运算符重载解析：操作数类（含继承链）声明了对应 operator fun → 返回其返回类型
    fn operator_overload_return(&self, op: BinOp, lt: &Ty) -> Option<Ty> {
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
        let cls = match lt {
            Ty::Named(n) => n.clone(),
            _ => return None,
        };
        let mut cur = Some(cls);
        while let Some(c) = cur {
            if let Some(set) = self.operator_methods.get(&c) {
                if set.contains(name) {
                    let full = format!("{}.{}", c, name);
                    if let Some(fns) = self.symbols.lookup_function(&full) {
                        if let Some(sym) = fns.first() {
                            if let SymbolKind::Function {
                                return_type,
                                ..
                            } = &sym.kind
                            {
                                return Some(return_type.clone());
                            }
                        }
                    }
                    return Some(Ty::Any);
                }
                cur = self.superclasses.get(&c).cloned();
            } else {
                break;
            }
        }
        None
    }

    /// 子类 → 祖先类的赋值兼容（沿 superclasses 链）
    fn is_subclass(&self, sub: &str, sup: &str) -> bool {
        let mut cur = self.superclasses.get(sub);
        while let Some(s) = cur {
            if s == sup {
                return true;
            }
            cur = self.superclasses.get(s);
        }
        false
    }

    fn check_binary(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr, span: Span) -> Ty {
        let lt = self.check_expr(lhs);
        let rt = self.check_expr(rhs);
        if lt == Ty::Error || rt == Ty::Error {
            return Ty::Error;
        }

        // P-K2：运算符重载（operator fun plus/minus/...）优先于内建数值语义
        if let Some(ret) = self.operator_overload_return(op, &lt) {
            return ret;
        }

        // 空安全检查：算术/位运算操作数不能是可空
        if op != BinOp::Eq
            && op != BinOp::Ne
            && op != BinOp::And
            && op != BinOp::Or
            && (lt.is_nullable() || rt.is_nullable())
        {
            self.report(
                span,
                format!(
                    "cannot apply operator on nullable operands ('{}' / '{}')",
                    lt.name(),
                    rt.name()
                ),
            );
            return Ty::Error;
        }

        match op {
            BinOp::Add => {
                if lt.is_string() && (rt.is_string() || rt == Ty::Any) {
                    Ty::String
                } else if lt.is_numeric() && rt.is_numeric() {
                    if lt == Ty::Double || rt == Ty::Double || lt == Ty::Float || rt == Ty::Float {
                        Ty::Float
                    } else {
                        Ty::Int
                    }
                } else if lt == Ty::String || rt == Ty::String {
                    Ty::String
                } else {
                    self.report(
                        span,
                        format!(
                            "operator '+' cannot be applied to '{}' and '{}'",
                            lt.name(),
                            rt.name()
                        ),
                    );
                    Ty::Error
                }
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                if lt.is_numeric() && rt.is_numeric() {
                    if lt == Ty::Float || rt == Ty::Float || lt == Ty::Double || rt == Ty::Double {
                        Ty::Float
                    } else if lt == Ty::Long || rt == Ty::Long {
                        Ty::Long
                    } else {
                        Ty::Int
                    }
                } else {
                    self.report(
                        span,
                        format!(
                            "operator requires numeric operands, got '{}' and '{}'",
                            lt.name(),
                            rt.name()
                        ),
                    );
                    Ty::Error
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if !lt.can_assign_to(&rt)
                    && !rt.can_assign_to(&lt)
                    && lt != Ty::Any
                    && rt != Ty::Any
                {
                    self.report_warning(
                        span,
                        format!(
                            "comparing unrelated types '{}' and '{}'",
                            lt.name(),
                            rt.name()
                        ),
                    );
                }
                Ty::Boolean
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if !(lt.is_numeric() && rt.is_numeric())
                    && !(lt == Ty::Any || rt == Ty::Any)
                    && !(lt == rt)
                {
                    self.report(
                        span,
                        format!(
                            "comparison requires comparable operands, got '{}' and '{}'",
                            lt.name(),
                            rt.name()
                        ),
                    );
                }
                Ty::Boolean
            }
            BinOp::And | BinOp::Or => {
                if !lt.is_boolean() && lt != Ty::Any {
                    self.report(
                        span,
                        format!("logical operator requires Boolean, got '{}'", lt.name()),
                    );
                }
                if !rt.is_boolean() && rt != Ty::Any {
                    self.report(
                        span,
                        format!("logical operator requires Boolean, got '{}'", rt.name()),
                    );
                }
                Ty::Boolean
            }
            BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => {
                if !(lt.is_integer() && rt.is_integer()) && lt != Ty::Any && rt != Ty::Any {
                    self.report(
                        span,
                        format!(
                            "bitwise operator requires Int operands, got '{}' and '{}'",
                            lt.name(),
                            rt.name()
                        ),
                    );
                }
                lt
            }
            BinOp::Assign => {
                // 处理在 parse 阶段已转成 Expr::Assign，这里不会到达
                rt
            }
            BinOp::To => {
                // key to value：返回 Pair 类型（简化为 Any）
                let _ = lt;
                let _ = rt;
                Ty::Named("Pair".into())
            }
        }
    }

    fn check_unary(&mut self, op: UnOp, operand: &Expr, span: Span) -> Ty {
        let ot = self.check_expr(operand);
        if ot.is_nullable() {
            self.report(
                span,
                format!("cannot apply unary operator on nullable '{}'", ot.name()),
            );
            return Ty::Error;
        }
        match op {
            UnOp::Minus => {
                if !ot.is_numeric() && ot != Ty::Any {
                    self.report(
                        span,
                        format!("unary '-' requires numeric operand, got '{}'", ot.name()),
                    );
                }
                ot
            }
            UnOp::Not => {
                if !ot.is_boolean() && ot != Ty::Any {
                    self.report(
                        span,
                        format!("unary '!' requires Boolean, got '{}'", ot.name()),
                    );
                }
                Ty::Boolean
            }
            UnOp::Increment | UnOp::Decrement => {
                if !ot.is_numeric() && ot != Ty::Any {
                    self.report(
                        span,
                        format!("'++'/'--' requires numeric, got '{}'", ot.name()),
                    );
                }
                ot
            }
            UnOp::AddrOf => Ty::Pointer(Box::new(ot)),
            UnOp::Dereference => match ot {
                Ty::Pointer(inner) => *inner,
                _ => {
                    self.report(span, format!("cannot dereference '{}'", ot.name()));
                    Ty::Error
                }
            },
            UnOp::NotNull => match ot {
                Ty::Nullable(inner) => *inner,
                _ => {
                    self.report(
                        span,
                        format!("'!!' requires nullable operand, got '{}'", ot.name()),
                    );
                    Ty::Error
                }
            },
        }
    }

    /// 从成员访问链中提取完整点分函数名（如 `aura.concurrent.spawn`）
    fn extract_dotted_name(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(name, _) => Some(name.clone()),
            Expr::MemberAccess {
                object,
                name,
                ..
            } => {
                let obj_name = Self::extract_dotted_name(object)?;
                Some(format!("{}.{}", obj_name, name))
            }
            _ => None,
        }
    }

    fn check_call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Ty {
        // 先尝试完整点分函数名解析（支持 aura.concurrent.spawn 等）
        if let Some(full_name) = Self::extract_dotted_name(callee) {
            if let Some(fns) = self.symbols.lookup_function(&full_name) {
                let cloned: Vec<Symbol> = fns.clone();
                return self.check_call_args(&cloned, args, span);
            }
            // Phase 1: prelude 函数兜底 — 仅 17 个全局内置免import
            // 命名空间函数（aura.math.sin 等）需通过 import 引入
            if crate::std::decl::is_prelude(&full_name) {
                return Ty::Any;
            }
        }

        // 方法调用：obj.method(...)
        if let Expr::MemberAccess {
            object,
            name,
            ..
        } = callee
        {
            let _obj_ty = self.check_expr(object);
            // 查找方法：简单按 "Type.name" 查找
            let mname = format!("{}.{}", _obj_ty.non_null().name(), name);
            if let Some(fns) = self.symbols.lookup_function(&mname) {
                let cloned: Vec<Symbol> = fns.clone();
                return self.check_call_args(&cloned, args, span);
            }
            // 内置方法
            return self.check_builtin_method(&_obj_ty.non_null().clone(), name, args, span);
        }

        // 普通函数调用：先检查标识符是否是函数/类型（避免 unresolved）
        if let Expr::Ident(name, _) = callee {
            if let Some(fns) = self.symbols.lookup_function(name) {
                let cloned: Vec<Symbol> = fns.clone();
                return self.check_call_args(&cloned, args, span);
            }
            // 构造函数调用：类型名(...)
            if let Some(_t) = self.symbols.lookup_type(name) {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Named(name.clone());
            }
            // Phase 1: 顶层 prelude 函数兜底（println / abs / sqrt 等）
            if crate::std::decl::is_prelude(name) {
                return Ty::Any;
            }
        }
        // 否则作为表达式检查（可能是 lambda 调用等）
        let callee_ty = self.check_expr(callee);
        // Lambda/函数类型调用：检查参数类型匹配
        if let Ty::Function {
            params,
            ret,
        } = &callee_ty
        {
            // 检查实参数量
            if args.len() > params.len() {
                self.report(
                    span,
                    format!(
                        "too many arguments: expected {}, got {}",
                        params.len(),
                        args.len()
                    ),
                );
                return Ty::Error;
            }
            // 检查每个实参类型
            for (i, arg) in args.iter().enumerate() {
                let at = self.check_expr(arg);
                if i < params.len() {
                    let pt = &params[i];
                    if self.is_type_variable(pt) {
                        // 类型变量接受任意类型
                    } else if pt != &Ty::Any && !at.can_assign_to(pt) {
                        self.report(
                            arg.span(),
                            format!(
                                "argument {} type mismatch: expected '{}', got '{}'",
                                i + 1,
                                pt.name(),
                                at.name()
                            ),
                        );
                    }
                }
            }
            return (**ret).clone();
        }
        self.report(
            span,
            format!("expression is not callable ('{}')", callee_ty.name()),
        );
        Ty::Error
    }

    /// 重载解析（P3.10）：在多个同名候选中挑选最匹配的一个。
    ///
    /// 1. 先按参数数量筛选可行候选（考虑默认参数：`required..params.len()`）。
    /// 2. 对可行候选按实参类型匹配度打分：精确匹配 +2，可赋值 +1。
    /// 3. 取最高分；最高分出现多个则报“歧义调用”。
    /// 4. 无可行候选则报“无可匹配的重载”。
    /// 判断一个类型是否是类型变量（如 `T`、`U`），而非已知具名类型。
    fn is_type_variable(&self, ty: &Ty) -> bool {
        if let Ty::Named(name) = ty {
            if name.chars().count() == 1
                && name.chars().next().map_or(false, |c| c.is_ascii_uppercase())
            {
                // 不在已知类型表中
                return !self.symbols.types.contains_key(name);
            }
        }
        false
    }

    /// 检查参数类型是否接受给定实参类型（支持类型变量和 vararg）
    fn param_accepts(&self, pt: &Ty, at: &Ty, is_vararg: bool) -> bool {
        if at == pt {
            return true;
        }
        if at.can_assign_to(pt) {
            return true;
        }
        // 类型变量（T, U 等）接受任意类型
        if self.is_type_variable(pt) {
            return true;
        }
        // vararg 参数接受额外实参
        if is_vararg {
            return true;
        }
        // Ty::Any 接受任意类型
        if *pt == Ty::Any {
            return true;
        }
        false
    }

    fn check_call_args(&mut self, overloads: &[Symbol], args: &[Expr], span: Span) -> Ty {
        if overloads.is_empty() {
            return Ty::Error;
        }
        let name = overloads[0].name.clone();

        // Phase 1: 检查从非 suspend 上下文调用 suspend 函数
        if !self.is_in_suspend_fn && self.suspend_functions.contains(&name) {
            self.report(
                span,
                format!(
                    "calling suspend function '{}' from non-suspend context",
                    name
                ),
            );
        }

        // 每个实参只检查一次（避免重复诊断与重复副作用）
        let arg_types: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();

        // 1) 按参数数量筛选可行候选（支持 vararg：实参数 >= 必选参数数）
        let viable: Vec<&Symbol> = overloads
            .iter()
            .filter(|sym| {
                if let SymbolKind::Function { params, .. } = &sym.kind {
                    let has_vararg = params.last().map_or(false, |p| p.is_vararg);
                    let required = params.iter().filter(|p| !p.has_default && !p.is_vararg).count();
                    if has_vararg {
                        args.len() >= required
                    } else {
                        args.len() >= required && args.len() <= params.len()
                    }
                } else {
                    false
                }
            })
            .collect();

        if viable.is_empty() {
            self.report(
                span,
                format!(
                    "no overload of '{}' accepts {} argument(s)",
                    name,
                    args.len()
                ),
            );
            return Ty::Error;
        }

        // 2) 在可行候选中按类型匹配度打分
        let mut best_score: i32 = -1;
        let mut best: Vec<&Symbol> = Vec::new();
        for sym in &viable {
            if let SymbolKind::Function { params, .. } = &sym.kind {
                let mut score = 0i32;
                let mut ok = true;
                // 处理命名参数：先按名匹配
                let mut matched_params = vec![false; params.len()];
                let mut named_scores: i32 = 0;
                let mut remaining_args: Vec<(usize, &Ty)> = Vec::new();

                // 第一遍：处理命名参数
                for (i, arg) in args.iter().enumerate() {
                    if let Expr::NamedArg {
                        name: arg_name,
                        ..
                    } = arg
                    {
                        if let Some(pi) = params.iter().position(|p| p.name == *arg_name) {
                            matched_params[pi] = true;
                            let at = &arg_types[i];
                            let pt = &params[pi].ty;
                            if self.param_accepts(pt, at, params[pi].is_vararg) {
                                if *at == *pt {
                                    named_scores += 2;
                                } else {
                                    named_scores += 1;
                                }
                            } else {
                                ok = false;
                            }
                        } else {
                            self.report(
                                args[i].span(),
                                format!(
                                    "unknown named argument '{}' in call to '{}'",
                                    arg_name, name
                                ),
                            );
                            ok = false;
                        }
                    }
                }
                score += named_scores;

                // 第二遍：按位置处理非命名参数
                let mut param_idx = 0;
                for (i, arg) in args.iter().enumerate() {
                    if matches!(arg, Expr::NamedArg { .. }) {
                        continue; // 已在第一遍处理
                    }
                    // 找到下一个未匹配的参数
                    while param_idx < params.len() && matched_params[param_idx] {
                        param_idx += 1;
                    }
                    if param_idx >= params.len() {
                        // vararg：允许额外参数
                        if params.last().map_or(false, |p| p.is_vararg) {
                            // 额外参数计入 vararg 类型
                            let at = &arg_types[i];
                            let vararg_param = params.last().unwrap();
                            if self.param_accepts(&vararg_param.ty, at, true) {
                                score += 1;
                            }
                        } else {
                            ok = false;
                            break;
                        }
                    } else {
                        let at = &arg_types[i];
                        let p = &params[param_idx];
                        if *at == p.ty {
                            score += 2;
                        } else if self.param_accepts(&p.ty, at, p.is_vararg) {
                            score += 1;
                        } else {
                            ok = false;
                        }
                    }
                }

                if ok {
                    if score > best_score {
                        best_score = score;
                        best.clear();
                        best.push(sym);
                    } else if score == best_score {
                        best.push(sym);
                    }
                }
            }
        }

        if best.is_empty() {
            if viable.len() == 1 {
                // 唯一候选但类型不匹配：给出逐参数诊断（如 `argument 1 type mismatch`）
                let sym = viable[0];
                if let SymbolKind::Function {
                    params,
                    return_type,
                } = &sym.kind
                {
                    for (i, at) in arg_types.iter().enumerate() {
                        let pt = params.get(i).map(|p| p.ty.clone()).unwrap_or(Ty::Any);
                        let is_vararg = params.get(i).map_or(false, |p| p.is_vararg);
                        if !self.param_accepts(&pt, at, is_vararg) && pt != Ty::Any {
                            self.report(
                                args[i].span(),
                                format!(
                                    "argument {} type mismatch: expected '{}', got '{}'",
                                    i + 1,
                                    pt.name(),
                                    at.name()
                                ),
                            );
                        }
                    }
                    return return_type.clone();
                }
                return Ty::Error;
            }
            self.report(
                span,
                format!("no overload of '{}' accepts the given argument types", name),
            );
            return Ty::Error;
        }

        // 3) 歧义检测
        if best.len() > 1 {
            self.report(
                span,
                format!(
                    "ambiguous call to '{}': {} overloads match the arguments",
                    name,
                    best.len()
                ),
            );
        }

        // 4) 校验选中签名的实参类型并返回返回类型
        let chosen = best[0];
        if let SymbolKind::Function {
            params,
            return_type,
        } = &chosen.kind
        {
            // 按名匹配命名参数，按位置匹配非命名参数
            let mut matched_params = vec![false; params.len()];
            for (i, arg) in args.iter().enumerate() {
                if let Expr::NamedArg {
                    name: arg_name,
                    ..
                } = arg
                {
                    if let Some(pi) = params.iter().position(|p| p.name == *arg_name) {
                        matched_params[pi] = true;
                        let at = &arg_types[i];
                        let pt = &params[pi].ty;
                        if !self.param_accepts(pt, at, params[pi].is_vararg) && *pt != Ty::Any {
                            self.report(
                                args[i].span(),
                                format!(
                                    "argument '{}' type mismatch: expected '{}', got '{}'",
                                    arg_name,
                                    pt.name(),
                                    at.name()
                                ),
                            );
                        }
                    }
                }
            }
            let mut param_idx = 0;
            for (i, arg) in args.iter().enumerate() {
                if matches!(arg, Expr::NamedArg { .. }) {
                    continue;
                }
                while param_idx < params.len() && matched_params[param_idx] {
                    param_idx += 1;
                }
                if param_idx < params.len() {
                    let at = &arg_types[i];
                    let p = &params[param_idx];
                    if !self.param_accepts(&p.ty, at, p.is_vararg) && p.ty != Ty::Any {
                        self.report(
                            args[i].span(),
                            format!(
                                "argument {} type mismatch: expected '{}', got '{}'",
                                i + 1,
                                p.ty.name(),
                                at.name()
                            ),
                        );
                    }
                }
            }
            return_type.clone()
        } else {
            Ty::Error
        }
    }

    fn check_builtin_method(&mut self, obj_ty: &Ty, name: &str, args: &[Expr], span: Span) -> Ty {
        // 类型变量（T, U 等）：当作 Any 处理
        if self.is_type_variable(obj_ty) {
            return match name {
                "toString" => Ty::String,
                "hashCode" => Ty::Int,
                "equals" => Ty::Boolean,
                _ => {
                    // 通用方法：返回 Any
                    Ty::Any
                }
            };
        }
        // 内置常用方法（简化）
        match (obj_ty, name) {
            (Ty::String, "length") => {
                if !args.is_empty() {
                    self.report(span, "String.length takes no arguments");
                }
                Ty::Int
            }
            (Ty::String, "toInt") => Ty::Int,
            (Ty::String, "toFloat") => Ty::Float,
            (Ty::String, "toLong") => Ty::Long,
            (Ty::String, "toDouble") => Ty::Double,
            (Ty::String, "toCStr") => Ty::Pointer(Box::new(Ty::Char)),
            (Ty::Int, "toFloat") => Ty::Float,
            (Ty::Int, "toDouble") => Ty::Double,
            (Ty::Int, "toLong") => Ty::Long,
            (Ty::Int, "toString") => Ty::String,
            (Ty::Long, "toString") => Ty::String,
            (Ty::Short, "toString") => Ty::String,
            (Ty::Byte, "toString") => Ty::String,
            (Ty::Float, "toInt") => Ty::Int,
            (Ty::Float, "toString") => Ty::String,
            (Ty::Double, "toString") => Ty::String,
            (Ty::Boolean, "toString") => Ty::String,
            (Ty::Char, "toString") => Ty::String,
            (Ty::String, "toString") => Ty::String,
            (Ty::Any, "toString") => Ty::String,
            (Ty::List(_), "toString") => Ty::String,
            (Ty::List(_), "add") => {
                for (i, a) in args.iter().enumerate() {
                    let _ = self.check_expr(a);
                    if i >= 1 {
                        self.report(span, "List.add takes one argument");
                    }
                }
                Ty::Unit
            }
            (Ty::List(_), "size") | (Ty::String, _) => Ty::Int,
            _ => {
                self.report(
                    span,
                    format!("unresolved method '{}' on '{}'", name, obj_ty.name()),
                );
                Ty::Error
            }
        }
    }

    fn check_member(&mut self, object: &Expr, name: &str, span: Span) -> Ty {
        let obj_ty = self.check_expr(object);
        if obj_ty.is_nullable() {
            self.report(
                span,
                format!(
                    "cannot access '.' on nullable '{}' (use '?.' instead)",
                    obj_ty.name()
                ),
            );
            return Ty::Error;
        }
        let base = obj_ty.non_null();
        // P3.6：private / protected 成员不可在定义类型之外被访问
        let type_name = base.name().to_string();
        let private_access = match self.member_visibility.get(&format!("{}.{}", type_name, name)) {
            Some(vis) if *vis != Visibility::Public => {
                self.current_type.as_deref() != Some(type_name.as_str())
            }
            _ => false,
        };
        if private_access {
            self.report(
                span,
                format!(
                    "'{}' is private/protected and cannot be accessed outside '{}'",
                    name, type_name
                ),
            );
        }
        match base {
            Ty::Named(type_name) => {
                // 类型变量（T, U 等）：当作 Any 处理，允许所有方法调用
                if self.is_type_variable(&Ty::Named(type_name.clone())) {
                    return match name {
                        "toString" => Ty::String,
                        "hashCode" => Ty::Int,
                        "equals" => Ty::Boolean,
                        _ => {
                            // 通用方法：返回 Any
                            Ty::Any
                        }
                    };
                }
                // Pair<Int, Int> 等标准库类型的成员（first/second/toString）
                if type_name == "Pair" {
                    return match name {
                        // Pair 的 first/second 返回类型由泛型实参决定；
                        // 当前 Ty 系统不保留泛型参数，统一返回 Any
                        "first" | "second" => Ty::Any,
                        "toString" => Ty::String,
                        _ => {
                            self.report(span, format!("unresolved member '{}' on Pair", name));
                            Ty::Error
                        }
                    };
                }
                // struct/enum 字段
                let field = format!("{}.{}", type_name, name);
                if let Some(t) = self.lookup_var_ty(&field) {
                    return t;
                }
                // 枚举变体
                if let Some(variants) = self.enum_variants.get(type_name) {
                    if variants.iter().any(|v| v.as_str() == name) {
                        // 枚举变体访问：返回枚举类型本身
                        return Ty::Named(type_name.clone());
                    }
                }
                if self.symbols.lookup_type(type_name).is_some() {
                    self.report(
                        span,
                        format!("unresolved member '{}' on type '{}'", name, type_name),
                    );
                    return Ty::Error;
                }
                Ty::Error
            }
            Ty::String => match name {
                "length" | "size" => Ty::Int,
                "isEmpty" | "isNotEmpty" => Ty::Boolean,
                "trim" => Ty::String,
                "lowercase" | "uppercase" => Ty::String,
                _ => {
                    self.report(span, format!("unresolved member '{}' on String", name));
                    Ty::Error
                }
            },
            Ty::List(elem) => match name {
                "size" => Ty::Int,
                "isEmpty" => Ty::Boolean,
                "first" | "last" => (**elem).clone(),
                _ => {
                    self.report(span, format!("unresolved member '{}' on List", name));
                    Ty::Error
                }
            },
            Ty::Array(elem) => match name {
                "size" => Ty::Int,
                "isEmpty" => Ty::Boolean,
                "first" | "last" => (**elem).clone(),
                _ => {
                    self.report(span, format!("unresolved member '{}' on Array", name));
                    Ty::Error
                }
            },
            Ty::Map(_, v) => match name {
                "size" => Ty::Int,
                "keys" => Ty::List(Box::new(Ty::Any)),
                _ => {
                    let _ = v;
                    self.report(span, format!("unresolved member '{}' on Map", name));
                    Ty::Error
                }
            },
            Ty::Any => Ty::Any,
            _ => {
                self.report(
                    span,
                    format!("cannot access member '{}' on '{}'", name, base.name()),
                );
                Ty::Error
            }
        }
    }

    fn check_lambda(&mut self, params: &[Param], body: &Expr, _span: Span) -> Ty {
        self.symbols.enter_scope(true);
        self.var_env.push(HashMap::new());
        for p in params {
            let pt = p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any);
            self.define_var_env(&p.name, pt, true);
        }
        let body_ty = self.check_expr(body);
        self.var_env.pop();
        self.symbols.exit_scope();
        let ptypes: Vec<Ty> = params
            .iter()
            .map(|p| p.type_hint.as_deref().map(ast_type_to_ty).unwrap_or(Ty::Any))
            .collect();
        Ty::Function {
            params: ptypes,
            ret: Box::new(body_ty),
        }
    }

    fn check_if(
        &mut self,
        condition: &Expr,
        then_branch: &Expr,
        else_branch: &Option<Box<Expr>>,
        span: Span,
    ) -> Ty {
        let ct = self.check_expr(condition);
        if !ct.is_boolean() && ct != Ty::Any {
            self.report(
                span,
                format!("if condition must be Boolean, got '{}'", ct.name()),
            );
        }
        let tt = self.check_expr(then_branch);
        match else_branch {
            Some(el) => {
                let et = self.check_expr(el);
                self.merge_types(tt, et)
            }
            None => Ty::Unit,
        }
    }

    fn check_when(&mut self, subject: &Option<Box<Expr>>, arms: &[WhenArm], span: Span) -> Ty {
        let subject_ty = subject.as_deref().map(|e| self.check_expr(e)).unwrap_or(Ty::Unit);

        // 主体变量名（用于智能转换 / 穷举性检查）
        let subj_name = match subject.as_deref() {
            Some(Expr::Ident(n, _)) => Some(n.clone()),
            _ => None,
        };
        // 若主体是枚举类型，记录其变体集合
        let subject_enum = match &subject_ty {
            Ty::Named(sn) => self.enum_variants.get(sn).cloned(),
            _ => None,
        };

        let mut has_else = false;
        let mut result: Option<Ty> = None;
        let mut covered: Vec<String> = Vec::new();

        for arm in arms {
            // 收集已覆盖的枚举变体
            if let Some(variants) = &subject_enum {
                for p in &arm.patterns {
                    if let Expr::Ident(n, _) = p {
                        if n != "else" && variants.contains(n) {
                            covered.push(n.clone());
                        }
                    }
                }
            }
            if arm.patterns.iter().any(|p| matches!(p, Expr::Ident(n, _) if n == "else")) {
                has_else = true;
            }

            // 智能转换（P3.5）：形如 `is TypeName` 的模式窄化主体变量
            let mut narrowed: Option<String> = None;
            if subj_name.is_some() {
                for p in &arm.patterns {
                    if let Expr::Ident(tname, _) = p {
                        if tname != "else" {
                            let is_variant =
                                subject_enum.as_ref().map_or(false, |vs| vs.contains(tname));
                            // 仅当 tname 是已声明的类型时才视为 `is` 智能转换
                            if !is_variant && self.symbols.lookup_type(tname).is_some() {
                                narrowed = Some(tname.clone());
                                break;
                            }
                        }
                    }
                }
            }

            // 应用窄化（保存旧值以便恢复）
            let mut had: Option<String> = None;
            let mut prev_ty: Option<Ty> = None;
            if let (Some(sn), Some(nt)) = (&subj_name, &narrowed) {
                if let Some(scope) = self.var_env.last_mut() {
                    let present = scope.contains_key(sn);
                    let prev = scope.insert(sn.clone(), Ty::Named(nt.clone()));
                    if present {
                        had = Some(sn.clone());
                        prev_ty = prev;
                    }
                }
            }

            let body_ty = self.check_expr(arm.body.as_ref());

            // 恢复窄化前类型
            if let Some(sn) = had {
                if let Some(scope) = self.var_env.last_mut() {
                    scope.insert(sn, prev_ty.unwrap());
                }
            }

            result = Some(match result {
                Some(prev) => self.merge_types(prev, body_ty),
                None => body_ty,
            });
        }

        // when 穷举性检查（P3.8）：枚举类型且无 else 时，必须覆盖所有变体
        if !has_else {
            if let Some(variants) = &subject_enum {
                let missing: Vec<&String> =
                    variants.iter().filter(|v| !covered.contains(v)).collect();
                if !missing.is_empty() {
                    self.report(
                        span,
                        format!(
                            "'when' is not exhaustive: missing branch(es) for {}",
                            missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                        ),
                    );
                }
            }
        }

        // when 表达式返回所有分支类型的合并（有 else 或无 else 均返回实际类型）
        match result {
            Some(t) => t,
            None => Ty::Unit,
        }
    }

    fn check_block(&mut self, stmts: &[Stmt]) -> Ty {
        let mut last = Ty::Unit;
        for stmt in stmts {
            last = self.check_statement(stmt);
        }
        last
    }

    fn check_for(&mut self, pattern: &Expr, iterable: &Expr, body: &Expr, span: Span) -> Ty {
        let it = self.check_expr(iterable);
        let elem = match it {
            Ty::List(e) => *e,
            Ty::Array(e) => *e,
            Ty::Named(ref n) if n.contains("Range") => Ty::Int,
            Ty::String => Ty::Char,
            Ty::Any => Ty::Any,
            _ => {
                self.report(span, format!("not iterable: '{}'", it.name()));
                Ty::Error
            }
        };
        let _ = span;
        // 绑定模式变量
        if let Expr::Ident(name, _) = pattern {
            self.define_var_env(name, elem, true);
        } else {
            // 解构模式（简化：忽略）
            let _ = pattern;
        }
        self.in_loop_depth += 1;
        let bt = self.check_expr(body);
        self.in_loop_depth -= 1;
        let _ = bt;
        Ty::Unit
    }

    fn check_while(&mut self, condition: &Expr, body: &Expr, span: Span) -> Ty {
        let ct = self.check_expr(condition);
        if !ct.is_boolean() && ct != Ty::Any {
            self.report(
                span,
                format!("while condition must be Boolean, got '{}'", ct.name()),
            );
        }
        self.in_loop_depth += 1;
        self.check_expr(body);
        self.in_loop_depth -= 1;
        Ty::Unit
    }

    fn check_dowhile(&mut self, condition: &Expr, body: &Expr, span: Span) -> Ty {
        self.check_while(condition, body, span)
    }

    fn check_return(&mut self, value: &Option<Box<Expr>>, span: Span) -> Ty {
        let expected = self.current_fn_return.clone().unwrap_or(Ty::Unit);
        match value {
            Some(v) => {
                let vt = self.check_expr(v);
                if !vt.can_assign_to(&expected) && expected != Ty::Any {
                    self.report(
                        span,
                        format!(
                            "return type mismatch: expected '{}', got '{}'",
                            expected.name(),
                            vt.name()
                        ),
                    );
                }
            }
            None => {
                if expected != Ty::Unit && expected != Ty::Any {
                    self.report(
                        span,
                        format!("return requires a value of type '{}'", expected.name()),
                    );
                }
            }
        }
        Ty::Nothing
    }

    fn check_try(
        &mut self,
        block: &Expr,
        catches: &[CatchClause],
        finally: &Option<Box<Expr>>,
        span: Span,
    ) -> Ty {
        let bt = self.check_expr(block);
        let mut result = bt;
        for c in catches {
            self.symbols.enter_scope(true);
            self.var_env.push(HashMap::new());
            self.define_var_env(&c.variable, Ty::Named(c.type_name.clone()), true);
            let ct = self.check_expr(c.body.as_ref());
            result = self.merge_types(result, ct);
            self.var_env.pop();
            self.symbols.exit_scope();
        }
        if let Some(f) = finally {
            self.check_expr(f);
        }
        let _ = span;
        result
    }

    fn check_new(&mut self, type_name: &str, args: &[Expr], span: Span) -> Ty {
        if self.symbols.lookup_type(type_name).is_none() {
            self.report(span, format!("unknown type '{}'", type_name));
        }
        for a in args {
            self.check_expr(a);
        }
        Ty::Named(type_name.into())
    }

    fn check_destructure(&mut self, patterns: &[Expr], expr: &Expr, span: Span) -> Ty {
        let et = self.check_expr(expr);
        // 简化：不检查数量
        for p in patterns {
            if let Expr::Ident(name, _) = p {
                self.define_var_env(name, Ty::Any, true);
            }
        }
        let _ = span;
        et
    }

    // ── 类型合并（if/when 分支）──

    fn merge_types(&self, a: Ty, b: Ty) -> Ty {
        if a == b {
            return a;
        }
        if a == Ty::Error {
            return b;
        }
        if b == Ty::Error {
            return a;
        }
        // 数值提升
        if a.is_numeric() && b.is_numeric() {
            if a == Ty::Double || b == Ty::Double {
                return Ty::Double;
            }
            if a == Ty::Float || b == Ty::Float {
                return Ty::Float;
            }
            if a == Ty::Long || b == Ty::Long {
                return Ty::Long;
            }
            return Ty::Int;
        }
        // 可空合并：一方可空 → 可空
        if a.is_nullable() || b.is_nullable() {
            let na = a.non_null().clone();
            let nb = b.non_null().clone();
            if na == nb {
                return Ty::Nullable(Box::new(na));
            }
        }
        // 其他：合并为 Any
        if a == Ty::Unit {
            return b;
        }
        if b == Ty::Unit {
            return a;
        }
        Ty::Any
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 语句
    // ═══════════════════════════════════════════════════════════════════════

    fn check_statement(&mut self, stmt: &Stmt) -> Ty {
        match stmt {
            Stmt::Expr(e) => self.check_expr(e),
            Stmt::Val {
                name,
                type_hint,
                initializer,
                span,
            } => self.check_val_or_var(
                name,
                type_hint.as_deref(),
                initializer.as_deref(),
                false,
                *span,
            ),
            Stmt::Var {
                name,
                type_hint,
                initializer,
                span,
            } => self.check_val_or_var(
                name,
                type_hint.as_deref(),
                initializer.as_deref(),
                true,
                *span,
            ),
            Stmt::Destructure {
                patterns,
                expr,
                type_hint: _type_hint,
                span,
            } => {
                let et = self.check_expr(expr);
                for p in patterns {
                    self.check_expr(p);
                    if let Expr::Ident(name, _) = p {
                        self.define_var_env(name, Ty::Any, true);
                    }
                }
                let _ = span;
                et
            }
            Stmt::Block(stmts, _) => self.check_block(stmts),
        }
    }

    fn check_val_or_var(
        &mut self,
        name: &str,
        type_hint: Option<&Type>,
        initializer: Option<&Expr>,
        is_mutable: bool,
        span: Span,
    ) -> Ty {
        let declared = type_hint.map(|t| self.check_type(t)).unwrap_or(Ty::Any);

        if self.symbols.is_defined_in_current_scope(name) {
            self.report(span, format!("redeclaration of '{}'", name));
        }

        let init_ty = match initializer {
            Some(init) => {
                let t = self.check_expr(init);
                if declared != Ty::Any {
                    // P-K2：子类实例可赋给祖先类型变量
                    let subclass_ok = matches!((&t, &declared), (Ty::Named(a), Ty::Named(b))
                        if a != b && self.is_subclass(a, b));
                    if !t.can_assign_to(&declared) && !subclass_ok {
                        self.report(
                            init.span(),
                            format!(
                                "type mismatch: cannot initialize '{}' with '{}'",
                                declared.name(),
                                t.name()
                            ),
                        );
                    }
                    declared.clone()
                } else {
                    t
                }
            }
            None => {
                if type_hint.is_none() {
                    self.report(
                        span,
                        format!("variable '{}' requires explicit type or initializer", name),
                    );
                    Ty::Error
                } else if !is_mutable && type_hint.is_some() && initializer.is_none() {
                    // val 必须有初始值
                    self.report(span, format!("'val' must be initialized: '{}'", name));
                    declared.clone()
                } else {
                    declared
                }
            }
        };

        self.define_local(name, init_ty.clone());
        init_ty
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 顶层语句处理（脚本模式 val/var/lateinit 注册为全局变量）
    // ═══════════════════════════════════════════════════════════════════════

    /// 第一遍：收集顶层 val/var 声明到全局符号表（允许前向引用）
    ///
    /// Bug fix: 此前顶层 `val`/`var` 被归入 `top_level_statements` 而非 `Decl`，
    /// 导致 sema 完全忽略它们。当程序同时含 `fun main()` 时，main 体内的
    /// 顶层变量引用报 "unresolved reference"。此方法把顶层 val/var 注册为
    /// 全局变量符号，使 main 体可以解析到它们。
    fn collect_top_level_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Val {
                name,
                type_hint,
                ..
            } => {
                let ty = type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                self.define_var_env(name, ty, false);
            }
            Stmt::Var {
                name,
                type_hint,
                ..
            } => {
                let ty = type_hint.as_deref().map(|t| self.check_type(t)).unwrap_or(Ty::Any);
                self.define_var_env(name, ty, true);
            }
            Stmt::Destructure {
                patterns,
                expr,
                ..
            } => {
                let et = self.check_expr(expr);
                for p in patterns {
                    if let Expr::Ident(name, _) = p {
                        self.define_var_env(name, et.clone(), true);
                    }
                }
            }
            Stmt::Block(stmts, _) => {
                for s in stmts {
                    self.collect_top_level_stmt(s);
                }
            }
            Stmt::Expr(_) => {}
        }
    }

    /// 第二遍：检查顶层 val/var 初始化器类型
    ///
    /// 注意：此方法不重新注册变量（已在 collect 阶段注册），仅检查初始化器类型。
    fn check_top_level_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Val {
                name,
                initializer,
                type_hint,
                span,
            }
            | Stmt::Var {
                name,
                initializer,
                type_hint,
                span,
            } => {
                if let Some(init) = initializer {
                    let init_ty = self.check_expr(init);
                    if let Some(ty) = type_hint {
                        let declared = self.check_type(ty);
                        if !matches!(init_ty, Ty::Any | Ty::Error) {
                            let subclass_ok = matches!((&init_ty, &declared), (Ty::Named(a), Ty::Named(b))
                                    if a != b && self.is_subclass(a, b));
                            if !init_ty.can_assign_to(&declared) && !subclass_ok {
                                self.report(
                                    init.span(),
                                    format!(
                                        "type mismatch: cannot initialize '{}' with '{}'",
                                        declared.name(),
                                        init_ty.name()
                                    ),
                                );
                            }
                        }
                        self.define_var_env(name, declared, true);
                    } else {
                        self.define_var_env(name, init_ty, true);
                    }
                }
            }
            Stmt::Destructure {
                patterns,
                expr,
                type_hint: _type_hint,
                span,
            } => {
                let et = self.check_expr(expr);
                for p in patterns {
                    self.check_expr(p);
                    if let Expr::Ident(name, _) = p {
                        self.define_var_env(name, Ty::Any, true);
                    }
                }
                let _ = span;
            }
            Stmt::Block(stmts, _) => {
                for s in stmts {
                    self.check_top_level_stmt(s);
                }
            }
            Stmt::Expr(e) => {
                self.check_expr(e);
            }
        }
    }
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

/// 便捷入口：解析源码 → 语义分析 → (程序, 语义结果)
pub fn analyze_source(source: &str) -> (Program, SemanticResult) {
    let mut lexer = crate::lexer::Lexer::new(source);
    let tokens = lexer.tokenize();
    let mut parser = crate::parser::Parser::new(tokens);
    let program = parser.parse_program();

    let mut checker = Checker::new();
    checker.analyze(&program);
    let result = checker.into_result();

    // 合并 parser 错误
    let mut errs = result.errors;
    for e in parser.errors() {
        errs.push(e.clone());
    }
    let result2 = SemanticResult {
        errors: errs,
        symbols: result.symbols,
        info: result.info,
    };
    (program, result2)
}
