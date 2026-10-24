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
    /// 是否将 String 表示为 `{ i8*, i64 }` 结构（默认 false，统一为 i8* C ABI 指针）
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
            string_as_struct: false,
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
        emit::emit_program(self, program, false, false, false)
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
        wrapper_exported: bool,
        c_abi: bool,
    ) -> Result<String, AotError> {
        emit::emit_program(self, program, blob_mode, wrapper_exported, c_abi)
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
        let ir =
            self.generate_ir_with_mode(program, blob_mode, wrapper_exported, self.options.c_abi)?;

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

        // Phase C: 注入并发原生函数声明（Thread/Mutex/Atomic/RwLock/Condvar/Barrier）
        inject_concurrent_natives(&mut hir);

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

    // 语义门禁（仅可靠检查）：实参数/重载违规必须硬失败。
    //
    // AOT 路径此前**完全不跑 sema**，实参数不匹配静默通过：缺失实参在 LLVM IR
    // 里落成未初始化的 `%var`（垃圾值），或调用点发射成不存在的符号。
    // 实测 `joinPieces(parts)` 漏传 `sep` ⇒ freeFuncs 整串损坏（2026-10-02）；
    // `String.indexOf(s, sub, start)` 三参形态（不存在的 API）发射
    // `@String_indexOf` → `llc: use of undefined value`。
    // 只对「no overload of …」这类来自真实符号签名的可靠诊断硬失败；
    // 其余语义诊断因 P3 泛型等已知误报不在此阻断。
    {
        let (_ast, sema) = crate::sema::checker::analyze_source(source);
        if crate::sema::checker::has_overload_violation(&sema.errors) {
            let msgs = crate::sema::checker::overload_violation_messages(&sema.errors);
            return Err(CodegenError::Aot(format!(
                "semantic error: call arity violation ({}):\n  {}",
                msgs.len(),
                msgs.join("\n  ")
            )));
        }
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

    // 如果用户指定了非默认输出文件名，复制到目标路径。
    //
    // 判断必须基于**实际将被复制的那一个产物**，而不是「任意产物路径与目标
    // 不同」。旧实现把 exe/obj/blob/... 全部串进 `||` 守卫：只要该次构建还
    // 产出了 `.obj`（几乎总是），即使 exe 路径已经等于目标，也会进入分支并
    // 无条件自拷贝 `exe_path → output_path`。当 `--output <dir>/<dir>.exe`
    // （产物名与输出目录同名）时二者是同一文件，Windows 上 `fs::copy` 会以
    // ERROR_SHARING_VIOLATION(32) 失败，构建「成功产出 exe 却报错」。
    //
    // ⚠️ `object_path` 必须排在**所有最终产物之后**。`.obj` 是中间件，几乎每次
    // 构建都存在；旧顺序把 `.obj` 放在 `shared_library_path` 之前，导致
    // `--shared --output x.dll` 复制的是 COFF 目标文件而非链接好的 DLL
    // （实测 `utils.dll` 只有 5375B 且以 `64 86`(COFF) 开头 → `ctypes` 报
    // WinError 193「不是有效的 Win32 应用程序」），P3.4 FFI demo 因此恒失败。
    let artifact_src = output
        .exe_path
        .as_ref()
        .or(output.shared_library_path.as_ref())
        .or(output.blob_path.as_ref())
        .or(output.rust_host_path.as_ref())
        .or(output.object_path.as_ref());
    if let Some(src) = artifact_src {
        if !same_file(src, output_path) {
            std::fs::copy(src, output_path).map_err(|e| CodegenError::Aot(e.to_string()))?;
        }
    }

    Ok(output)
}

/// P3.4: 由 Aura 源码生成 C 头文件（`aura export-header`）。
///
/// 输出内容与 `--cabi --shared` 产物的导出契约一致：
/// - 每个**用户函数**（非 native 声明、非合成入口）声明一条 `aura_c_<name>` 原型；
/// - 类型按既定 ABI 映射：`Int`→`int32_t`、`Long`→`int64_t`、`Float`/`Double`→`double`、
///   `Boolean`→`bool`、`Char`→`char`、`String`/`CString`/指针/`Any`→`const char*` / `void*`。
///
/// 生成过程复用与 AOT 相同的 HIR 降级链路（desugar + mono + inline + fold），
/// 因此头文件中的函数集合与 `--cabi` 实际导出的包装函数一一对应。
pub fn export_c_header(source: &str, entry: &str) -> Result<String, CodegenError> {
    use crate::codegen::{
        desugar_program, fold_hir, inline_hir, mono_hir, synthesize_main_if_missing,
    };
    use crate::codegen::hir::{HirFunction, HirType};
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

    let mut hir = desugar_program(&program);
    synthesize_main_if_missing(&mut hir);
    mono_hir(&mut hir);
    inline_hir(&mut hir);
    fold_hir(&mut hir);

    // HirType → C 类型名
    fn c_type(ty: Option<&HirType>) -> String {
        match ty {
            None => "void".to_string(),
            Some(HirType::Pointer(inner)) => format!("{}*", c_type(Some(inner))),
            Some(HirType::Nullable(inner)) => format!("{}*", c_type(Some(inner))),
            Some(HirType::Array { inner, .. }) => format!("{}*", c_type(Some(inner))),
            Some(HirType::Named(n)) => match n.as_str() {
                "Int" | "Int32" => "int32_t".to_string(),
                "Long" | "Int64" | "Size" | "u64" | "i64" => "int64_t".to_string(),
                "Float" | "Double" | "f64" => "double".to_string(),
                "Boolean" | "Bool" => "bool".to_string(),
                "Char" | "Byte" | "u8" => "char".to_string(),
                "CString" | "CStr" => "const char*".to_string(),
                "Unit" | "Nothing" | "Void" => "void".to_string(),
                // String / List / Map / Any / 用户类型 一律退化为不透明指针
                _ => "void*".to_string(),
            },
            Some(HirType::Function { .. }) => "void*".to_string(),
            Some(HirType::Unknown) => "void*".to_string(),
        }
    }

    fn is_user_fn(f: &HirFunction) -> bool {
        // 排除 native/FFI 声明、合成入口、以及类方法（含 `.` 或 `$` 的名字）
        if f.is_native {
            return false;
        }
        let n = f.name.as_str();
        if n == "main" || n.starts_with("$") {
            return false;
        }
        if n.contains('.') || n.contains('$') {
            return false;
        }
        true
    }

    let mut decls = String::new();
    let mut count = 0usize;
    for f in hir.functions.iter().filter(|f| is_user_fn(f)) {
        let ret = c_type(f.ret.as_ref());
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{} {}", c_type(p.ty.as_ref()), p.name))
            .collect();
        let param_str = if params.is_empty() { "void".to_string() } else { params.join(", ") };
        decls.push_str(&format!(
            "/* {} */\n{} aura_c_{}({});\n\n",
            f.name, ret, f.name, param_str
        ));
        count += 1;
    }

    let stem = entry
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("aura_ffi")
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or("aura_ffi");
    let guard = format!("{}_H", stem.replace(['.', '-'], "_").to_uppercase());

    let header = format!(
        "/*\n\
         * {stem}.h — C 头文件（由 `aura export-header` 生成）\n\
         *\n\
         * 对应 Aura AOT + --cabi --shared 产物的 C ABI 接口。\n\
         * 导出符号前缀为 aura_c_，调用约定为 C ABI (ccc)。\n\
         *\n\
         * 源文件：{entry}\n\
         * 构建：aura build {entry} --aot --shared --cabi --output <lib>.dll\n\
         */\n\n\
         #ifndef {guard}\n\
         #define {guard}\n\n\
         #include <stdint.h>\n\
         #include <stdbool.h>\n\n\
         #ifdef __cplusplus\n\
         extern \"C\" {{\n\
         #endif\n\n\
         {decls}\
         #ifdef __cplusplus\n\
         }}\n\
         #endif\n\n\
         #endif /* {guard} */\n",
        stem = stem,
        entry = entry,
        guard = guard,
        decls = decls,
    );

    let _ = count;
    Ok(header)
}
fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Phase C: 注入并发原生函数声明到 HIR 程序
///
/// 并发原生函数（Thread/Mutex/Atomic/RwLock/Condvar/Barrier）
/// 在 VM 端注册于 NativeRegistry，但 AOT 后端需要
/// 在 program.natives 中声明才能生成 LLVM IR 调用。
fn inject_concurrent_natives(program: &mut crate::codegen::hir::HirProgram) {
    use crate::codegen::hir::{HirFunction, HirParam, HirType};
    use crate::codegen::opcode::FfiAbi;

    let make_native = |name: &str, params: &[(&str, HirType)], ret: Option<HirType>| HirFunction {
        name: name.to_string(),
        params: params
            .iter()
            .map(|(n, t)| HirParam {
                name: n.to_string(),
                ty: Some(t.clone()),
                default_value: None,
                is_vararg: false,
            })
            .collect(),
        ret,
        body: crate::codegen::hir::HirBlock {
            stmts: Vec::new(),
        },
        is_native: true,
        type_params: Vec::new(),
        ffi_abi: FfiAbi::C,
        ffi_lib: None,
        native_attr: None,
    };

    let i64 = || HirType::Named("Int".to_string());
    let void = || Some(HirType::Named("Unit".to_string()));
    let bool_t = || Some(HirType::Named("Boolean".to_string()));
    let any_t = || HirType::Named("Any".to_string());

    // 检查是否已存在（避免重复注入）
    let existing_names: std::collections::HashSet<String> =
        program.natives.iter().map(|n| n.name.clone()).collect();

    let natives_to_add: Vec<HirFunction> = vec![
        // Thread
        make_native(
            "aura.lang.concurrent.Thread.spawn",
            &[
                ("fnId", i64()),
                ("arg", i64()),
            ],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Thread.join",
            &[("id", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Thread.sleep",
            &[("ms", i64())],
            void(),
        ),
        make_native("aura.lang.concurrent.Thread.id", &[], Some(i64())),
        make_native("aura.lang.concurrent.Thread.parallelism", &[], Some(i64())),
        make_native(
            "aura.lang.concurrent.Thread.availableCores",
            &[],
            Some(i64()),
        ),
        // Mutex
        make_native("aura.lang.concurrent.Mutex.new", &[], Some(i64())),
        make_native("aura.lang.concurrent.Mutex.lock", &[("id", i64())], void()),
        make_native(
            "aura.lang.concurrent.Mutex.unlock",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Mutex.tryLock",
            &[("id", i64())],
            bool_t(),
        ),
        make_native(
            "aura.lang.concurrent.Mutex.destroy",
            &[("id", i64())],
            void(),
        ),
        // Atomic
        make_native(
            "aura.lang.concurrent.Atomic.new",
            &[("initial", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Atomic.load",
            &[("id", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Atomic.store",
            &[
                ("id", i64()),
                ("val", i64()),
            ],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Atomic.add",
            &[
                ("id", i64()),
                ("delta", i64()),
            ],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Atomic.sub",
            &[
                ("id", i64()),
                ("delta", i64()),
            ],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Atomic.cas",
            &[
                ("id", i64()),
                ("expected", i64()),
                ("desired", i64()),
            ],
            bool_t(),
        ),
        // RwLock
        make_native("aura.lang.concurrent.RwLock.new", &[], Some(i64())),
        make_native(
            "aura.lang.concurrent.RwLock.readLock",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.RwLock.writeLock",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.RwLock.readUnlock",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.RwLock.writeUnlock",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.RwLock.destroy",
            &[("id", i64())],
            void(),
        ),
        // Condvar
        make_native("aura.lang.concurrent.Condvar.new", &[], Some(i64())),
        make_native(
            "aura.lang.concurrent.Condvar.wait",
            &[
                ("id", i64()),
                ("mutexId", i64()),
            ],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Condvar.signal",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Condvar.broadcast",
            &[("id", i64())],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Condvar.destroy",
            &[("id", i64())],
            void(),
        ),
        // Barrier
        make_native(
            "aura.lang.concurrent.Barrier.new",
            &[("count", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Barrier.wait",
            &[("id", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Barrier.destroy",
            &[("id", i64())],
            void(),
        ),
        // Channel
        make_native(
            "aura.lang.concurrent.Channel.newChannel",
            &[("cap", i64())],
            Some(i64()),
        ),
        make_native(
            "aura.lang.concurrent.Channel.channelSend",
            &[
                ("id", i64()),
                ("val", any_t()),
            ],
            void(),
        ),
        make_native(
            "aura.lang.concurrent.Channel.channelRecv",
            &[("id", i64())],
            Some(any_t()),
        ),
    ];

    for native in natives_to_add {
        if !existing_names.contains(&native.name) {
            program.natives.push(native);
        }
    }
}
