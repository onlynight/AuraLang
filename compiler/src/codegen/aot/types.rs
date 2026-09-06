//! 类型映射（Aura Type → LLVM IR 类型字符串）
//!
//! 对应 技术方案 §9.2.2 TypeMapper。
//!
//! 因为使用文本 LLVM IR（而非 inkwell 绑定），所以映射结果是一个 `String`，
//! 直接拼入 LLVM IR 文本中。这避免了编译期对 LLVM C API 的依赖。
//!
//! 设计原则：
//! - 基本类型（Int/Float/Boolean/…）映射为对应的 LLVM 标量类型
//! - 结构体映射为 LLVM 结构体 `{ T1, T2, ... }`
//! - 指针映射为 `ptr`（LLVM 13+ 不透明指针）
//! - 函数类型映射为函数指针 `ptr`（具体类型通过 LLVM function type 描述）
//! - String 可配置为 `{ i8*, i64 }` 结构（长度感知）或裸 `i8*`

use crate::codegen::hir::HirType;

/// 类型映射器
#[derive(Debug, Clone)]
pub struct TypeMapper {
    /// 是否将 `String` 表示为 `{ i8*, i64 }` 结构（长度感知字符串）
    pub string_as_struct: bool,
}

impl TypeMapper {
    /// 创建新的类型映射器
    pub fn new(string_as_struct: bool) -> Self {
        Self {
            string_as_struct,
        }
    }

    /// 将 HIR 类型映射为 LLVM IR 类型字符串
    pub fn map(&self, ty: &HirType) -> String {
        match ty {
            HirType::Named(name) => self.map_named(name),
            HirType::Nullable(inner) => self.map_nullable(inner),
            HirType::Pointer(_inner) => "ptr".to_string(), // LLVM 13+ 不透明指针
            // Fix 3: 函数类型 → 函数指针（不透明指针）
            HirType::Function { .. } => "ptr".to_string(),
            HirType::Unknown => "i8*".to_string(),
        }
    }

    /// 可空类型映射为 `i8*`（通过指针的 null 语义表达可空性）
    fn map_nullable(&self, inner: &HirType) -> String {
        let inner_ty = self.map(inner);
        // 如果是标量，包装为 `{ <inner>, i1 }` 结构（标量 + 是否为 null 标记）
        // 简化：直接返回 i8*，运行时通过指针 null 判断
        if is_scalar(&inner_ty) {
            // 标量的可空包装为 `{ <inner_type>, i1 }`
            format!("{{ {}, i1 }}", inner_ty)
        } else {
            // 指针/结构体的可空，直接保留内部类型（指针天然可空）
            inner_ty
        }
    }

    fn map_named(&self, name: &str) -> String {
        match name {
            "Int" => "i32".to_string(),
            "Long" => "i64".to_string(),
            "Short" => "i16".to_string(),
            "Byte" | "U8" => "i8".to_string(),
            "Float" => "float".to_string(),
            "Double" => "double".to_string(),
            "Boolean" | "Bool" => "i1".to_string(),
            "Char" => "i16".to_string(),
            "Unit" | "Void" => "".to_string(), // void 返回
            "String" => {
                if self.string_as_struct {
                    "{ i8*, i64 }".to_string()
                } else {
                    "i8*".to_string()
                }
            }
            "Any" => "i8*".to_string(),
            "Nothing" => "i8*".to_string(),
            // P8.5 / P8.6: FFI 类型 → 不透明指针
            "CString" | "CStr" | "Handle" => "i8*".to_string(),
            "Color" => "{ i8, i8, i8, i8 }".to_string(),
            // 运行时类型 → 不透明指针（堆对象）
            "List" | "Map" | "Set" | "Value" | "Iterator" | "Closure" => "i8*".to_string(),
            // 函数类型 → 函数指针
            _ if name.starts_with('(') => "ptr".to_string(),
            _ => {
                // 用户自定义结构体/命名类型：在 emit 阶段会被替换为对应的 struct 类型名
                // 这里返回 `%struct.<Name>` 占位符
                format!("%struct.{}", sanitizellvm(name))
            }
        }
    }

    /// 将 LLVM 类型名（字符串）转为函数类型描述字符串（用于函数指针声明）
    pub fn fn_type(&self, ret: &HirType, params: &[HirType], is_vararg: bool) -> String {
        let ret_str = self.map(ret);
        let ret_str = if ret_str.is_empty() { "void" } else { &ret_str };
        let params_str: Vec<String> = params.iter().map(|p| self.map(p)).collect();
        let params_str =
            if params_str.is_empty() { "void".to_string() } else { params_str.join(", ") };
        if is_vararg {
            format!("{ret_str}( {params_str }, ... )")
        } else {
            format!("{ret_str}( {params_str } )")
        }
    }
}

/// 判断一个 LLVM 类型字符串是否为标量类型
fn is_scalar(ty: &str) -> bool {
    matches!(
        ty,
        "i1" | "i8" | "i16" | "i32" | "i64" | "i128" | "float" | "double"
    )
}

/// 将标识符中 LLVM IR 不允许的字符替换为下划线
pub fn sanitizellvm(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_int() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Int".into())), "i32");
    }

    #[test]
    fn test_map_long() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Long".into())), "i64");
    }

    #[test]
    fn test_map_float() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Float".into())), "float");
    }

    #[test]
    fn test_map_double() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Double".into())), "double");
    }

    #[test]
    fn test_map_bool() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Boolean".into())), "i1");
    }

    #[test]
    fn test_map_string_struct() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("String".into())), "{ i8*, i64 }");
    }

    #[test]
    fn test_map_string_ptr() {
        let tm = TypeMapper::new(false);
        assert_eq!(tm.map(&HirType::Named("String".into())), "i8*");
    }

    #[test]
    fn test_map_struct_named() {
        let tm = TypeMapper::new(true);
        assert_eq!(tm.map(&HirType::Named("Player".into())), "%struct.Player");
    }

    #[test]
    fn test_map_nullable_int() {
        let tm = TypeMapper::new(true);
        let ty = HirType::Nullable(Box::new(HirType::Named("Int".into())));
        // 标量可空 → { i32, i1 }
        assert_eq!(tm.map(&ty), "{ i32, i1 }");
    }

    #[test]
    fn test_sanitize_llvm() {
        assert_eq!(sanitizellvm("my-type"), "my_type");
        assert_eq!(sanitizellvm("hello.world"), "hello_world");
        assert_eq!(sanitizellvm("foo-bar"), "foo_bar");
    }

    #[test]
    fn test_fn_type() {
        let tm = TypeMapper::new(true);
        let params = vec![
            HirType::Named("Int".into()),
            HirType::Named("Int".into()),
        ];
        let ret = HirType::Named("Int".into());
        assert_eq!(tm.fn_type(&ret, &params, false), "i32( i32, i32 )");
    }

    #[test]
    fn test_fn_type_void() {
        let tm = TypeMapper::new(true);
        let params: Vec<HirType> = vec![];
        let ret = HirType::Named("Unit".into());
        assert_eq!(tm.fn_type(&ret, &params, false), "void( void )");
    }
}
