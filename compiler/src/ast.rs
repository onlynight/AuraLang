//! AST 定义 — 覆盖 Aura 语言方案 §3 的全部语法结构

use crate::span::Span;
use crate::token::{Token, TokenKind};

// ─────────────────────────────────────────────────────────────────────────────
// 访问修饰符
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Internal,
    Private,
}

impl Visibility {
    pub fn from_token(t: &Token) -> Option<Self> {
        match t.kind {
            TokenKind::Public => Some(Visibility::Public),
            TokenKind::Protected => Some(Visibility::Protected),
            _ => None,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 类型
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Named {
        name: String,
        span: Span,
    },
    Int,
    Long,
    Short,
    Byte,
    Float,
    Double,
    Boolean,
    Char,
    String,
    Any,
    Unit,
    Nothing,
    Nullable(Box<Type>),
    Pointer(Box<Type>),
    Array(Box<Type>),
    Function {
        params: Vec<Param>,
        return_type: Option<Box<Type>>,
        span: Span,
    },
    Generic {
        name: String,
        args: Vec<Type>,
        span: Span,
    },
    /// 星投影：`List<*>`（Kotlin star projection，表示未知类型实参）
    StarProjection {
        span: Span,
    },
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Named { span, .. } => *span,
            Type::Nullable(t) => t.span(),
            Type::Pointer(t) => t.span(),
            Type::Array(t) => t.span(),
            Type::Function { span, .. } => *span,
            Type::Generic { span, .. } => *span,
            _ => Span::single(0, 1, 1),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,    // +
    Sub,    // -
    Mul,    // *
    Div,    // /
    Mod,    // %
    Eq,     // ==
    Ne,     // !=
    Lt,     // <
    Gt,     // >
    Le,     // <=
    Ge,     // >=
    And,    // &&
    Or,     // ||
    BitAnd, // &
    BitOr,  // |
    BitXor, // ^
    Shl,    // <<
    Shr,    // >>
    UShr,   // >>>
    Assign, // =（赋值，右结合）
    To,     // to（map entry）
    Is,     // is（类型检查，Phase 2）
    As,     // as（类型转换，Phase 2）
}

impl BinOp {
    /// 运算符优先级（Pratt 解析用，值越小优先级越低）
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Assign => 1,
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne => 3,
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 4,
            BinOp::Shl | BinOp::Shr | BinOp::UShr => 5,
            BinOp::Add | BinOp::Sub => 6,
            BinOp::Mul | BinOp::Div | BinOp::Mod => 7,
            _ => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Minus,
    Not,
    Increment,
    Decrement,
    AddrOf,
    Dereference,
    NotNull,
}

// ─────────────────────────────────────────────────────────────────────────────
// 字面量
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Char(char),
    Bool(bool),
    Null,
}

// ─────────────────────────────────────────────────────────────────────────────
// 表达式
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal, Span),
    Ident(String, Span),
    Assign {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    Unary {
        op: UnOp,
        operand: Box<Expr>,
        span: Span,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    NamedArg {
        name: String,
        value: Box<Expr>,
        span: Span,
    },
    MemberAccess {
        object: Box<Expr>,
        name: String,
        span: Span,
    },
    SafeAccess {
        object: Box<Expr>,
        name: String,
        span: Span,
    },
    Index {
        container: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
        span: Span,
    },
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        span: Span,
    },
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },
    When {
        subject: Option<Box<Expr>>,
        arms: Vec<WhenArm>,
        span: Span,
    },
    For {
        pattern: Box<Expr>,
        iterable: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    While {
        condition: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    DoWhile {
        condition: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    Return {
        value: Option<Box<Expr>>,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Throw {
        value: Box<Expr>,
        span: Span,
    },
    Try {
        block: Box<Expr>,
        catches: Vec<CatchClause>,
        finally: Option<Box<Expr>>,
        span: Span,
    },
    New {
        type_name: String,
        args: Vec<Expr>,
        span: Span,
    },
    Destructure {
        patterns: Vec<Expr>,
        expr: Box<Expr>,
        span: Span,
    },
    TypeCast {
        expr: Box<Expr>,
        type_name: Box<Type>,
        /// `as?` 安全转换：失败时返回 null 而非抛错
        safe: bool,
        span: Span,
    },
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
        span: Span,
    },
    /// when 分支中的 in range 匹配模式
    InRange {
        range: Box<Expr>,
        span: Span,
    },
    Elvis {
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    AssertNonNull {
        expr: Box<Expr>,
        span: Span,
    },
    Defer {
        block: Box<Expr>,
        span: Span,
    },
    Await {
        expr: Box<Expr>,
        span: Span,
    },
    /// async 块（P8）：`async { body }` — 引入 suspend 上下文，值为 body 的值
    AsyncBlock {
        body: Box<Expr>,
        span: Span,
    },
    /// 字符串插值（P14）：`"a $b ${c + 1}"` — parts 为字面量片段与被插值表达式交替
    StrInterp {
        parts: Vec<Expr>,
        span: Span,
    },
    /// this 引用（对象自身）
    This(Span),
    /// select 多路复用（P10.9）：多个 `Channel.receive()` 分支 + 可选 default
    Select {
        branches: Vec<SelectBranch>,
        span: Span,
    },
    Block(Vec<Stmt>, Span),
    // TODO: spread operator, string interpolation nodes, etc.
}

// ─────────────────────────────────────────────────────────────────────────────
// 语句
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Expr(Expr),
    Val { name: String, type_hint: Option<Box<Type>>, initializer: Option<Box<Expr>>, span: Span },
    Var { name: String, type_hint: Option<Box<Type>>, initializer: Option<Box<Expr>>, span: Span },
    Destructure { patterns: Vec<Expr>, expr: Box<Expr>, type_hint: Option<Box<Type>>, span: Span },
    Block(Vec<Stmt>, Span),
    // Expressions used as statements
}

// ─────────────────────────────────────────────────────────────────────────────
// 声明
#[derive(Debug, Clone, PartialEq)]
pub enum Decl {
    Function(FnDecl),
    Struct(StructDecl),
    Class(ClassDecl),
    Interface(InterfaceDecl),
    Enum(EnumDecl),
    Actor(ActorDecl),
    /// 单例对象（Kotlin `object`）：全局唯一实例，不可手动实例化
    Object(ObjectDecl),
    TypeAlias(TypeAliasDecl),
    Extern(ExternDecl),
    ExternInterface(ExternInterfaceDecl),
    Import(ImportDecl),
    Annotation(AnnotationDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub visibility: Visibility,
    pub modifiers: Vec<FnModifier>,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,
    pub return_type: Option<Box<Type>>,
    pub body: Option<Box<Expr>>,
    /// 文档注释（`///` / `/** */`），按行合并
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnModifier {
    Suspend,
    Async,
    Inline,
    Override,
    Comptime,
    Static,
    /// 可被子类重写（Kotlin：成员默认 final，需显式 open）
    Open,
    /// 抽象方法：无函数体，必须由子类实现
    Abstract,
    /// 运算符重载标记（plus/minus/eq 等）
    Operator,
    /// 中缀函数：`a between b` 形式调用
    Infix,
    /// 尾递归优化标记：编译器校验自递归并优化为循环
    Tailrec,
    /// inline 函数中禁止内联的 lambda 参数
    Noinline,
    /// inline 函数中允许在非局部上下文调用的 lambda 参数
    Crossinline,
    /// 多平台 expect 声明（声明与实现分离）
    Expect,
    /// 多平台 actual 实现
    Actual,
    /// 默认实现（extern interface 内 loadLibrary 使用）
    Default,
}

/// 类修饰符（value / data / sealed / final / open / abstract / expect / actual）。
/// `struct` 解析为 `ClassDecl { modifiers: [Value], ... }`，作为 `value class` 的别名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassModifier {
    /// 值类型：栈内联、值拷贝、无继承、映射 C struct
    Value,
    /// 数据类：自动生成 toString/equals/hashCode/copy
    Data,
    /// 受控继承
    Sealed,
    /// 不可继承
    Final,
    /// 可被继承（Kotlin：类默认 final）
    Open,
    /// 抽象类：可包含抽象方法，不能直接实例化
    Abstract,
    /// 多平台 expect 声明
    Expect,
    /// 多平台 actual 实现
    Actual,
}

/// 次构造函数 / init 构造函数（Kotlin `constructor` / `init(params)`）
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorDecl {
    pub visibility: Visibility,
    pub params: Vec<Param>,
    /// 委托调用：`: super(...)` / `: this(...)`（或 Aura 风格 `: Base(...)`，按 super 处理）
    pub delegation: Option<ConstructorDelegation>,
    pub body: Option<Box<Expr>>,
    pub span: Span,
}

/// 构造函数委托目标
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtorDelegationTarget {
    Super,
    This,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstructorDelegation {
    pub target: CtorDelegationTarget,
    pub args: Vec<Expr>,
    pub span: Span,
}

/// 伴生对象（Kotlin `companion object`）：类级静态成员容器
#[derive(Debug, Clone, PartialEq)]
pub struct CompanionDecl {
    /// `companion object Name { ... }` 的可选名称
    pub name: Option<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<FnDecl>,
    pub init_blocks: Vec<Expr>,
    pub span: Span,
}

/// 单例对象（Kotlin `object`）：全局唯一实例，不可手动实例化
///
/// 语法：`[可见性] object Name [: Parent() | Interface] { 成员 }`
/// 语义：
/// - 全局唯一实例（懒初始化，首次访问时创建）
/// - 不可手动实例化（无 `Name()` 语法）
/// - 支持继承：`object : Parent()` / `object : Interface`
/// - 成员通过 `Name.member` 访问
/// - 隐式继承 `Any` 基类
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectDecl {
    pub visibility: Visibility,
    pub name: String,
    /// 父类名（如 `object Foo : Bar()`）
    pub superclass: Option<String>,
    /// 实现的接口列表
    pub implementations: Vec<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<FnDecl>,
    /// init 块（Kotlin 风格 `init { ... }`）
    pub init_blocks: Vec<Expr>,
    /// 类修饰符集合（open / final / sealed / expect / actual）
    pub modifiers: Vec<ClassModifier>,
    pub doc: Option<String>,
    pub span: Span,
}

/// 属性访问器单条声明（getter / setter）
#[derive(Debug, Clone, PartialEq)]
pub struct AccessorDecl {
    /// setter 参数（getter 恒为 None）
    pub param: Option<Param>,
    pub body: Box<Expr>,
    pub span: Span,
}

/// 属性访问器集合（Kotlin 软关键字 `get` / `set`；访问器体内可用 `field` 引用底层字段）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldAccessors {
    pub getter: Option<AccessorDecl>,
    pub setter: Option<AccessorDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub visibility: Visibility,
    /// 是否为密封结构体（sealed）。
    pub sealed: bool,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<StructField>,
    pub methods: Vec<FnDecl>,
    pub implementations: Vec<String>, // "TraitName"
    /// init 块 / init 构造函数体（Kotlin 风格 `init { ... }`）
    pub init_blocks: Vec<Expr>,
    /// 次构造函数（`constructor(...)` / `init(...)`）
    pub constructors: Vec<ConstructorDecl>,
    /// 伴生对象（`companion object { ... }`）
    pub companion_objects: Vec<CompanionDecl>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub visibility: Visibility,
    pub is_mutable: bool,
    pub name: String,
    pub type_hint: Option<Box<Type>>,
    pub default_value: Option<Box<Expr>>,
    /// 属性访问器（Kotlin 软关键字 `get` / `set`）
    pub accessors: Option<Box<FieldAccessors>>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub visibility: Visibility,
    /// 是否为密封类（sealed）。密封类型的直接子类必须位于同一编译单元（单文件模型下恒满足，
    /// 待多文件模块支持后用于跨文件子类化检查）。
    pub sealed: bool,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub superclass: Option<String>,
    pub fields: Vec<StructField>,
    pub methods: Vec<FnDecl>,
    pub implementations: Vec<String>,
    /// init 块（Kotlin 风格 `init { ... }`，按声明顺序在构造时执行）
    pub init_blocks: Vec<Expr>,
    /// 次构造函数（`constructor(...)`）
    pub constructors: Vec<ConstructorDecl>,
    /// 伴生对象（`companion object { ... }`）
    pub companion_objects: Vec<CompanionDecl>,
    pub doc: Option<String>,
    pub span: Span,
    /// 类修饰符集合（value / data / sealed / final / open / abstract / expect / actual）。
    /// 空集合表示普通引用类型（默认 final，需 open/abstract 才可继承）。
    pub modifiers: Vec<ClassModifier>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub methods: Vec<FnDecl>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub visibility: Visibility,
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActorDecl {
    pub visibility: Visibility,
    pub name: String,
    pub fields: Vec<StructField>,
    pub methods: Vec<FnDecl>,
    pub init_blocks: Vec<Expr>,
    pub constructors: Vec<ConstructorDecl>,
    pub companion_objects: Vec<CompanionDecl>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDecl {
    pub visibility: Visibility,
    pub name: String,
    pub type_params: Vec<TypeParam>,
    pub aliased_type: Box<Type>,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternDecl {
    pub abi: String, // "c", "stdcall", etc.
    pub library: Option<String>,
    pub functions: Vec<FnDecl>,
    /// FFI 常量（P8.1）：`val NAME: Type` 声明
    pub constants: Vec<Stmt>,
    pub span: Span,
}

/// extern interface 声明：绑定到 AOT 动态库的函数接口
/// 语法：`extern interface Name = "path" { fun add(...) }`
#[derive(Debug, Clone, PartialEq)]
pub struct ExternInterfaceDecl {
    pub name: String,             // 接口名，如 "Utils"
    pub lib_path: Option<String>, // 库路径，None 时按模块名自动查找
    pub functions: Vec<FnDecl>,   // 函数声明列表
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: String,
    pub alias: Option<String>,
    pub wildcard: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationDecl {
    pub name: String,
    pub args: Vec<Expr>,
    pub span: Span,
}

// ─────────────────────────────────────────────────────────────────────────────
// 辅助结构
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_hint: Option<Box<Type>>,
    pub default_value: Option<Box<Expr>>,
    pub is_vararg: bool,
    /// inline 函数中禁止内联的 lambda 参数（Kotlin `noinline`）
    pub is_noinline: bool,
    /// inline 函数中允许跨内联上下文调用的 lambda 参数（Kotlin `crossinline`）
    pub is_crossinline: bool,
    pub span: Span,
}

/// 泛型型变（Kotlin 声明处型变：`out T` / `in T`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeVariance {
    Invariant,
    Out,
    In,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: String,
    pub variance: TypeVariance,
    pub bounds: Vec<Type>,
    pub default: Option<Box<Type>>,
    /// inline 函数中保留类型信息（Kotlin `reified`，配合 `is T` / `as T`）
    pub reified: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhenArm {
    pub patterns: Vec<Expr>,
    pub guard: Option<Box<Expr>>,
    pub body: Box<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub variable: String,
    pub type_name: String,
    pub body: Box<Expr>,
    pub span: Span,
}

/// select 分支（P10.9）：模式（如 `ch.receive()`）+ 绑定变量 + 块
#[derive(Debug, Clone, PartialEq)]
pub struct SelectBranch {
    pub pattern: Expr,
    pub bind_var: Option<String>,
    pub body: Box<Expr>,
    pub span: Span,
}

// ─────────────────────────────────────────────────────────────────────────────
// 程序根节点
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub imports: Vec<ImportDecl>,
    pub declarations: Vec<Decl>,
    /// 顶层语句（脚本模式：无 main 时，顶层语句会被自动包装为隐式 main）
    pub top_level_statements: Vec<Stmt>,
}

// ─────────────────────────────────────────────────────────────────────────────
// 格式化辅助
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Named { name, .. } => write!(f, "{}", name),
            Type::Int => write!(f, "Int"),
            Type::Long => write!(f, "Long"),
            Type::Short => write!(f, "Short"),
            Type::Byte => write!(f, "Byte"),
            Type::Float => write!(f, "Float"),
            Type::Double => write!(f, "Double"),
            Type::Boolean => write!(f, "Boolean"),
            Type::Char => write!(f, "Char"),
            Type::String => write!(f, "String"),
            Type::Any => write!(f, "Any"),
            Type::Unit => write!(f, "Unit"),
            Type::Nothing => write!(f, "Nothing"),
            Type::Nullable(t) => write!(f, "{}?", t),
            Type::Pointer(t) => write!(f, "Pointer<{}>", t),
            Type::Array(t) => write!(f, "Array<{}>", t),
            _ => write!(f, "<type>"),
        }
    }
}

impl Expr {
    /// 返回表达式的源码位置
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, s)
            | Expr::Ident(_, s)
            | Expr::AsyncBlock {
                span: s, ..
            }
            | Expr::StrInterp {
                span: s, ..
            }
            | Expr::Assign {
                span: s, ..
            }
            | Expr::Binary {
                span: s, ..
            }
            | Expr::Unary {
                span: s, ..
            }
            | Expr::Call {
                span: s, ..
            }
            | Expr::NamedArg {
                span: s, ..
            }
            | Expr::MemberAccess {
                span: s, ..
            }
            | Expr::SafeAccess {
                span: s, ..
            }
            | Expr::Index {
                span: s, ..
            }
            | Expr::Lambda {
                span: s, ..
            }
            | Expr::Closure {
                span: s, ..
            }
            | Expr::If {
                span: s, ..
            }
            | Expr::When {
                span: s, ..
            }
            | Expr::For {
                span: s, ..
            }
            | Expr::While {
                span: s, ..
            }
            | Expr::DoWhile {
                span: s, ..
            }
            | Expr::Return {
                span: s, ..
            }
            | Expr::Break { span: s }
            | Expr::Continue { span: s }
            | Expr::Throw {
                span: s, ..
            }
            | Expr::Try {
                span: s, ..
            }
            | Expr::New {
                span: s, ..
            }
            | Expr::Destructure {
                span: s, ..
            }
            | Expr::TypeCast {
                span: s, ..
            }
            | Expr::Range {
                span: s, ..
            }
            | Expr::InRange {
                span: s, ..
            }
            | Expr::Elvis {
                span: s, ..
            }
            | Expr::AssertNonNull {
                span: s, ..
            }
            | Expr::Defer {
                span: s, ..
            }
            | Expr::Await {
                span: s, ..
            }
            | Expr::This(s)
            | Expr::Select {
                span: s, ..
            }
            | Expr::Block(_, s) => *s,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Expr(e) => e.span(),
            Stmt::Val {
                span: s, ..
            }
            | Stmt::Var {
                span: s, ..
            }
            | Stmt::Destructure {
                span: s, ..
            }
            | Stmt::Block(_, s) => *s,
        }
    }
}
