//! 字节码编译器（P4 总入口）
//!
//! 完整流水线（对应 技术方案 §5.2）：
//! ```text
//! AST → HIR（去语法糖 + 泛型单态化）→ 优化(HIR: 内联/常量折叠)
//!     → MIR（基本块 + CFG）→ 优化(MIR: DCE/LICM/逃逸分析)
//!     → 字节码发射 → .auc 模块
//! ```
//!
//! 对外提供：
//! - [`compile`]：从已解析的 AST 生成 [`BytecodeModule`]
//! - [`compile_source`]：从源码字符串跑完前端 + 代码生成
//! - [`disassemble`]：反汇编（调试）
//! - [`to_bytes`] / [`from_bytes`] / [`write_auc`] / [`read_auc`]：`.auc` 序列化

pub mod disasm;
pub mod emit;
pub mod hir;
pub mod mir;
pub mod mono;
pub mod opcode;
pub mod opt;
pub mod serialize;

// P7 内存管理
pub mod arc;

// AOT（LLVM）后端（P6）
#[cfg(feature = "llvm")]
pub mod aot;

pub use disasm::disassemble;
pub use emit::{emit_module, find_const};
pub use hir::{HirProgram, desugar_program};
pub use mir::{MirFunction, lower_program};
pub use mono::mono_hir;
pub use opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const, OpCode};
pub use opt::{dce_mir, escape_mir, fold_hir, inline_hir, licm_mir};
pub use serialize::{SerializeError, from_bytes, read_auc, to_bytes, write_auc};

#[cfg(feature = "llvm")]
pub use aot::{AotCodeGenerator, AotError, AotOptions, AotOutput, OutputFormat, aot_compile};

use crate::ast::Program;

/// AOT / LLVM 后端统一错误类型（对外）
#[derive(Debug)]
pub enum CodegenError {
    Aot(String),
    Bytecode(String),
    Other(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::Aot(s) => write!(f, "AOT 错误: {s}"),
            CodegenError::Bytecode(s) => write!(f, "字节码发射失败: {s}"),
            CodegenError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for CodegenError {}

#[cfg(feature = "llvm")]
impl From<aot::AotError> for CodegenError {
    fn from(e: aot::AotError) -> Self {
        CodegenError::Aot(e.to_string())
    }
}

/// 代码生成选项
#[derive(Debug, Clone)]
pub struct CodeGenOptions {
    /// 是否运行优化通道（默认 true）
    pub optimize: bool,
}

impl Default for CodeGenOptions {
    fn default() -> Self {
        CodeGenOptions { optimize: true }
    }
}

/// 从 AST 程序编译为字节码模块
pub fn compile(program: &Program, opts: &CodeGenOptions) -> BytecodeModule {
    // 1. AST → HIR（去语法糖）
    let mut hir = desugar_program(program);

    // 2. 泛型单态化（HIR）
    mono_hir(&mut hir);

    // 3. HIR 优化：内联展开 + 常量折叠
    inline_hir(&mut hir);
    fold_hir(&mut hir);

    // 4. HIR → MIR（基本块 + CFG）
    let (mut mir_funcs, ctx) = lower_program(&hir);

    // 5. MIR 优化：死代码消除 + 循环不变量外提（+ 逃逸分析作为分析）
    // 注意：`dce_mir` 与 `licm_mir` 当前均存在 CFG 损坏缺陷（入口块假设、preheader 改写 If
    // 分支目标等），会导致控制流错误；在修复前默认关闭。仅保留 `escape_mir` 分析（不改行为）。
    if opts.optimize {
        // dce_mir(&mut mir_funcs);   // TODO(P5): 修复 CFG 损坏后启用
        // licm_mir(&mut mir_funcs);  // TODO(P5): 修复 CFG 损坏后启用
        let _escape = escape_mir(&mir_funcs); // 分析：标注未逃逸分配
    }

    // 5.5 P7: ARC 分析（逃逸分析 + 自动插入 + 优化 + 泄漏检测）
    if opts.optimize {
        let _arc_result = arc::run_arc_analysis(&mut mir_funcs);
    }

    // 6. MIR → 字节码发射
    emit_module(&hir, &mir_funcs, &ctx)
}

/// 从源码字符串编译为字节码模块（自动跑前端 + 语义检查）
pub fn compile_source(source: &str) -> Result<BytecodeModule, String> {
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::sema::analyze_source;

    // 词法
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    let lex_err = lexer
        .errors()
        .first()
        .map(|e| format!("lex error: {}", e.message));
    if let Some(m) = lex_err {
        return Err(m);
    }

    // 语法
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    let perrs: Vec<String> = parser
        .errors()
        .iter()
        .filter(|e| e.severity == crate::errors::ErrorSeverity::Error)
        .map(|e| format!("parse error: {}", e.message))
        .collect();
    if !perrs.is_empty() {
        return Err(perrs.join("\n"));
    }

    // 语义：P3 的类型检查存在已知局限（如泛型实例化），因此语义诊断仅作为
    // 警告输出，不阻断代码生成——降级阶段结构化处理，P4 目标是产出合法字节码。
    let (_ast, sema) = analyze_source(source);
    let serrs: Vec<String> = sema
        .errors
        .iter()
        .filter(|e| e.severity == crate::errors::ErrorSeverity::Error)
        .map(|e| format!("semantic warning: {}", e.message))
        .collect();
    for w in &serrs {
        eprintln!("{}", w);
    }

    Ok(compile(&program, &CodeGenOptions::default()))
}
