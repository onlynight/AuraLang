//! Runtime 库链接声明
//!
//! 对应 技术方案 §9.6 Runtime 库集成。
//!
//! Aura 的 runtime 库（`aura-runtime`）用 Rust 实现，通过 C ABI 导出：
//! - `aura_arc_increment(ptr)`：原子引用计数 +1
//! - `aura_arc_decrement(ptr)`：原子引用计数 -1，归零时释放
//! - `aura_coroutine_yield(ctx)`：协程挂起
//! - `aura_malloc(size)`：堆分配
//! - `aura_free(ptr)`：堆释放
//! - `aura_string_new(ptr, len)`：创建字符串对象
//! - `aura_string_len(s)`：获取字符串长度
//! - `aura_string_data(s)`：获取字符串数据指针
//!
//! 代码生成时自动注入这些函数的外部声明，使生成的 LLVM IR 可以引用它们。

use crate::codegen::aot::types::TypeMapper;

/// Runtime 函数声明
struct RuntimeFn {
    name: &'static str,
    ret: &'static str,
    params: &'static [(&'static str, &'static str)], // (name, llvm_type)
}

/// 所有 runtime 函数的声明
const RUNTIME_FUNCTIONS: &[RuntimeFn] = &[
    RuntimeFn {
        name: "aura_arc_increment",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_arc_decrement",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_coroutine_yield",
        ret: "void",
        params: &[("ctx", "i8*")],
    },
    RuntimeFn {
        name: "aura_malloc",
        ret: "i8*",
        params: &[("size", "i64")],
    },
    RuntimeFn {
        name: "aura_free",
        ret: "void",
        params: &[("ptr", "i8*")],
    },
    RuntimeFn {
        name: "aura_string_new",
        ret: "i8*",
        params: &[("data", "i8*"), ("len", "i64")],
    },
    RuntimeFn {
        name: "aura_string_len",
        ret: "i64",
        params: &[("s", "i8*")],
    },
    RuntimeFn {
        name: "aura_string_data",
        ret: "i8*",
        params: &[("s", "i8*")],
    },
];

/// 生成所有 runtime 函数的 LLVM 外部声明
pub fn generate_runtime_declarations(_type_mapper: &TypeMapper) -> String {
    let mut s = String::new();
    s.push_str("; ---- Aura Runtime Declarations ----\n");
    for fn_decl in RUNTIME_FUNCTIONS {
        let params_str: Vec<&str> = fn_decl.params.iter().map(|(_, ty)| *ty).collect();
        let params_str = if params_str.is_empty() {
            "void".to_string()
        } else {
            params_str.join(", ")
        };
        s.push_str(&format!(
            "declare {} @{}({})\n",
            fn_decl.ret, fn_decl.name, params_str
        ));
    }
    s
}

/// 获取所有 runtime 函数名（供内联 / 消引用使用）
pub fn runtime_function_names() -> Vec<&'static str> {
    RUNTIME_FUNCTIONS.iter().map(|f| f.name).collect()
}

/// 检查一个函数名是否为 runtime 函数
pub fn is_runtime_function(name: &str) -> bool {
    RUNTIME_FUNCTIONS.iter().any(|f| f.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_declarations() {
        let tm = TypeMapper::new(true);
        let decls = generate_runtime_declarations(&tm);
        assert!(decls.contains("aura_arc_increment"));
        assert!(decls.contains("aura_arc_decrement"));
        assert!(decls.contains("aura_coroutine_yield"));
        assert!(decls.contains("aura_malloc"));
        assert!(decls.contains("aura_free"));
        assert!(decls.contains("aura_string_new"));
    }

    #[test]
    fn test_is_runtime() {
        assert!(is_runtime_function("aura_arc_increment"));
        assert!(is_runtime_function("aura_malloc"));
        assert!(!is_runtime_function("main"));
        assert!(!is_runtime_function("add"));
    }

    #[test]
    fn test_runtime_names() {
        let names = runtime_function_names();
        assert_eq!(names.len(), RUNTIME_FUNCTIONS.len());
        assert!(names.contains(&"aura_arc_increment"));
    }
}
