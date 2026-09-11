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

    /// 数组/集合类型
    Array(Box<Ty>),
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
            crate::ast::Type::Array(inner) => Ty::Array(Box::new(Ty::from_ast(inner))),
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
        if target == &Ty::Any {
            return true;
        }
        if self.is_numeric() && target.is_numeric() {
            return true;
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
            Ty::Array(inner) => format!("Array<{}>", inner.name()),
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
