//! 目标代码生成与链接
//!
//! 对应 技术方案 §9.5.2 / §9.5.3 ObjectGenerator + 交叉编译。
//!
//! 本模块通过调用外部 `llc`（LLVM 静态编译器）和 `clang`（LLVM 编译器）
//! 将生成的 LLVM IR 编译为目标文件或可执行文件。
//!
//! 工具路径探测：
//! 1. `AotOptions.llvm_home` 显式指定
//! 2. 运行时环境变量 `AURA_LLVM_HOME`
//! 3. 编译时配置（根 Cargo.toml `[workspace.metadata.aura]` 中的 `llvm-home`）
//! 4. 编译时配置中的 `llvm-search-paths`（按优先级探测）
//! 5. 环境变量 `PATH`（系统 PATH 中的 llc / clang）
//!
//! 交叉编译通过 `-mtriple` 参数传递给 llc / clang。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::codegen::opcode::{AuraFuncDesc, FUNC_EXPORT};

use super::AotOptions;
use super::error::AotError;
use super::target::TargetTriple;

/// 调用外部 LLVM 工具的错误
#[derive(Debug)]
pub struct LlvmToolError {
    pub tool: String,
    pub status: std::process::ExitStatus,
    pub stderr: String,
}

impl std::fmt::Display for LlvmToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} execution failed (exit code {:?}): {}",
            self.tool, self.status, self.stderr
        )
    }
}

impl std::error::Error for LlvmToolError {}

/// LLVM 工具调用结果
pub type LlvmToolResult<T> = std::result::Result<T, LlvmToolError>;

/// 查找 LLVM 工具二进制
fn find_tool<'a>(name: &str, options: &'a AotOptions) -> Option<PathBuf> {
    // 1. 显式指定的 llvm_home
    if let Some(ref home) = options.llvm_home {
        let path = home.join("bin").join(format!(
            "{}{}",
            name,
            if cfg!(target_os = "windows") { ".exe" } else { "" }
        ));
        if path.exists() {
            return Some(path);
        }
    }

    // 2. 运行时环境变量 AURA_LLVM_HOME
    if let Ok(home) = std::env::var("AURA_LLVM_HOME") {
        let path = PathBuf::from(home).join("bin").join(format!(
            "{}{}",
            name,
            if cfg!(target_os = "windows") { ".exe" } else { "" }
        ));
        if path.exists() {
            return Some(path);
        }
    }

    // 3. 编译时配置（来自根 Cargo.toml [workspace.metadata.aura] llvm-home）
    if let Some(home) = option_env!("AURA_CONFIG_LLVM_HOME") {
        let path = PathBuf::from(home).join("bin").join(format!(
            "{}{}",
            name,
            if cfg!(target_os = "windows") { ".exe" } else { "" }
        ));
        if path.exists() {
            return Some(path);
        }
    }

    // 4. 编译时配置中的搜索路径（分号分隔）
    if let Some(search_paths) = option_env!("AURA_CONFIG_LLVM_SEARCH_PATHS") {
        for home in search_paths.split(';') {
            let home = home.trim();
            if home.is_empty() {
                continue;
            }
            let path = PathBuf::from(home).join("bin").join(format!(
                "{}{}",
                name,
                if cfg!(target_os = "windows") { ".exe" } else { "" }
            ));
            if path.exists() {
                return Some(path);
            }
        }
    }

    // 5. PATH
    if let Ok(output) = Command::new("where").arg(name).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = stdout.lines().next() {
                if !first.trim().is_empty() {
                    return Some(PathBuf::from(first.trim()));
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    if let Ok(output) = Command::new("which").arg(name).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = stdout.lines().next() {
                if !first.trim().is_empty() {
                    return Some(PathBuf::from(first.trim()));
                }
            }
        }
    }

    None
}

/// 构建通用的 LLVM 工具命令
fn build_command(tool: &str, options: &AotOptions) -> Result<Command, AotError> {
    let tool_path = find_tool(tool, options).ok_or_else(|| {
        AotError::ToolError(format!(
            "LLVM tool '{}' not found. Set AURA_LLVM_HOME or add LLVM bin to PATH",
            tool
        ))
    })?;

    let mut cmd = Command::new(tool_path);

    // 目标三元组：llc 使用 `-mtriple`，clang 使用 `-target`
    if tool == "llc" {
        cmd.arg("-mtriple").arg(options.target.to_string());
    } else {
        cmd.arg("-target").arg(options.target.to_string());
    }

    Ok(cmd)
}

/// 将 `.ll` 文件编译为 `.o` 目标文件（通过 `llc`）
pub fn link_to_object(
    ll_path: &Path,
    object_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError> {
    let mut cmd = build_command("llc", options)?;
    cmd.arg(ll_path)
        .arg("-o")
        .arg(object_path)
        .arg(options.opt_level.as_llvm_flag())
        .arg("-filetype=obj");

    // 调试信息：DWARF 元数据已在 LLVM IR 文本中生成（!DIFile / !DISubprogram），
    // llc 无需 -g 标志；仅 clang 链接时需要 -g 保留调试信息。
    // 注意：llc 不支持 -g，传入会导致 "Unknown command line argument" 错误。

    run_and_report(&mut cmd, "llc")?;
    Ok(())
}

/// 将 `.ll` 或 `.o` 文件链接为可执行文件
///
/// - Windows：优先使用 `clang`（自带 MSVC 运行库，正确解析 `__chkstk` 等
///   栈探测符号）；若 clang 不可用则回退 `lld-link`（需 `/entry` + `/subsystem`）
/// - Linux/macOS：使用 `clang`（自动选择合适的链接器）
pub fn link_to_executable(
    input_path: &Path,
    exe_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError> {
    // Phase 4: 如果启用 std C FFI，先编译 C FFI 源文件
    let mut cffi_object_path = None;
    if options.link_std_cffi {
        cffi_object_path = Some(compile_std_cffi(options)?);
    }

    #[cfg(target_os = "windows")]
    {
        use super::target::OperatingSystem;
        if options.target.os == OperatingSystem::Windows {
            // 优先 clang：它链接 MSVC CRT，自动提供 `__chkstk`（大栈帧必需）
            if let Some(clang_path) = find_tool("clang", options) {
                let mut cmd = Command::new(clang_path);
                cmd.arg(input_path);
                if let Some(ref cffi_obj) = cffi_object_path {
                    cmd.arg(cffi_obj);
                }
                cmd.arg("-o").arg(exe_path).arg(options.opt_level.as_llvm_flag());
                if options.debug_info {
                    cmd.arg("-g");
                }
                run_and_report(&mut cmd, "clang")?;
                return Ok(());
            }
            // 回退 lld-link（不提供 __chkstk，仅适用于小栈帧程序）
            let tool_path = find_tool("lld-link", options).ok_or_else(|| {
                AotError::ToolError(
                    "clang or lld-link not found. Set AURA_LLVM_HOME or add LLVM bin to PATH"
                        .to_string(),
                )
            })?;
            let mut cmd = Command::new(tool_path);
            cmd.arg(input_path);
            if let Some(ref cffi_obj) = cffi_object_path {
                cmd.arg(cffi_obj);
            }
            cmd.arg(format!("/out:{}", exe_path.display()))
                .arg("/entry:main")
                .arg("/subsystem:console");
            run_and_report(&mut cmd, "lld-link")?;
            return Ok(());
        }
    }

    // 非 Windows 目标或非 Windows 主机：用 clang
    let mut cmd = build_command("clang", options)?;
    cmd.arg(input_path);
    if let Some(ref cffi_obj) = cffi_object_path {
        cmd.arg(cffi_obj);
    }
    cmd.arg("-o").arg(exe_path).arg(options.opt_level.as_llvm_flag());
    if options.debug_info {
        cmd.arg("-g");
    }

    run_and_report(&mut cmd, "clang")?;
    Ok(())
}

/// Phase 4: 编译 std C FFI 源文件为目标文件
///
/// 编译 `compiler/src/std/cffi/aura_std_cffi.c` 为 `.o`/`.obj` 文件，
/// 供 AOT 可执行文件链接使用。
fn compile_std_cffi(options: &AotOptions) -> Result<PathBuf, AotError> {
    // C FFI 源文件路径
    let cffi_src = concat!(env!("CARGO_MANIFEST_DIR"), "/src/std/cffi/aura_std_cffi.c");
    let cffi_header = concat!(env!("CARGO_MANIFEST_DIR"), "/src/std/cffi/aura_std_cffi.h");

    // 输出文件路径（临时文件）
    let ext = if cfg!(target_os = "windows") { "obj" } else { "o" };
    let tmp_dir = std::env::temp_dir();
    let cffi_obj = tmp_dir.join(format!("aura_std_cffi.{}", ext));

    // 找 clang
    let clang_path = find_tool("clang", options).ok_or_else(|| {
        AotError::ToolError("clang not found, cannot compile std C FFI".to_string())
    })?;

    // 编译命令
    let mut cmd = Command::new(clang_path);
    cmd.arg("-c")
        .arg(cffi_src)
        .arg("-o")
        .arg(&cffi_obj)
        .arg("-I")
        .arg(Path::new(cffi_header).parent().unwrap());

    run_and_report(&mut cmd, "clang")?;

    Ok(cffi_obj)
}

/// 执行命令并报告结果
fn run_and_report(cmd: &mut Command, tool_name: &str) -> Result<(), AotError> {
    let output = cmd
        .output()
        .map_err(|e| AotError::ToolError(format!("failed to start {}: {}", tool_name, e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        return Err(AotError::LinkerFailed(format!(
            "{} failed:\nstdout: {}\nstderr: {}",
            tool_name, stdout, stderr
        )));
    }

    Ok(())
}

/// 获取当前 AOT 后端使用的 LLVM 工具路径（调试用）
pub fn tool_paths(options: &AotOptions) -> (Option<PathBuf>, Option<PathBuf>) {
    let llc = find_tool("llc", options);
    let clang = find_tool("clang", options);
    (llc, clang)
}

/// 交叉编译配置（供高级用户使用）
#[derive(Debug, Clone)]
pub struct CrossCompilationConfig {
    /// 目标三元组
    pub target_triple: TargetTriple,
    /// sysroot 路径（交叉编译工具链根目录）
    pub sysroot: Option<PathBuf>,
    /// 链接器路径
    pub linker: Option<PathBuf>,
    /// C 标准库路径
    pub c_stdlib: Option<PathBuf>,
}

impl CrossCompilationConfig {
    /// 为树莓派 4（aarch64）创建配置
    pub fn for_raspberry_pi4(sysroot: Option<PathBuf>) -> Self {
        Self {
            target_triple: TargetTriple::linux_aarch64(),
            sysroot: sysroot.clone(),
            linker: Some(PathBuf::from(
                option_env!("AURA_CONFIG_CROSS_LINKER_AARCH64").unwrap_or("aarch64-linux-gnu-gcc"),
            )),
            c_stdlib: sysroot.as_ref().map(|s| s.join("usr").join("lib")),
        }
    }

    /// 为树莓派 3 / Zero 2（armv7）创建配置
    pub fn for_raspberry_pi3(sysroot: Option<PathBuf>) -> Self {
        Self {
            target_triple: TargetTriple::linux_armv7(),
            sysroot: sysroot.clone(),
            linker: Some(PathBuf::from(
                option_env!("AURA_CONFIG_CROSS_LINKER_ARMV7").unwrap_or("arm-linux-gnueabihf-gcc"),
            )),
            c_stdlib: sysroot.as_ref().map(|s| s.join("usr").join("lib")),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1 AOT Blob: 从目标文件提取 .text 段 + 函数描述符
// (设计文档 §6.2 link_to_blob)
// ─────────────────────────────────────────────────────────────────────────────

/// 构造一个 `LlvmToolError`（用于非 LLVM 工具场景，如对象文件解析）
fn make_tool_error(tool: &str, msg: &str) -> LlvmToolError {
    LlvmToolError {
        tool: tool.to_string(),
        status: std::process::ExitStatus::default(),
        stderr: msg.to_string(),
    }
}

/// 解析对象文件，提取 .text 段原始字节、段起始虚拟地址、以及符号表。
///
/// 使用 `object` crate 的 `File::parse` 自动检测 ELF/COFF/PE 格式。
/// ELF 的 symbol.address() 是绝对虚拟地址，COFF 的是段内偏移。
fn parse_object_file(
    bytes: &[u8],
) -> Result<(Vec<u8>, u64, bool, Vec<(String, u64)>), LlvmToolError> {
    use object::read::{File, Object, ObjectSection, ObjectSymbol};

    let file = File::parse(bytes).map_err(|e| {
        make_tool_error(
            "parse_object",
            &format!("failed to parse object file: {}", e),
        )
    })?;

    let is_elf = matches!(file.format(), object::BinaryFormat::Elf);

    // 查找 .text 段
    let mut text_data: Vec<u8> = Vec::new();
    let mut text_start: u64 = 0;
    if let Some(section) = file.section_by_name(".text") {
        text_data = section
            .data()
            .map_err(|e| {
                make_tool_error(
                    "parse_object",
                    &format!("failed to read .text section: {}", e),
                )
            })?
            .to_vec();
        text_start = section.address();
    }
    if text_data.is_empty() {
        return Err(make_tool_error(
            "parse_object",
            ".text section not found or empty in object file",
        ));
    }

    // 收集所有已定义符号
    let mut symbols: Vec<(String, u64)> = Vec::new();
    for symbol in file.symbols() {
        if symbol.is_undefined() {
            continue;
        }
        if let Ok(name) = symbol.name() {
            symbols.push((name.to_string(), symbol.address()));
        }
    }

    Ok((text_data, text_start, is_elf, symbols))
}

/// 从 AOT 包装函数符号名解析元数据
///
/// 符号名格式：`aura_aot_<sanitized_name>!<nargs>!<rettag>!<tag0>!<tag1>!...`
/// 返回：(函数部分, num_args, return_tag, arg_tags)
fn parse_aot_symbol_name(name: &str) -> Option<(&str, u8, u8, Vec<u8>)> {
    if !name.starts_with("aura_aot_") {
        return None;
    }
    let parts: Vec<&str> = name.split('!').collect();
    if parts.len() < 3 {
        return None; // 至少需要 name!nargs!rettag
    }
    // 去掉 `aura_aot_` 前缀，得到 emit::sanitizellvm 处理后的函数名
    let func_part = &parts[0]["aura_aot_".len()..];
    let nargs: u8 = parts[1].parse().ok()?;
    let rettag: u8 = parts[2].parse().ok()?;
    let arg_tags: Vec<u8> = parts[3..].iter().filter_map(|s| s.parse().ok()).collect();
    Some((func_part, nargs, rettag, arg_tags))
}

/// 将参数类型标签列表编码为 AuraFuncDesc.arg_tags (u8)
///
/// 每参数 4 bit，最多 2 个参数使用紧凑编码；超过 2 个参数时，
/// 低 4 bit 存储参数个数（扩展编码标记）。
fn compute_arg_tags(arg_tags: &[u8]) -> u8 {
    if arg_tags.len() <= 2 {
        // 紧凑编码：tag0 在 bit 4-7，tag1 在 bit 0-3
        let t0 = arg_tags.first().copied().unwrap_or(0) & 0x0F;
        let t1 = arg_tags.get(1).copied().unwrap_or(0) & 0x0F;
        (t0 << 4) | t1
    } else {
        // 扩展编码标记：低 4 bit = 参数个数
        (arg_tags.len() as u8) & 0x0F
    }
}

/// 将 LLVM 目标文件编译为机器码 blob（不链接，不生成可执行文件）。
///
/// 输出：
/// - `blob_path`：原始 .text 段字节流
/// - 返回值：`(脱前缀函数名, 描述符)` 向量（每个 `aura_aot_*` 符号一个）
///
/// 对应设计文档 §6.2。使用纯 Rust `object` crate 解析 ELF/COFF 目标文件，
/// 提取 .text 段原始字节和 `aura_aot_*` 包装函数符号，计算入口偏移。
pub fn link_to_blob(
    object_path: &Path,
    blob_path: &Path,
    _options: &AotOptions,
) -> LlvmToolResult<Vec<(String, AuraFuncDesc)>> {
    // 1. 读取目标文件
    let bytes = std::fs::read(object_path).map_err(|e| {
        make_tool_error("read_object", &format!("failed to read object file: {}", e))
    })?;

    // 2. 解析目标文件，提取 .text 数据和符号表
    let (text_data, text_start, is_elf, symbols) = parse_object_file(&bytes)?;

    // 3. 写入 blob 文件
    std::fs::write(blob_path, &text_data)
        .map_err(|e| make_tool_error("write_blob", &format!("failed to write blob file: {}", e)))?;

    // 4. 为每个 aura_aot_* 符号生成函数描述符
    let mut descs = Vec::new();
    for (name, address) in &symbols {
        if !name.starts_with("aura_aot_") {
            continue;
        }

        // 解析元数据（从符号名中提取函数名/nargs/rettag/arg_tags）
        let meta = parse_aot_symbol_name(name);
        let (func_name, nargs, rettag, arg_tags_vec) = match meta {
            Some((func_name, nargs, rettag, arg_tags)) => (func_name, nargs, rettag, arg_tags),
            None => {
                // 跳过没有元数据的符号（如纯函数符号，无 `!` 分隔符）
                continue;
            }
        };

        // 计算 entry_offset（相对于 .text 段起始）
        let offset = if is_elf {
            if *address >= text_start {
                address.wrapping_sub(text_start)
            } else {
                0 // 无效偏移，后续会报错
            }
        } else {
            // COFF: symbol.address() 已经是段内偏移
            *address
        };

        if offset == 0 {
            return Err(make_tool_error(
                "link_to_blob",
                &format!(
                    "function '{}' has entry_offset 0 (address={:#x}, text_start={:#x}), \
                     please check if this symbol is within the .text section",
                    name, address, text_start
                ),
            ));
        }

        // 编码 arg_tags
        let arg_tags_u8 = compute_arg_tags(&arg_tags_vec);

        // 创建描述符（name_offset / name_len 由序列化模块填充）
        let desc = AuraFuncDesc {
            name_offset: 0,
            name_len: 0,
            _pad1: 0,
            entry_offset: offset,
            num_args: nargs,
            arg_tags: arg_tags_u8,
            return_tag: rettag,
            flags: FUNC_EXPORT,
            source_line: 0,
            source_file_offset: 0,
            _pad2: 0,
        };
        descs.push((func_name.to_string(), desc));
    }

    Ok(descs)
}

// ==================== Phase 4: 动态库与编译期链接 ====================

/// Tier 2: 将目标文件链接为动态库（`.so` / `.dylib` / `.dll`）
///
/// 动态库模式允许 AOT 编译的模块在运行时通过 `dlopen`/`LoadLibrary` 加载，
/// 用于插件系统、第三方模块扩展等场景。
///
/// 与 `link_to_executable` 保持一致：若 `link_std_cffi = true`，先编译
/// `aura_std_cffi.c` 为目标文件并一并链接，确保 Aura 代码中调用的
/// `aura_println` / `aura_malloc` 等 C FFI 符号在动态库内被解析。
pub fn link_to_shared_library(
    input_path: &Path,
    lib_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError> {
    // Phase 4: 如果启用 std C FFI，先编译 C FFI 源文件（与可执行文件相同）
    let cffi_object_path =
        if options.link_std_cffi { Some(compile_std_cffi(options)?) } else { None };

    let mut cmd = build_command("clang", options)?;
    cmd.arg(input_path);
    if let Some(ref cffi_obj) = cffi_object_path {
        cmd.arg(cffi_obj);
    }
    if options.debug_info {
        cmd.arg("-g");
    }
    cmd.arg("-o").arg(lib_path);

    // 跨平台动态库标志
    #[cfg(target_os = "windows")]
    {
        cmd.arg("-shared").arg("-Wl,/DLL").arg("-fuse-ld=lld").arg("-Wl,--export-all-symbols");
    }
    #[cfg(target_os = "macos")]
    {
        cmd.arg("-dynamiclib");
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        cmd.arg("-shared");
    }

    cmd.arg(options.opt_level.as_llvm_flag());
    run_and_report(&mut cmd, "clang")?;
    Ok(())
}

/// Tier 4: 编译期链接到 Rust 宿主
///
/// 将 AOT 目标文件与 Rust 宿主二进制链接，生成单一可执行文件。
pub fn link_to_rust_host(
    input_path: &Path,
    host_path: &Path,
    options: &AotOptions,
) -> Result<(), AotError> {
    let mut cffi_object_path = None;
    if options.link_std_cffi {
        cffi_object_path = Some(compile_std_cffi(options)?);
    }

    let mut cmd = build_command("clang", options)?;
    cmd.arg(input_path);
    if let Some(ref cffi_obj) = cffi_object_path {
        cmd.arg(cffi_obj);
    }
    cmd.arg("-o").arg(host_path).arg(options.opt_level.as_llvm_flag());
    if options.debug_info {
        cmd.arg("-g");
    }

    #[cfg(target_os = "windows")]
    {
        use super::target::OperatingSystem;
        if options.target.os == OperatingSystem::Windows {
            cmd.arg("-Wl,/subsystem:console");
        }
    }

    run_and_report(&mut cmd, "clang")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_tool_no_llvm() {
        // 在没有设置 AURA_LLVM_HOME 的情况下，find_tool 应该返回 None
        let options = AotOptions::default();
        // 这里仅检查不 panic
        let _ = find_tool("llc", &options);
    }

    #[test]
    fn test_parse_aot_symbol_name() {
        let (func_part, nargs, rettag, tags) =
            parse_aot_symbol_name("aura_aot_add!2!0!0!0").unwrap();
        assert_eq!(func_part, "add");
        assert_eq!(nargs, 2);
        assert_eq!(rettag, 0);
        assert_eq!(tags, vec![0, 0]);

        let (_, nargs, rettag, tags) = parse_aot_symbol_name("aura_aot_float_2!2!1!1!1").unwrap();
        assert_eq!(nargs, 2);
        assert_eq!(rettag, 1);
        assert_eq!(tags, vec![1, 1]);

        // 无元数据
        assert!(parse_aot_symbol_name("aura_aot_nometadata").is_none());
        assert!(parse_aot_symbol_name("other_symbol").is_none());
    }

    #[test]
    fn test_compute_arg_tags() {
        // 紧凑编码（<=2 参数）
        assert_eq!(compute_arg_tags(&[0, 0]), 0x00); // Int, Int
        assert_eq!(compute_arg_tags(&[1, 1]), 0x11); // Float, Float
        assert_eq!(compute_arg_tags(&[2, 3]), 0x23); // Bool, Unit
        assert_eq!(compute_arg_tags(&[0]), 0x00); // Int only
        assert_eq!(compute_arg_tags(&[]), 0x00); // no args

        // 扩展编码标记（>2 参数）
        assert_eq!(compute_arg_tags(&[0, 0, 0]), 3); // 3 args
        assert_eq!(compute_arg_tags(&[0, 0, 0, 0]), 4); // 4 args
    }
}
