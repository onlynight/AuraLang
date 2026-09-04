//! C 代码后端（备选方案）
//!
//! 对应 技术方案 §5.2 "C 后端备选方案"。
//!
//! 当 LLVM 不可用或交叉编译工具链缺失时，回退到生成 C 代码，
//! 然后调用系统 C 编译器（gcc/clang/msvc）生成可执行文件。
//!
//! C 后端比 LLVM IR 后端更简单：Aura 类型直接映射到 C 类型，
//! HIR 语句直接翻译为 C 语句。
//!
//! 限制：
//! - 不支持闭包 / 高阶函数（C 语言天然不支持）
//! - 不支持泛型（C 无泛型，需用宏或模板近似）
//! - 不支持 ARC（需用手动 malloc/free）
//! - 控制流翻译为基本 C 语法

use std::path::Path;

use crate::ast::Literal;
use crate::codegen::aot::error::AotError;
use crate::codegen::hir::{HirBinOp, HirExpr, HirFunction, HirProgram, HirStmt, HirType, HirUnOp};

/// C 类型映射
fn map_type(ty: &HirType) -> &str {
    match ty {
        HirType::Named(name) => match name.as_str() {
            "Int" => "int32_t",
            "Long" => "int64_t",
            "Short" => "int16_t",
            "Byte" | "U8" => "uint8_t",
            "Float" => "float",
            "Double" => "double",
            "Boolean" | "Bool" => "int",
            "Char" => "int16_t",
            "String" => "const char*",
            "Unit" | "Void" => "void",
            "Any" | "Nothing" => "void*",
            // P8.5 / P8.6: FFI 类型
            "CString" | "CStr" | "Handle" => "void*",
            "Color" => "unsigned int",
            _ => "void*",
        },
        HirType::Nullable(_) => "void*",
        HirType::Pointer(_inner) => "void*", // 简化：所有指针 → void*
        HirType::Unknown => "void*",
    }
}

/// 生成完整的 C 代码
pub fn generate_c_code(program: &HirProgram) -> Result<String, AotError> {
    let mut s = String::new();
    s.push_str("#include <stdint.h>\n");
    s.push_str("#include <stdbool.h>\n");
    s.push_str("#include <stdlib.h>\n");
    s.push_str("#include <string.h>\n");
    s.push_str("\n");

    // 结构体声明
    for st in &program.structs {
        s.push_str(&format!("typedef struct {{\n"));
        for (name, ty) in &st.fields {
            s.push_str(&format!("    {} {};\n", map_type(ty), sanitize_c(name)));
        }
        s.push_str(&format!("}} {};\n\n", sanitize_c(&st.name)));
    }

    // FFI 外部声明
    for func in &program.natives {
        let ret = func.ret.as_ref().map(|t| map_type(t)).unwrap_or("void");
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| {
                let default_ty = HirType::Named("Int".into());
                let ty = map_type(p.ty.as_ref().unwrap_or(&default_ty));
                format!("{} {}", ty, sanitize_c(&p.name))
            })
            .collect();
        let params_str = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        s.push_str(&format!("extern {} {}({});\n", ret, func.name, params_str));
    }
    s.push_str("\n");

    // 用户函数
    for func in &program.functions {
        if func.is_native {
            continue;
        }
        let func_c = generate_function(func);
        s.push_str(&func_c);
        s.push('\n');
    }

    // 若没有 main，合成
    if !program.functions.iter().any(|f| f.name == "main") {
        s.push_str("int main(void) {\n    return 0;\n}\n");
    }

    Ok(s)
}

fn generate_function(func: &HirFunction) -> String {
    let ret = func.ret.as_ref().map(|t| map_type(t)).unwrap_or("int");

    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| {
            let default_ty = HirType::Named("Int".into());
            let ty = map_type(p.ty.as_ref().unwrap_or(&default_ty));
            format!("{} {}", ty, sanitize_c(&p.name))
        })
        .collect();
    let params_str = if params.is_empty() {
        "void".to_string()
    } else {
        params.join(", ")
    };

    let mut s = String::new();
    s.push_str(&format!("{} {}({}) {{\n", ret, func.name, params_str));
    emit_c_block(&mut s, &func.body, 1);
    s.push_str("}\n");
    s
}

fn emit_c_block(s: &mut String, block: &crate::codegen::hir::HirBlock, indent: usize) {
    for stmt in &block.stmts {
        emit_c_stmt(s, stmt, indent);
    }
}

fn emit_c_stmt(s: &mut String, stmt: &HirStmt, indent: usize) {
    let pad = "    ".repeat(indent);
    match stmt {
        HirStmt::Val { name, ty, init } => {
            let c_ty = ty.as_ref().map(|t| map_type(t)).unwrap_or("int32_t");
            s.push_str(&format!("{}{} {}", pad, c_ty, sanitize_c(name)));
            if let Some(init) = init {
                s.push_str(" = ");
                emit_c_expr(s, init);
            }
            s.push_str(";\n");
        }
        HirStmt::Var { name, ty, init } => {
            let c_ty = ty.as_ref().map(|t| map_type(t)).unwrap_or("int32_t");
            s.push_str(&format!("{}{} {}", pad, c_ty, sanitize_c(name)));
            if let Some(init) = init {
                s.push_str(" = ");
                emit_c_expr(s, init);
            }
            s.push_str(";\n");
        }
        HirStmt::Assign { target, value } => {
            emit_c_expr(s, target);
            s.push_str(" = ");
            emit_c_expr(s, value);
            s.push_str(";\n");
        }
        HirStmt::Expr(e) => {
            emit_c_expr(s, e);
            s.push_str(";\n");
        }
        HirStmt::Return(val) => {
            s.push_str(&format!("{}return", pad));
            if let Some(v) = val {
                s.push(' ');
                emit_c_expr(s, v);
            }
            s.push_str(";\n");
        }
        HirStmt::If {
            cond,
            then_b,
            else_b,
        } => {
            s.push_str(&format!("{}if (", pad));
            emit_c_expr(s, cond);
            s.push_str(") {\n");
            emit_c_block(s, then_b, indent + 1);
            s.push_str(&format!("{}}}\n", pad));
            if let Some(else_block) = else_b {
                s.push_str(&format!("{}else {{\n", pad));
                emit_c_block(s, else_block, indent + 1);
                s.push_str(&format!("{}}}\n", pad));
            }
        }
        HirStmt::While { cond, body } => {
            s.push_str(&format!("{}while (", pad));
            emit_c_expr(s, cond);
            s.push_str(") {\n");
            emit_c_block(s, body, indent + 1);
            s.push_str(&format!("{}}}\n", pad));
        }
        HirStmt::Break => {
            s.push_str(&format!("{}break;\n", pad));
        }
        HirStmt::Continue => {
            s.push_str(&format!("{}continue;\n", pad));
        }
        HirStmt::Block(b) => {
            s.push_str(&format!("{}{{\n", pad));
            emit_c_block(s, b, indent + 1);
            s.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn emit_c_expr(s: &mut String, expr: &HirExpr) {
    match expr {
        HirExpr::Lit(lit) => emit_c_literal(s, lit),
        HirExpr::Var(name) => s.push_str(&sanitize_c(name)),
        HirExpr::Binary { op, lhs, rhs } => {
            s.push('(');
            emit_c_expr(s, lhs);
            s.push_str(" ");
            emit_c_binop(s, op);
            s.push(' ');
            emit_c_expr(s, rhs);
            s.push(')');
        }
        HirExpr::Unary { op, operand } => {
            emit_c_unop(s, op);
            emit_c_expr(s, operand);
        }
        HirExpr::Call { callee, args } => {
            s.push_str(&sanitize_c(callee));
            s.push('(');
            let args_str: Vec<String> = args
                .iter()
                .map(|a| {
                    let mut buf = String::new();
                    emit_c_expr(&mut buf, a);
                    buf
                })
                .collect();
            s.push_str(&args_str.join(", "));
            s.push(')');
        }
        HirExpr::Member { object, name } => {
            emit_c_expr(s, object);
            s.push('.');
            s.push_str(&sanitize_c(name));
        }
        HirExpr::Index { container, index } => {
            emit_c_expr(s, container);
            s.push('[');
            emit_c_expr(s, index);
            s.push(']');
        }
        HirExpr::New { type_name, args } => {
            s.push_str(&format!(
                "({{({}*)malloc(sizeof({}))}})",
                sanitize_c(type_name),
                sanitize_c(type_name)
            ));
            if !args.is_empty() {
                let args_str: Vec<String> = args
                    .iter()
                    .map(|a| {
                        let mut buf = String::new();
                        emit_c_expr(&mut buf, a);
                        buf
                    })
                    .collect();
                s.push_str(&format!(", {{{}}}", args_str.join(", ")));
            }
        }
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            s.push('(');
            emit_c_expr(s, cond);
            s.push_str(" ? ");
            emit_c_expr(s, then_e);
            s.push_str(" : ");
            emit_c_expr(s, else_e);
            s.push(')');
        }
        HirExpr::Block(block) => {
            s.push('{');
            for (i, stmt) in block.stmts.iter().enumerate() {
                if i + 1 == block.stmts.len() {
                    match stmt {
                        HirStmt::Expr(e) => emit_c_expr(s, e),
                        HirStmt::Return(Some(e)) => {
                            s.push_str("return ");
                            emit_c_expr(s, e);
                        }
                        _ => emit_c_stmt(s, stmt, 0),
                    }
                } else {
                    emit_c_stmt(s, stmt, 0);
                }
            }
            s.push('}');
        }
    }
}

fn emit_c_literal(s: &mut String, lit: &Literal) {
    match lit {
        Literal::Int(v) => s.push_str(&format!("{}", v)),
        Literal::Float(v) => s.push_str(&format!("{}f", v)),
        Literal::String(v) => s.push_str(&format!(
            "\"{}\"",
            v.replace('\\', "\\\\").replace('"', "\\\"")
        )),
        Literal::Bool(v) => s.push_str(if *v { "1" } else { "0" }),
        Literal::Null => s.push_str("NULL"),
        Literal::Char(c) => s.push_str(&format!("'{}'", c)),
    }
}

fn emit_c_binop(s: &mut String, op: &HirBinOp) {
    match op {
        HirBinOp::Add => s.push('+'),
        HirBinOp::Sub => s.push('-'),
        HirBinOp::Mul => s.push('*'),
        HirBinOp::Div => s.push('/'),
        HirBinOp::Rem => s.push('%'),
        HirBinOp::Eq => s.push_str("=="),
        HirBinOp::Ne => s.push_str("!="),
        HirBinOp::Lt => s.push('<'),
        HirBinOp::Gt => s.push('>'),
        HirBinOp::Le => s.push_str("<="),
        HirBinOp::Ge => s.push_str(">="),
        HirBinOp::And => s.push_str("&&"),
        HirBinOp::Or => s.push_str("||"),
        HirBinOp::BitAnd => s.push('&'),
        HirBinOp::BitOr => s.push('|'),
        HirBinOp::BitXor => s.push('^'),
        HirBinOp::Shl => s.push_str("<<"),
        HirBinOp::Shr => s.push_str(">>"),
        _ => s.push('+'),
    }
}

fn emit_c_unop(s: &mut String, op: &HirUnOp) {
    match op {
        HirUnOp::Minus => s.push('-'),
        HirUnOp::Not => s.push('!'),
    }
}

fn sanitize_c(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
        let _ = i;
    }
    out
}

/// 编译 C 代码为可执行文件（调用 gcc/clang/cl）
pub fn compile_c_to_exe(c_source: &str, output_path: &Path) -> Result<(), AotError> {
    use std::process::Command;

    let tmp_c = output_path.with_extension("c");
    std::fs::write(&tmp_c, c_source).map_err(|e| AotError::Io(e.to_string()))?;

    let compiler = if cfg!(target_os = "windows") {
        "cl.exe"
    } else {
        "gcc"
    };

    let args: Vec<String> = if cfg!(target_os = "windows") {
        vec![
            tmp_c.to_string_lossy().to_string(),
            format!("/Fe:{}", output_path.to_string_lossy()),
            "/std:c11".to_string(),
            "/O2".to_string(),
        ]
    } else {
        vec![
            tmp_c.to_string_lossy().to_string(),
            "-o".to_string(),
            output_path.to_string_lossy().to_string(),
            "-std=c11".to_string(),
            "-O2".to_string(),
            "-lm".to_string(),
        ]
    };

    let output = Command::new(compiler)
        .args(&args)
        .output()
        .map_err(|e| AotError::ToolError(format!("无法启动 {}: {}", compiler, e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(AotError::LinkerFailed(format!(
            "{} 失败: {}",
            compiler, stderr
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_simple_function() {
        let src = "fun main(): Int { return 42 }";
        let program = parse_source(src);
        let hir = crate::codegen::hir::desugar_program(&program);
        let c = generate_c_code(&hir).unwrap();
        assert!(c.contains("main"));
        assert!(c.contains("return"));
        println!("Generated C:\n{}", c);
    }

    #[test]
    fn test_generate_addition() {
        let src = "fun add(a: Int, b: Int): Int { return a + b }";
        let program = parse_source(src);
        let hir = crate::codegen::hir::desugar_program(&program);
        let c = generate_c_code(&hir).unwrap();
        assert!(c.contains("add"));
        assert!(c.contains("+"));
        println!("Generated C:\n{}", c);
    }

    #[test]
    fn test_generate_if() {
        let src = r#"
            fun max(a: Int, b: Int): Int {
                if (a > b) { return a }
                return b
            }
            fun main(): Int { return max(1, 2) }
        "#;
        let program = parse_source(src);
        let hir = crate::codegen::hir::desugar_program(&program);
        let c = generate_c_code(&hir).unwrap();
        assert!(c.contains("if"));
        assert!(c.contains(">"));
        assert!(c.contains("return a"));
        assert!(c.contains("return b"));
        println!("Generated C:\n{}", c);
    }

    fn parse_source(src: &str) -> crate::ast::Program {
        use crate::lexer::Lexer;
        use crate::parser::Parser;
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse_program()
    }
}
