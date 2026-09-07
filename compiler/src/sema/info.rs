//! sema → codegen 信息通道
//!
//! Checker 在类型检查时顺带记录：
//! - 每个表达式的类型名（span 键控）——供降级期做接收者类型分派
//!   （方法调用 `obj.m()` / 运算符重载 `a + b` / 访问器 `obj.prop`）；
//! - 类/结构体的成员摘要（方法名、字段、父类、companion、访问器、运算符方法）。

use crate::ast::Expr;
use std::collections::HashMap;

/// 类/结构体成员摘要（AST 派生部分由 codegen 自行收集，这里只放 sema 才知道的信息）
#[derive(Debug, Clone, Default)]
pub struct SemaInfo {
    /// 表达式 (span.start, span.end) → 类型名（如 "Counter"、"Int"、"Counter?"）
    pub expr_types: HashMap<(usize, usize), String>,
}

impl SemaInfo {
    /// 查询表达式的静态类型名（剥掉可空标记 `?`）
    pub fn expr_type(&self, e: &Expr) -> Option<String> {
        let sp = e.span();
        self.expr_types.get(&(sp.start, sp.end)).map(|t| t.trim_end_matches('?').to_string())
    }
}
