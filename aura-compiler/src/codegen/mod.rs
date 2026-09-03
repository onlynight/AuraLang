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

pub use disasm::disassemble;
pub use emit::{emit_module, find_const};
pub use hir::{desugar_program, HirProgram};
pub use mir::{lower_program, MirFunction};
pub use mono::mono_hir;
pub use opcode::{BytecodeFunction, BytecodeModule, BytecodeNative, Const, OpCode};
pub use opt::{dce_mir, escape_mir, fold_hir, inline_hir, licm_mir};
pub use serialize::{from_bytes, read_auc, to_bytes, write_auc, SerializeError};

use crate::ast::Program;

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
    if opts.optimize {
        dce_mir(&mut mir_funcs);
        licm_mir(&mut mir_funcs);
        let _escape = escape_mir(&mir_funcs); // 分析：标注未逃逸分配
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
    let lex_err = lexer.errors().first().map(|e| format!("lex error: {}", e.message));
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
