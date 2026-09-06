//! FFI 声明生成
//!
//! 对应 技术方案 §9.3 FFI 处理。
//!
//! Aura 的 `extern "c" "libname" { ... }` 声明会生成 LLVM 外部函数声明（ExternalLinkage）。
//! 生成的 LLVM IR 中，这些函数以 `declare ... @name(...)` 形式出现，链接时由目标平台的库解析。

use crate::codegen::aot::error::AotError;
use crate::codegen::aot::types::TypeMapper;
use crate::codegen::hir::HirFunction;
use crate::codegen::opcode::FfiAbi;

/// FFI 声明生成器
pub struct FfiGenerator<'a> {
    type_mapper: &'a TypeMapper,
}

impl<'a> FfiGenerator<'a> {
    pub fn new(type_mapper: &'a TypeMapper) -> Self {
        Self {
            type_mapper,
        }
    }

    /// 生成 FFI 函数声明的 LLVM IR 文本
    pub fn generate_declarations(&self, funcs: &[HirFunction]) -> Result<Vec<String>, AotError> {
        let mut decls = Vec::new();
        for func in funcs {
            decls.push(self.generate_extern_function(func));
        }
        Ok(decls)
    }

    fn generate_extern_function(&self, func: &HirFunction) -> String {
        let ret_ty = self
            .type_mapper
            .map(func.ret.as_ref().unwrap_or(&crate::codegen::hir::HirType::Named("Unit".into())));
        let ret_str = if ret_ty.is_empty() { "void" } else { &ret_ty };
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| {
                self.type_mapper.map(
                    p.ty.as_ref().unwrap_or(&crate::codegen::hir::HirType::Named("Int".into())),
                )
            })
            .collect();
        let params_str = if params.is_empty() { String::new() } else { params.join(", ") };

        // P8-Rust: 按 ABI 标记生成注释（调用约定均为 C ABI / ccc）
        let mut comment = String::new();
        match func.ffi_abi {
            FfiAbi::Rust => {
                comment = "; P8-Rust: Rust library (C ABI)\n".to_string();
                if let Some(ref lib) = func.ffi_lib {
                    comment.push_str(&format!(";   library: {}\n", lib));
                }
            }
            FfiAbi::C => {
                if let Some(ref lib) = func.ffi_lib {
                    comment = format!("; library: {}\n", lib);
                }
            }
            FfiAbi::None => {}
        }

        format!(
            "{comment}declare {ret_str} @{name}({params_str})\n",
            comment = comment,
            ret_str = ret_str,
            name = func.name,
            params_str = params_str,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::hir::{HirParam, HirType};
    use std::sync::OnceLock;

    fn tm() -> &'static TypeMapper {
        static TM: OnceLock<TypeMapper> = OnceLock::new();
        TM.get_or_init(|| TypeMapper::new(true))
    }

    #[test]
    fn test_extern_function_decl() {
        let ffi_gen = FfiGenerator::new(tm());
        let func = HirFunction {
            name: "DrawCircle".into(),
            params: vec![
                HirParam {
                    name: "x".into(),
                    ty: Some(HirType::Named("Int".into())),
                },
                HirParam {
                    name: "y".into(),
                    ty: Some(HirType::Named("Int".into())),
                },
                HirParam {
                    name: "r".into(),
                    ty: Some(HirType::Named("Float".into())),
                },
            ],
            ret: Some(HirType::Named("Unit".into())),
            body: crate::codegen::hir::HirBlock {
                stmts: vec![],
            },
            is_native: true,
            type_params: vec![],
            ffi_abi: FfiAbi::None,
            ffi_lib: None,
        };
        let decls = ffi_gen.generate_declarations(&[func]).unwrap();
        assert_eq!(decls.len(), 1);
        assert!(decls[0].contains("DrawCircle"));
        assert!(decls[0].contains("void"));
        assert!(decls[0].contains("i32"));
        assert!(decls[0].contains("float"));
    }
}
