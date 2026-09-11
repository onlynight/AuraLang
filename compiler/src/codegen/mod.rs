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

// Phase 3: 标准库集成
pub mod execution;
pub mod ffi_aot;
pub mod link_stdlib;
pub mod resolve_stdlib;

// Phase 5: FFI 缓存与优化
pub mod ffi_cache;
pub mod ffi_optimize;

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
    let (ast, sema) = analyze_source(source);
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
    let enabled_modules = extract_enabled_modules(&ast);
    let opts = CodeGenOptions {
        optimize: true,
        enabled_modules,
    };

    Ok(compile_with_info(&ast, &opts, &sema.info))
}

/// 自举编译器包根：`aura.lang.compiler` 映射到入口文件所在目录
/// （即 `aura/lang/compiler/`），用于把「包名 import」解析为相对 `.aura` 文件路径。
///
/// 包名 import 形如（无引号，与 `aura.lang.std.*` 风格一致）：
///   import aura.lang.compiler.lexer.Span
/// 末段为模块文件名（去掉 `.aura`），前段为子包目录；
/// `aura.lang.compiler.lexer.Span` → `lexer/Span.aura`（相对入口目录）。
const COMPILER_PKG_ROOT: &str = "aura.lang.compiler.";

/// 预处理：解析 `import` 语句，将外部模块内容内联。
///
/// 支持三种形式：
/// 1. `import "path.aura"`（引号 + 文件路径，相对入口目录）—— 内联文件内容；
/// 2. `import aura.lang.compiler.<pkg>.<Mod>`（包名，无引号）—— 映射为
///    `<pkg>/<Mod>.aura` 后内联；
/// 3. 其余（如 `import aura.lang.std.String`）原样透传，交给 VM 模块系统。
///
/// `file_path` 为当前源文件路径（用于解析相对路径），`None` 时无法解析相对导入。
pub fn resolve_aura_imports(source: &str, file_path: Option<&str>) -> String {
    // `pkg_root`：`aura.lang.compiler` 包根目录（即入口文件所在目录
    // `aura/lang/compiler/`），包名 import 一律相对它解析。
    // `base_dir`：当前正在处理的文件的目录，仅用于路径形式 import
    // （`import "x.aura"`），相对该文件解析。
    let pkg_root = match file_path {
        Some(fp) => {
            std::path::Path::new(fp).parent().unwrap_or(std::path::Path::new(".")).to_path_buf()
        }
        None => std::path::PathBuf::from("."),
    };
    let mut visited = std::collections::HashSet::new();
    resolve_aura_imports_rec(source, &pkg_root, &pkg_root, &mut visited)
}

/// 递归解析 import：除入口文件的顶层 import 外，被内联文件内部的 import
/// 也必须解析（否则同包内隐式引用的模块不会编译进模块图，导致
/// `use of undefined value` 链接错误）。同一个文件只内联一次（按规范路径去重），
/// 避免多路径重复 import 造成的重复定义。
fn resolve_aura_imports_rec(
    source: &str,
    pkg_root: &std::path::Path,
    base_dir: &std::path::Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
) -> String {
    let mut result = String::new();

    for line in source.lines() {
        let trimmed = line.trim_start();
        // 包声明（如 `package aura.lang.compiler.lexer`）由编译器消费，
        // 不参与 AST；必须在预处理阶段剥离，否则解析器会将其当作非法声明。
        if trimmed.starts_with("package ") {
            continue;
        }
        // 匹配 `import "path"` 或 `import <pkg>`
        if let Some(rest) = trimmed.strip_prefix("import ") {
            let rest = rest.trim();
            // 计算需要内联的目标文件路径（若有）
            let target: Option<std::path::PathBuf> = if let Some(path_str) = rest.strip_prefix("\"")
            {
                let (path, _suffix) = split_at_quote(path_str);
                if path.ends_with(".aura") {
                    Some(base_dir.join(path))
                } else if path.starts_with(COMPILER_PKG_ROOT) {
                    let rel =
                        pkg_to_aura_path(path.strip_prefix(COMPILER_PKG_ROOT).unwrap_or(path));
                    Some(pkg_root.join(rel))
                } else {
                    None
                }
            } else if let Some(pkg) = rest.strip_prefix(COMPILER_PKG_ROOT) {
                let rel = pkg_to_aura_path(pkg);
                Some(pkg_root.join(rel))
            } else {
                None
            };

            if let Some(full_path) = target {
                // 规范化路径以便跨不同相对写法的去重（如 `./x.aura` 与 `x.aura`）。
                let canon = std::fs::canonicalize(&full_path).unwrap_or_else(|_| full_path.clone());
                if visited.contains(&canon) {
                    continue; // 已内联，跳过避免重复定义
                }
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    visited.insert(canon);
                    // 被导入文件内部的路径 import 相对其自身目录解析；
                    // 包名 import 永远相对包根，故 pkg_root 透传。
                    let child_base = full_path.parent().unwrap_or(pkg_root);
                    let resolved =
                        resolve_aura_imports_rec(&content, pkg_root, child_base, visited);
                    result.push_str(&resolved);
                    continue; // 跳过原 import 行
                } else {
                    eprintln!("[codegen] 无法读取导入文件: {}", full_path.display());
                }
            }
            // 未识别的 import（如 aura.lang.std.*）原样保留，交给 VM 模块系统
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

/// 将包名末段转换为相对 `.aura` 文件路径。
///
/// 例：`lexer.Span` → `lexer/Span.aura`；`Parser` → `Parser.aura`。
/// 前段（若有）按 `.` 拆成目录层级，末段补 `.aura` 后缀。
fn pkg_to_aura_path(pkg: &str) -> String {
    let parts: Vec<&str> = pkg.split('.').collect();
    match parts.split_last() {
        Some((last, init)) if !init.is_empty() => {
            format!("{}/{}.aura", init.join("/"), last)
        }
        _ => format!("{}.aura", pkg),
    }
}

/// 从 AST 程序提取启用的 std 模块名
///
/// 遍历 `program.imports`，解析 `import` 声明，返回模块名集合（如 `["math", "io"]`）。
///
/// 新命名（`aura.lang.std.<ClassName>`）：
/// - `aura.lang.std.Coroutine.*` → `["concurrent"]`（Coroutine/Actor/Channel 共用一个模块键）
/// - `aura.lang.std.Math.*`     → `["math"]`
/// - `aura.lang.std.FileSystem` → `["fs"]`
/// - `aura.lang.std.Network`    → `["net"]`
///
/// 旧命名（`aura.<module>` / `aura.<module>.<fn>`）保留兼容。
fn extract_enabled_modules(program: &crate::ast::Program) -> Vec<String> {
    use std::collections::HashSet;
    let mut modules = HashSet::new();

    for imp in &program.imports {
        let path = &imp.path;
        // 自举编译器包内 import（aura.lang.compiler.*）已由 resolve_aura_imports
        // 内联处理，不映射到任何 std 模块，避免污染启用的模块集合。
        if path.starts_with("aura.lang.compiler.") {
            continue;
        }
        if let Some(rest) = path.strip_prefix("aura.lang.std.") {
            // 新命名：路径形如 aura.lang.std.<ClassName>[.<fn>]
            let class = rest.split('.').next().unwrap_or(rest);
            let mod_name: &str = match class {
                "Coroutine" | "Actor" | "Channel" => "concurrent",
                "FileSystem" => "fs",
                "Network" => "net",
                "Math" => "math",
                "IO" => "io",
                "Ascii" => "ascii",
                "Assert" => "assert",
                "Builtin" => "builtin",
                "Collections" => "collections",
                "Console" => "console",
                "Encoding" => "encoding",
                "Env" => "env",
                "Iter" => "iter",
                "Json" => "json",
                "Path" => "path",
                "Process" => "process",
                "Random" => "random",
                "String" => "string",
                "Test" => "test",
                "Time" => "time",
                _ => "",
            };
            let owned = if mod_name.is_empty() {
                class.to_ascii_lowercase()
            } else {
                String::from(mod_name)
            };
            modules.insert(owned);
        } else if let Some(rest) = path.strip_prefix("aura.") {
            // 旧命名：路径形如 aura.<module>[.<fn>]
            let module = rest.split('.').next().unwrap_or(rest);
            modules.insert(module.to_string());
        }
    }

    modules.into_iter().collect()
}

/// Phase 2: 从 core 目录生成 SourceIndex
///
/// 尝试多个路径查找 core/ 目录（原 phantom-source/，已重命名）：
/// 1. `AURA_CORE_SOURCE` 环境变量（新）
/// 2. `AURA_PHANTOM_SOURCE` 环境变量（旧，向后兼容）
/// 3. `./core`（相对当前工作目录）
/// 4. `../core`（相对 crate 根目录）
/// 5. `../../core`（相对 src/codegen 目录）
/// 6. `./phantom-source`（向后兼容）
/// 7. `../phantom-source`（向后兼容）
///
/// 如果找不到目录或解析失败，返回 None（SourceIndex 为可选段）。
fn generate_source_index_from_phantom() -> Option<crate::std::source_index::SourceIndex> {
    use std::path::PathBuf;

    // 尝试多个路径（新名优先，旧名向后兼容）
    let candidates = [
        std::env::var("AURA_CORE_SOURCE").ok().map(PathBuf::from),
        std::env::var("AURA_PHANTOM_SOURCE").ok().map(PathBuf::from),
        Some(PathBuf::from("./core")),
        Some(PathBuf::from("../core")),
        Some(PathBuf::from("./src/codegen/../../core")),
        Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
                .join("core"),
        ),
        // 向后兼容：旧名 phantom-source
        Some(PathBuf::from("./phantom-source")),
        Some(PathBuf::from("../phantom-source")),
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

/// Phase 3: Compile source code with stdlib linking.
///
/// This function:
/// 1. Compiles the application source code into a bytecode module
/// 2. Loads stdlib .auc files from the specified directory
/// 3. Links stdlib symbols into the application module
/// 4. Resolves stdlib function/type/constant references
/// 5. Configures execution mode (VM/JIT/AOT)
/// 6. Configures FFI AOT direct calls
///
/// # Arguments
/// * `source` - Application Aura source code
/// * `stdlib_auc_dir` - Directory containing stdlib .auc files
/// * `execution_mode` - Target execution mode
/// * `ffi_mode` - FFI mode (Aot/Cffi/RustFfi)
///
/// # Returns
/// A fully linked `BytecodeModule` ready for execution.
pub fn compile_source_with_stdlib(
    source: &str,
    stdlib_auc_dir: &std::path::Path,
    execution_mode: ffi_aot::ExecutionMode,
    ffi_mode: FfiMode,
) -> Result<BytecodeModule, String> {
    // 1. Compile application source code
    let mut module = compile_source(source)?;

    // 2. Link stdlib symbols
    let link_result = link_stdlib::link_stdlib_symbols(&mut module, stdlib_auc_dir)?;
    eprintln!(
        "stdlib linked: {} modules, {} symbols",
        link_result.modules_linked, link_result.symbols_resolved
    );

    // 3. Resolve stdlib calls
    let stdlib_exports: Vec<_> = module.exports.iter().cloned().collect();
    let resolution_result = resolve_stdlib::resolve_stdlib_calls(&module, &stdlib_exports);
    eprintln!(
        "stdlib resolved: {} symbols, {} unresolved",
        resolution_result.resolved.len(),
        resolution_result.unresolved.len()
    );

    // 4. Configure execution mode
    let exec_config = execution::ExecutionConfig {
        mode: match execution_mode {
            ffi_aot::ExecutionMode::Vm => execution::ExecutionMode::Vm,
            ffi_aot::ExecutionMode::Jit => execution::ExecutionMode::Jit,
            ffi_aot::ExecutionMode::Aot => execution::ExecutionMode::Aot,
        },
        ..Default::default()
    };
    let exec_result = execution::configure_execution_mode(&mut module, &exec_config);
    eprintln!("execution configured: {}", exec_result.mode);

    // 5. Configure FFI AOT direct calls (if AOT mode)
    if ffi_mode == FfiMode::Aot {
        let ffi_config = ffi_aot::FfiAotConfig::default();
        let ffi_result = ffi_aot::configure_ffi_aot_direct(&module, execution_mode, &ffi_config)?;
        eprintln!(
            "FFI AOT configured: {} declarations ({})",
            ffi_result.declarations_configured, ffi_result.execution_mode
        );
    }

    Ok(module)
}

/// FFI mode for Phase 3 compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FfiMode {
    #[default]
    Aot,
    Cffi,
    RustFfi,
}
