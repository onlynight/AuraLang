//! Phase D: std 函数签名表（单一真相源）
//!
//! 与 `aura/compiler/.../aot/StdSigs.aura` 同源，用于 sema 在检查
//! 方法调用时推断返回类型，避免 `String.split` 等退化为 `Ty::Any`。
//!
//! 格式：(类名, 方法名) → (返回类型, 参数类型列表)

use std::collections::HashMap;

/// 签名表条目
pub struct Signature {
    /// 返回的 LLVM 类型
    pub ret: &'static str,
    /// 参数的 LLVM 类型列表
    pub params: &'static [&'static str],
}

/// 全局签名表
pub fn std_signature_table() -> &'static HashMap<(String, String), Signature> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<HashMap<(String, String), Signature>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut m = HashMap::new();
        // ── String ──
        m.insert(
            ("String".into(), "length".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("String".into(), "contains".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "startsWith".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "endsWith".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "substring".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64", "i64",
                ],
            },
        );
        m.insert(
            ("String".into(), "charAt".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("String".into(), "charCodeAt".into()),
            Signature {
                ret: "i64",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("String".into(), "trim".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("String".into(), "toUpperCase".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("String".into(), "toLowerCase".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("String".into(), "replace".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "replaceAll".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "indexOf".into()),
            Signature {
                ret: "i64",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "lastIndexOf".into()),
            Signature {
                ret: "i64",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "countChar".into()),
            Signature {
                ret: "i64",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "substringBefore".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "substringAfter".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "padStart".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "split".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "equals".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("String".into(), "toInt".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("String".into(), "toFloat".into()),
            Signature {
                ret: "f64",
                params: &["i8*"],
            },
        );
        // ── List ──
        m.insert(
            ("List".into(), "get".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("List".into(), "getAt".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("List".into(), "add".into()),
            Signature {
                ret: "void",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("List".into(), "size".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "isEmpty".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "first".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "last".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "map".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "filter".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "reverse".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("List".into(), "take".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("List".into(), "remove".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i64",
                ],
            },
        );
        m.insert(
            ("List".into(), "contains".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("List".into(), "indexOf".into()),
            Signature {
                ret: "i64",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("List".into(), "sorted".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        // ── Map ──
        m.insert(
            ("Map".into(), "get".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("Map".into(), "set".into()),
            Signature {
                ret: "void",
                params: &[
                    "i8*", "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("Map".into(), "remove".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("Map".into(), "size".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Map".into(), "isEmpty".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Map".into(), "containsKey".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("Map".into(), "containsValue".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m.insert(
            ("Map".into(), "keys".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Map".into(), "values".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Map".into(), "merge".into()),
            Signature {
                ret: "i8*",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        // ── Math ──
        m.insert(
            ("Math".into(), "sin".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "cos".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "tan".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "abs".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "sqrt".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "pow".into()),
            Signature {
                ret: "f64",
                params: &[
                    "f64", "f64",
                ],
            },
        );
        m.insert(
            ("Math".into(), "log".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "exp".into()),
            Signature {
                ret: "f64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "ceil".into()),
            Signature {
                ret: "i64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "floor".into()),
            Signature {
                ret: "i64",
                params: &["f64"],
            },
        );
        m.insert(
            ("Math".into(), "min".into()),
            Signature {
                ret: "f64",
                params: &[
                    "f64", "f64",
                ],
            },
        );
        m.insert(
            ("Math".into(), "max".into()),
            Signature {
                ret: "f64",
                params: &[
                    "f64", "f64",
                ],
            },
        );
        // ── Any (toString / hashCode / equals) ──
        m.insert(
            ("Any".into(), "toString".into()),
            Signature {
                ret: "i8*",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Any".into(), "hashCode".into()),
            Signature {
                ret: "i64",
                params: &["i8*"],
            },
        );
        m.insert(
            ("Any".into(), "equals".into()),
            Signature {
                ret: "i32",
                params: &[
                    "i8*", "i8*",
                ],
            },
        );
        m
    })
}

/// LLVM 类型到 Aura Ty 的映射
pub fn llvm_type_to_ty(llvm: &str) -> Option<crate::sema::ty::Ty> {
    use crate::sema::ty::Ty;
    match llvm {
        "i64" => Some(Ty::Int),
        "i32" => Some(Ty::Boolean), // 布尔值在 C 中用 i32 表示
        "f64" => Some(Ty::Double),
        "i8*" => Some(Ty::String),
        "void" => Some(Ty::Unit),
        "i1" => Some(Ty::Boolean),
        _ => None,
    }
}
