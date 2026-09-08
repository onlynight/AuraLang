//! AOT 编译后端（LLVM）
//!
//! 对应 `技术方案.md` 第九章。核心思路：
//!
//! 1. 将 HIR 生成为 **LLVM IR 文本**（不依赖 inkwell / llvm-sys 运行时绑定）
//! 2. 将 LLVM IR 写入 `.ll` 文件
//! 3. 调用外部 `llc` / `clang`（从检测到的 LLVM 安装路径）编译为 `.o` 或可执行文件
//!
//! 为什么用文本 IR 而非 inkwell 直接绑定？
//! - inkwell 最新版（0.10.0）最高支持 LLVM 19，而当前项目使用 LLVM 23.1.0
//! - 文本 IR 是 LLVM 一等公民，跨 LLVM 版本完全兼容
//! - 实现简单、调试方便、无需编译期链接 LLVM C API
//! - 与技术方案中 "C 后端备选" 思路一致，只是输出格式不同
//!
//! 未来升级路线：当 inkwell 支持 LLVM 23 后，可将 `Emit` 从文本 IR 迁移到 inkwell IRBuilder。
//!
//! 对外 API：
//! - [`aot_compile`]：从 HIR 生成目标文件（.o / .exe / .ll）
//! - [`AotCodeGenerator`]：完整控制器
//! - [`AotOptions`]：编译选项（目标三元组、优化级别、输出格式）

pub mod c_backend;
pub mod dwarf;
pub mod emit;
pub mod error;
pub mod ffi;
pub mod linker;
pub mod optimize;
pub mod runtime;
pub mod target;
pub mod types;

use std::path::Path;

use crate::codegen::CodegenError;
use crate::codegen::hir::HirProgram;

pub use error::AotError;
pub use linker::{
    LlvmToolError, LlvmToolResult, link_to_blob, link_to_executable, link_to_object,
    link_to_rust_host, link_to_shared_library,
};
pub use optimize::OptimizationLevel;
pub use target::{AOTargetTriple, Architecture, OperatingSystem, TargetTriple, Vendor};
pub use types::TypeMapper;

/// AOT 编译选项
#[derive(Debug, Clone)]
pub struct AotOptions {
    /// 目标三元组（默认当前主机）
    pub target: TargetTriple,
    /// 优化级别（默认 O2）
    pub opt_level: OptimizationLevel,
    /// 是否生成调试信息（默认 false）
    pub debug_info: bool,
    /// 是否将 String 表示为 `{ i8*, i64 }` 结构（默认 true，便于长度感知的字符串操作）
    pub string_as_struct: bool,
    /// 是否注入 runtime 库声明（默认 true）
    pub link_runtime: bool,
    /// Phase 4: 是否链接 std C FFI（默认 true）
    /// 启用后 AOT 生成的可执行文件可调用 aura_println/aura_math_sin 等 C ABI 函数
    pub link_std_cffi: bool,
    /// LLVM 工具链根目录（覆盖自动探测）
    pub llvm_home: Option<std::path::PathBuf>,
    /// 是否生成 C ABI 包装函数（默认 false）
    /// 启用后为每个函数生成裸 C ABI 包装（`define c i32 @aura_add(i32, i32)`），
    /// 带 `dllexport`/`visibility("default")`，供外部 C/Python 消费者调用。
    pub c_abi: bool,
}

impl Default for AotOptions {
    fn default() -> Self {
        Self {
            target: TargetTriple::default(),
            opt_level: OptimizationLevel::Aggressive,
            debug_info: false,
            string_as_struct: true,
            link_runtime: true,
            link_std_cffi: true,
            llvm_home: None,
            c_abi: false,
        }
    }
}

/// AOT 编译产物
#[derive(Debug)]
pub struct AotOutput {
    /// 生成的 `.ll` 文件路径（如果 output_format == LlvmIr）
    pub ll_path: Option<std::path::PathBuf>,
    /// 生成的 `.o` 文件路径（如果 output_format == Object）
    pub object_path: Option<std::path::PathBuf>,
    /// 生成的可执行文件路径（如果 output_format == Executable）
    pub exe_path: Option<std::path::PathBuf>,
    /// 生成的机器码 blob 路径（如果 output_format == Blob）
    pub blob_path: Option<std::path::PathBuf>,
    /// 生成的动态库路径（如果 output_format == SharedLibrary）
    pub shared_library_path: Option<std::path::PathBuf>,
    /// 编译期链接产物路径（如果 output_format == RustHost）
    pub rust_host_path: Option<std::path::PathBuf>,
    /// Blob 格式下提取到的 AOT 函数描述符：`(脱前缀函数名, 描述符)`
    ///
    /// 供 [`crate::codegen::aot_embed::embed_aot`] 组装段表使用。
    pub descriptors: Vec<(String, crate::codegen::opcode::AuraFuncDesc)>,
    /// 生成的 LLVM IR 文本
    pub ir_text: String,
}

/// 输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// 仅生成 LLVM IR 文本文件（`.ll`）
    LlvmIr,
    /// 生成目标文件（`.o` / `.obj`）
    Object,
    /// 生成可执行文件（`.exe` / ELF 等）
    Executable,
    /// 生成机器码 blob（原始 .text 段字节，嵌入 `.auc` 用）
    Blob,
    /// 生成动态库（Tier 2：`.so` / `.dylib` / `.dll`）
    SharedLibrary,
    /// 编译期链接到 Rust 宿主（Tier 4）
    RustHost,
}

/// AOT 代码生成器主结构体
pub struct AotCodeGenerator {
    options: AotOptions,
    type_mapper: TypeMapper,
}

impl AotCodeGenerator {
    /// 创建新的 AOT 代码生成器
    pub fn new(options: AotOptions) -> Self {
        Self {
            type_mapper: TypeMapper::new(options.string_as_struct),
            options,
        }
    }

    /// 从 HIR 程序生成 LLVM IR 文本（默认不生成包装函数）
    pub fn generate_ir(&self, program: &HirProgram) -> Result<String, AotError> {
        emit::emit_program(self, program, false)
    }

    /// 从 HIR 程序生成 LLVM IR 文本，可选生成 JitValue ABI 包装函数
    ///
    /// `blob_mode = true` 时为每个函数生成 JitValue ABI 包装函数
    /// （设计文档 §6.3-§6.5），用户函数标 `internal`，不合成 `main` 入口。
    /// 用于 `OutputFormat::Blob` 与 `OutputFormat::SharedLibrary`。
    pub fn generate_ir_with_mode(
        &self,
        program: &HirProgram,
        blob_mode: bool,
        _wrapper_exported: bool,
    ) -> Result<String, AotError> {
        emit::emit_program(self, program, blob_mode)
    }

    /// 完整 AOT 编译流程：HIR → LLVM IR → 目标文件
    pub fn compile(
        &self,
        program: &HirProgram,
        output_dir: &Path,
        output_format: OutputFormat,
    ) -> Result<AotOutput, AotError> {
        // Blob 与 SharedLibrary 都需要 JitValue ABI 包装函数（blob_mode = true）。
        // SharedLibrary 需要包装函数以 external linkage 导出，供 dlsym 查找。
        let blob_mode = matches!(
            output_format,
            OutputFormat::Blob | OutputFormat::SharedLibrary
        );
        let wrapper_exported = matches!(output_format, OutputFormat::SharedLibrary);
        let ir = self.generate_ir_with_mode(program, blob_mode, wrapper_exported)?;

        let stem = output_dir
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "output".to_string());

        let mut output = AotOutput {
            ll_path: None,
            object_path: None,
            exe_path: None,
            blob_path: None,
            shared_library_path: None,
            rust_host_path: None,
            descriptors: Vec::new(),
            ir_text: ir.clone(),
        };

        // 1. 写 .ll 文件（无论哪种输出格式，都保留 .ll 以便调试）
        let ll_path = output_dir.join(format!("{}.ll", stem));
        std::fs::write(&ll_path, &ir).map_err(|e| AotError::Io(e.to_string()))?;
        output.ll_path = Some(ll_path.clone());

        if output_format == OutputFormat::LlvmIr {
            return Ok(output);
        }

        // 2. 通过链接器生成目标文件
        let object_path = output_dir.join(format!(
            "{}.{}",
            stem,
            if cfg!(target_os = "windows") { "obj" } else { "o" }
        ));
        link_to_object(&ll_path, &object_path, &self.options)?;
        output.object_path = Some(object_path.clone());

        if output_format == OutputFormat::Object {
            return Ok(output);
        }

        // 3a. Blob 格式：提取 .text 段生成 blob 文件 + 函数描述符
        if output_format == OutputFormat::Blob {
            let blob_path = output_dir.join(format!("{}.blob", stem));
            let descs = link_to_blob(&object_path, &blob_path, &self.options)
                .map_err(|e| AotError::LinkerFailed(e.to_string()))?;
            output.blob_path = Some(blob_path);
            output.descriptors = descs;
            return Ok(output);
        }

        // 3b. SharedLibrary 格式（Tier 2）：生成动态库
        if output_format == OutputFormat::SharedLibrary {
            let lib_path = output_dir.join(format!(
                "{}{}",
                stem,
                if cfg!(target_os = "windows") {
                    ".dll"
                } else if cfg!(target_os = "macos") {
                    ".dylib"
                } else {
                    ".so"
                }
            ));
            linker::link_to_shared_library(&object_path, &lib_path, &self.options)
                .map_err(|e| AotError::LinkerFailed(e.to_string()))?;
            output.shared_library_path = Some(lib_path);
            return Ok(output);
        }

        // 3c. RustHost 格式（Tier 4）：编译期链接到 Rust 宿主
        if output_format == OutputFormat::RustHost {
            let host_path = output_dir.join(format!(
                "{}{}",
                stem,
                if cfg!(target_os = "windows") { ".exe" } else { "" }
            ));
            linker::link_to_rust_host(&object_path, &host_path, &self.options)
                .map_err(|e| AotError::LinkerFailed(e.to_string()))?;
            output.rust_host_path = Some(host_path);
            return Ok(output);
        }

        // 3d. Executable 格式：链接为可执行文件
        let exe_path = output_dir.join(format!(
            "{}{}",
            stem,
            if cfg!(target_os = "windows") { ".exe" } else { "" }
        ));
        link_to_executable(&object_path, &exe_path, &self.options)?;
        output.exe_path = Some(exe_path);

        Ok(output)
    }

    /// 从 `compile_source` 结果（Program）进行 AOT 编译（便捷入口）
    pub fn compile_program(
        &self,
        program: &crate::ast::Program,
        output_dir: &Path,
        output_format: OutputFormat,
    ) -> Result<AotOutput, AotError> {
        use crate::codegen::{
            desugar_program, fold_hir, inline_hir, mono_hir, synthesize_main_if_missing,
        };
        let mut hir = desugar_program(program);
        synthesize_main_if_missing(&mut hir);
        mono_hir(&mut hir);
        inline_hir(&mut hir);
        fold_hir(&mut hir);
        self.compile(&hir, output_dir, output_format)
    }
}

/// 一键 AOT 编译入口（从源码字符串到目标文件）
pub fn aot_compile(
    source: &str,
    output_path: &Path,
    options: AotOptions,
) -> Result<AotOutput, CodegenError> {
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        return Err(CodegenError::Aot(format!("lex: {}", e.message)));
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();
    if let Some(e) = parser.errors().first() {
        return Err(CodegenError::Aot(format!("parse: {}", e.message)));
    }

    let codegen = AotCodeGenerator::new(options);
    let output = codegen
        .compile_program(&program, output_path.parent().unwrap_or(Path::new(".")), {
            if output_path.extension().map(|e| e == "ll").unwrap_or(false) {
                OutputFormat::LlvmIr
            } else if output_path.extension().map(|e| e == "o" || e == "obj").unwrap_or(false) {
                OutputFormat::Object
            } else if output_path.extension().map(|e| e == "blob").unwrap_or(false) {
                OutputFormat::Blob
            } else if output_path
                .extension()
                .map(|e| e == "so" || e == "dylib" || e == "dll")
                .unwrap_or(false)
            {
                OutputFormat::SharedLibrary
            } else if output_path.extension().map(|e| e == "rust_host").unwrap_or(false) {
                OutputFormat::RustHost
            } else {
                OutputFormat::Executable
            }
        })
        .map_err(|e| CodegenError::Aot(e.to_string()))?;

    // 如果用户指定了非默认输出文件名，复制或重命名
    if output.exe_path.as_ref().map(|p| p != output_path).unwrap_or(false)
        || output.object_path.as_ref().map(|p| p != output_path).unwrap_or(false)
        || output.blob_path.as_ref().map(|p| p != output_path).unwrap_or(false)
        || output.shared_library_path.as_ref().map(|p| p != output_path).unwrap_or(false)
        || output.rust_host_path.as_ref().map(|p| p != output_path).unwrap_or(false)
    {
        if let Some(ref src) = output.exe_path {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        } else if let Some(ref src) = output.object_path {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        } else if let Some(ref src) = output.blob_path {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        } else if let Some(ref src) = output.shared_library_path {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        } else if let Some(ref src) = output.rust_host_path {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        }
    }

    Ok(output)
}
