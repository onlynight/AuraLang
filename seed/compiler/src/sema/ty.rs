//! 语义类型系统（对应技术方案 §4）
//!
//! `Ty` 是语义分析阶段的类型表示，避免与语法类型 `crate::ast::Type` 混淆。
//! 支持：
//! - 基本类型（Int/Long/Float/String/Boolean/…）
//! - 可空类型（Nullable）
//! - 函数类型（用于参数/返回类型、lambda）
//! - 命名类型（struct/enum/class/interface 实例）
//! - 泛型占位（TypeVar）
//! - 集合（List/Map/Set）
//! - Any/Nothing/Unit

use std::fmt;

/// 语义类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    // 基本类型（与 Kotlin 对齐）
    Int,
    Long,
    Short,
    Byte,
    Float,
    Double,
    Boolean,
    Char,
    String,
    /// 顶级类型 Any
    Any,
    /// 无值类型 Nothing（不可为空值）
    Nothing,

    /// 无返回值的函数类型
    Unit,

    /// 可空类型：`T?`
    Nullable(Box<Ty>),

    /// 指针类型 Pointer<T>（FFI）
    Pointer(Box<Ty>),

    /// 数组/集合类型（含定长大小信息）
    Array {
        inner: Box<Ty>,
        size: Option<usize>,
    },
    /// 列表类型 List<T>
    List(Box<Ty>),
    /// 映射类型 Map<K, V>
    Map(Box<Ty>, Box<Ty>),

    /// 命名类型（struct/enum/class/interface 实例或类型别名）
    Named(String),

    /// 函数类型：参数 -> 返回
    Function {
        params: Vec<Ty>,
        ret: Box<Ty>,
    },

    /// 泛型类型变量（推断中临时使用）
    TypeVar(u32),

    /// 模块命名空间（`aura` / `aura.string` / `Math` 等）。
    ///
    /// 仅用于**成员访问链**的类型传播：`aura.string.length(s)` 里
    /// `check_ident("aura")` 需要返回一个「这是模块」的记号，才能让
    /// `check_member("aura", "string")` 继续往下走并最终解析出 `length`
    /// 的返回类型 `Int`。没有这个变体时，`aura` 会被报 `unresolved reference`
    /// 并退化成 `Ty::Error` ⇒ 整条链的类型丢失 ⇒ HIR 把结果当 `Any` 处理。
    Module(String),

    /// 未知错误类型（无法推断）
    Error,
}

impl Ty {
    pub fn unit() -> Self {
        Ty::Unit
    }

    pub fn string() -> Self {
        Ty::String
    }

    pub fn int() -> Self {
        Ty::Int
    }

    /// 从类型名解析为语义类型。
    ///
    /// 用于 `is T` 智能转换等场景：必须把 `"String"` 映射为 [`Ty::String`]，
    /// 而不是 `Ty::Named("String")`——否则后续成员访问走 `Named` 分支，
    /// 拿不到内置成员表，会把 `value.length` 误报为「unresolved member」。
    /// 未知类型名退化为 `Ty::Named`。
    pub fn from_name(name: &str) -> Self {
        match name {
            "Int" => Ty::Int,
            "Long" => Ty::Long,
            "Short" => Ty::Short,
            "Byte" => Ty::Byte,
            "Float" => Ty::Float,
            "Double" => Ty::Double,
            "Boolean" => Ty::Boolean,
            "Char" => Ty::Char,
            "String" => Ty::String,
            "Any" => Ty::Any,
            "Unit" => Ty::Unit,
            "Nothing" => Ty::Nothing,
            _ => Ty::Named(name.to_string()),
        }
    }

    /// 从语法类型（AST）解析为语义类型（不含泛型解析）
    pub fn from_ast(ty: &crate::ast::Type) -> Self {
        match ty {
            crate::ast::Type::Int => Ty::Int,
            crate::ast::Type::Long => Ty::Long,
            crate::ast::Type::Short => Ty::Short,
            crate::ast::Type::Byte => Ty::Byte,
            crate::ast::Type::Float => Ty::Float,
            crate::ast::Type::Double => Ty::Double,
            crate::ast::Type::Boolean => Ty::Boolean,
            crate::ast::Type::Char => Ty::Char,
            crate::ast::Type::String => Ty::String,
            crate::ast::Type::Any => Ty::Any,
            crate::ast::Type::Unit => Ty::Unit,
            crate::ast::Type::Nothing => Ty::Nothing,
            crate::ast::Type::Nullable(inner) => Ty::Nullable(Box::new(Ty::from_ast(inner))),
            crate::ast::Type::Pointer(inner) => Ty::Pointer(Box::new(Ty::from_ast(inner))),
            crate::ast::Type::Array { inner, size } => {
                Ty::Array {
                    inner: Box::new(Ty::from_ast(inner)),
                    size: *size,
                }
            }
            crate::ast::Type::Named { name, .. } => match name.as_str() {
                "Int" => Ty::Int,
                "Long" => Ty::Long,
                "Short" => Ty::Short,
                "Byte" => Ty::Byte,
                "Float" => Ty::Float,
                "Double" => Ty::Double,
                "Boolean" => Ty::Boolean,
                "Char" => Ty::Char,
                "String" => Ty::String,
                "Any" => Ty::Any,
                "Nothing" => Ty::Nothing,
                "Unit" => Ty::Unit,
                other => Ty::Named(other.to_string()),
            },
            crate::ast::Type::Generic {
                name, args, ..
            } => {
                let mapped: Vec<Ty> = args.iter().map(Ty::from_ast).collect();
                match name.as_str() {
                    "Pointer" => Ty::Pointer(Box::new(mapped.first().cloned().unwrap_or(Ty::Any))),
                    _ => Ty::Named(name.clone()),
                }
            }
            crate::ast::Type::Function {
                params,
                return_type,
                ..
            } => {
                let p: Vec<Ty> = params
                    .iter()
                    .map(|prm| {
                        Ty::from_ast(prm.type_hint.as_deref().unwrap_or(&crate::ast::Type::Any))
                    })
                    .collect();
                let r = return_type.as_deref().map(Ty::from_ast).unwrap_or(Ty::Unit);
                Ty::Function {
                    params: p,
                    ret: Box::new(r),
                }
            }
            // 星投影 `List<*>`：类型实参未知，按 Any 处理
            crate::ast::Type::StarProjection { .. } => Ty::Any,
        }
    }

    /// 可空性判断：此类型是否可能为 null
    pub fn is_nullable(&self) -> bool {
        matches!(self, Ty::Nullable(_)) || *self == Ty::Any
    }

    /// 去除 Nullable 包装
    pub fn non_null(&self) -> &Ty {
        match self {
            Ty::Nullable(inner) => inner.as_ref(),
            _ => self,
        }
    }

    /// 是否为数字类型
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            Ty::Int | Ty::Long | Ty::Short | Ty::Byte | Ty::Float | Ty::Double
        )
    }

    pub fn is_boolean(&self) -> bool {
        *self == Ty::Boolean
    }

    pub fn is_string(&self) -> bool {
        match self {
            Ty::String => true,
            Ty::Nullable(inner) => inner.as_ref() == &Ty::String,
            _ => false,
        }
    }

    /// 是否为整数类型
    pub fn is_integer(&self) -> bool {
        matches!(self, Ty::Int | Ty::Long | Ty::Short | Ty::Byte)
    }

    /// 是否能隐式转换为 target（简化版：相同类型、数值间、派生到基类、any）
    pub fn can_assign_to(&self, target: &Ty) -> bool {
        if self == target {
            return true;
        }
        if target == &Ty::Any || target == &Ty::Error || self == &Ty::Error {
            return true;
        }
        // Any 可作为顶层类型赋给任何类型（字面量类型推断场景）
        if *self == Ty::Any {
            return true;
        }
        if self.is_numeric() && target.is_numeric() {
            return true;
        }
        // List 类型兼容：List<T> 可赋给 List<U>（简化处理）
        if let (Ty::List(_), Ty::List(_)) = (self, target) {
            return true;
        }
        // List→Array 兼容：List<T> 可赋给 Array<T>（字面量赋给数组声明）
        if let (Ty::List(e1), Ty::Array { inner: e2, .. }) = (self, target) {
            return e1.can_assign_to(e2);
        }
        // Named("ArrayList") → Array 兼容
        if let (Ty::Named(n), Ty::Array { .. }) = (self, target) {
            if n == "ArrayList" || n == "MutableList" {
                return true;
            }
        }
        // Array→Array 兼容（多维内层赋值）
        if let (Ty::Array { inner: i1, .. }, Ty::Array { inner: i2, .. }) = (self, target) {
            return i1.can_assign_to(i2);
        }
        // Ty::Named("List") 可赋给 Ty::List(_) 和 Ty::Named("List")
        if let Ty::Named(n) = self {
            if n == "List" || n == "ArrayList" || n == "MutableList" {
                if let Ty::List(_) = target {
                    return true;
                }
                if let Ty::Named(tn) = target {
                    if tn == "List" || tn == "ArrayList" || tn == "MutableList" {
                        return true;
                    }
                }
            }
        }
        // Ty::List(_) 可赋给 Ty::Named("List")
        if let (Ty::List(_), Ty::Named(tn)) = (self, target) {
            if tn == "List" || tn == "ArrayList" || tn == "MutableList" {
                return true;
            }
        }
        // 非空值可赋给可空类型
        if let Ty::Nullable(inner) = target {
            return self.can_assign_to(inner.as_ref());
        }
        // 缩小转换：可空 -> 其内部类型
        if let Ty::Nullable(inner) = self {
            return inner.can_assign_to(target);
        }
        // Nothing 可赋给任意类型
        if *self == Ty::Nothing {
            return true;
        }
        false
    }

    /// 简单展示（用于错误信息）
    pub fn name(&self) -> String {
        match self {
            Ty::Int => "Int".into(),
            Ty::Long => "Long".into(),
            Ty::Short => "Short".into(),
            Ty::Byte => "Byte".into(),
            Ty::Float => "Float".into(),
            Ty::Double => "Double".into(),
            Ty::Boolean => "Boolean".into(),
            Ty::Char => "Char".into(),
            Ty::String => "String".into(),
            Ty::Any => "Any".into(),
            Ty::Nothing => "Nothing".into(),
            Ty::Unit => "Unit".into(),
            Ty::Nullable(inner) => format!("{}?", inner.name()),
            Ty::Pointer(inner) => format!("Pointer<{}>", inner.name()),
            Ty::Array { inner, size } => {
                if let Some(n) = size {
                    format!("{}[{}]", inner.name(), n)
                } else {
                    format!("Array<{}>", inner.name())
                }
            }
            Ty::List(inner) => format!("List<{}>", inner.name()),
            Ty::Map(k, v) => format!("Map<{}, {}>", k.name(), v.name()),
            Ty::Named(n) => n.clone(),
            Ty::Function {
                params,
                ret,
            } => {
                let ps: Vec<String> = params.iter().map(|p| p.name()).collect();
                format!("({}) -> {}", ps.join(", "), ret.name())
            }
            Ty::TypeVar(i) => format!("T{}", i),
            Ty::Module(path) => format!("<module {}>", path),
            Ty::Error => "<error>".into(),
        }
    }

    /// 泛型引用（Record 泛型实例，如 List<Int> 之外的命名泛型）
    pub fn generic_ref() -> Self {
        Ty::Named("<generic>".into())
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}
