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
// Phase 1 AOT: 机器码嵌入 .auc v4（依赖 LLVM 后端）
#[cfg(feature = "llvm")]
pub mod aot_embed;

pub use disasm::disassemble;
pub use emit::{emit_module, find_const};
pub use hir::{HirProgram, desugar_program, desugar_program_with, synthesize_main_if_missing};
pub use mir::{MirFunction, lower_program};
pub use mono::mono_hir;
pub use opcode::{
    BytecodeFunction, BytecodeModule, BytecodeNative, Const, Dependency, ExportSymbol,
    ImportSymbol, ModuleIdentity, OpCode, SymbolKind,
};
pub use opt::{dce_mir, escape_mir, fold_hir, inline_hir, licm_mir};
pub use serialize::{SerializeError, from_bytes, read_auc, to_bytes, write_auc};

#[cfg(feature = "llvm")]
pub use aot::{AotCodeGenerator, AotError, AotOptions, AotOutput, OutputFormat, aot_compile};
#[cfg(feature = "llvm")]
pub use aot_embed::{AotEmbedResult, embed_aot};

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
    /// Phase 1c: 按需链接 — 启用的 std 模块名
    pub enabled_modules: Vec<String>,
}

impl Default for CodeGenOptions {
    fn default() -> Self {
        CodeGenOptions {
            optimize: true,
            enabled_modules: Vec::new(),
        }
    }
}

/// 从 AST 程序编译为字节码模块
pub fn compile(program: &Program, opts: &CodeGenOptions) -> BytecodeModule {
    compile_with_info(program, opts, &crate::sema::info::SemaInfo::default())
}

/// 从 AST 程序编译为字节码模块（带 sema 类型信息：类方法/运算符/访问器按接收者类型分派）
pub fn compile_with_info(
    program: &Program,
    opts: &CodeGenOptions,
    info: &crate::sema::info::SemaInfo,
) -> BytecodeModule {
    // 1. AST → HIR（去语法糖）
    let mut hir = desugar_program_with(program, Some(info));

    // 2. 脚本模式：合成隐式 main（若无 main 但有顶层语句）
    let _synthesized = synthesize_main_if_missing(&mut hir);

    // 3. 泛型单态化（HIR）
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
    let mut module = emit_module(&hir, &mir_funcs, &ctx);
    module.enabled_modules = opts.enabled_modules.clone();

    // Phase 2: 初始化模块标识与导出表
    module.module_identity = ModuleIdentity::new("default", "0.1.0");

    // Phase 2: 生成 SourceIndex（从 phantom source 提取符号）
    if let Some(idx) = generate_source_index_from_phantom() {
        module.source_index = Some(idx);
    }

    module.header_flags = module.compute_header_flags();
    module
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

    // Phase 1c: 提取启用的 std 模块（按需链接）
    let enabled_modules = extract_enabled_modules(&program);
    let opts = CodeGenOptions {
        optimize: true,
        enabled_modules,
    };

    Ok(compile_with_info(&program, &opts, &sema.info))
}

/// 预处理：解析 `import "path.aura"` 语句，将外部 `.aura` 文件内容内联
///
/// 用于支持 `extern interface` 声明放在独立文件中，通过 `import` 引入。
/// `file_path` 为当前源文件路径（用于解析相对路径），`None` 时无法解析相对导入。
pub fn resolve_aura_imports(source: &str, file_path: Option<&str>) -> String {
    let mut result = String::new();
    let mut base_dir = std::path::PathBuf::from(".");
    if let Some(fp) = file_path {
        base_dir =
            std::path::Path::new(fp).parent().unwrap_or(std::path::Path::new(".")).to_path_buf();
    }

    for line in source.lines() {
        let trimmed = line.trim_start();
        // 匹配 import "path" 或 import "path" as alias
        if let Some(rest) = trimmed.strip_prefix("import ") {
            let rest = rest.trim();
            if let Some(path_str) = rest.strip_prefix("\"") {
                let (path, _suffix) = split_at_quote(path_str);
                if path.ends_with(".aura") {
                    let full_path = base_dir.join(path);
                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        // 将导入文件的内容内联（跳过 import 行本身）
                        let content_lines: Vec<&str> = content.lines().collect();
                        let mut imported_content = String::new();
                        for cl in content_lines {
                            let cl_trimmed = cl.trim_start();
                            if cl_trimmed.starts_with("import ") {
                                continue; // 跳过嵌套 import
                            }
                            imported_content.push_str(cl);
                            imported_content.push('\n');
                        }
                        result.push_str(&imported_content);
                        continue; // 跳过原 import 行
                    } else {
                        eprintln!("[codegen] 无法读取导入文件: {}", full_path.display());
                    }
                }
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// 从字符串中找到第一个 `"` 的位置，返回 (前缀, 后缀)
fn split_at_quote(s: &str) -> (&str, &str) {
    if let Some(pos) = s.find('"') { (&s[..pos], &s[pos + 1..]) } else { (s, "") }
}

/// 从 AST 程序提取启用的 std 模块名
///
/// 遍历 `program.imports`，解析 `aura.math.*` / `import aura.math` 等语法，
/// 返回模块名集合（如 `["math", "io"]`）。
fn extract_enabled_modules(program: &crate::ast::Program) -> Vec<String> {
    let mut modules = std::collections::HashSet::new();

    for imp in &program.imports {
        let path = &imp.path;
        // 检查是否是 aura.* 命名空间
        if let Some(rest) = path.strip_prefix("aura.") {
            // 去掉可能的函数名（如 aura.math.sin → math）
            let module = rest.split('.').next().unwrap_or(rest);
            modules.insert(module.to_string());
        }
    }

    modules.into_iter().collect()
}

/// Phase 2: 从 phantom source 目录生成 SourceIndex
///
/// 尝试多个路径查找 phantom-source/ 目录：
/// 1. `AURA_PHANTOM_SOURCE` 环境变量
/// 2. `./phantom-source`（相对当前工作目录）
/// 3. `../phantom-source`（相对 crate 根目录）
/// 4. `../../phantom-source`（相对 src/codegen 目录）
///
/// 如果找不到目录或解析失败，返回 None（SourceIndex 为可选段）。
fn generate_source_index_from_phantom() -> Option<crate::std::source_index::SourceIndex> {
    use std::path::PathBuf;

    // 尝试多个路径
    let candidates = [
        std::env::var("AURA_PHANTOM_SOURCE").ok().map(PathBuf::from),
        Some(PathBuf::from("./phantom-source")),
        Some(PathBuf::from("../phantom-source")),
        Some(PathBuf::from("./src/codegen/../../phantom-source")),
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
                .join("phantom-source"),
        ),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.is_dir() {
            match crate::docgen::generate_source_index(&candidate) {
                Ok(idx) => {
                    eprintln!(
                        "[Phase 2] SourceIndex 已生成: {} 类型, {} 函数, {} 常量, {} 变量 (来源: {})",
                        idx.type_defs.len(),
                        idx.function_defs.len(),
                        idx.constant_defs.len(),
                        idx.variable_defs.len(),
                        candidate.display()
                    );
                    return Some(idx);
                }
                Err(e) => {
                    eprintln!(
                        "[Phase 2] SourceIndex 生成失败 ({}): {}",
                        candidate.display(),
                        e
                    );
                }
            }
        }
    }

    None
}
