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
            "{} 执行失败 (exit code {:?}): {}",
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
            "找不到 LLVM 工具 '{}'，请设置 AURA_LLVM_HOME 或将 LLVM bin 加入 PATH",
            tool
        ))
    })?;

    let mut cmd = Command::new(tool_path);

    // 目标三元组：llc 使用 `-mtriple`，clang 使用 `--target`（或 `-target`）
    if tool == "llc" {
        cmd.arg("-mtriple").arg(options.target.to_string());
    } else {
        cmd.arg("--target").arg(options.target.to_string());
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
                    "找不到 clang 或 lld-link，请设置 AURA_LLVM_HOME 或将 LLVM bin 加入 PATH"
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
    let clang_path = find_tool("clang", options)
        .ok_or_else(|| AotError::ToolError("找不到 clang，无法编译 std C FFI".to_string()))?;

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
    let output =
        cmd.output().map_err(|e| AotError::ToolError(format!("无法启动 {}: {}", tool_name, e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        return Err(AotError::LinkerFailed(format!(
            "{} 失败:\nstdout: {}\nstderr: {}",
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
}
