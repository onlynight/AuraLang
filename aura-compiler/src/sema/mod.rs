//! 语义分析（P3）
//!
//! 输入 AST（`crate::ast::Program`），输出：
//! - 类型推断结果（表达式类型）
//! - 符号表（作用域链）
//! - 诊断错误（类型不匹配、未定义引用、空安全违规等）
//!
//! 设计对应 技术方案 §4 类型系统。

pub mod checker;
pub mod symbol;
pub mod ty;

pub use checker::{Checker, SemanticResult, analyze_source};
pub use symbol::{Scope, Symbol, SymbolKind, SymbolTable};
pub use ty::Ty;
