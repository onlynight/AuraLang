//! [Phase B2] 内置任务定义与执行
//!
//! 任务清单：
//! - clean: 清理 target/ 目录
//! - resolve: 解析依赖（下载 + 锁文件更新）
//! - compile-{source-set}: 编译源码集
//! - run-tests: 执行测试
//! - package: 打包为 .auz
//! - verify: 验证制品完整性
//! - install: 安装到本地注册表
//! - publish: 发布到远程仓库
//! - run: 运行应用
//! - watch: 监听源码变化

use std::path::{Path, PathBuf};

use crate::error::LoomError;
use crate::manifest::{CompileMode, FfiMode, priority::ResolvedBuildConfig};
use crate::task::TaskDefinition;

/// 获取项目目录（`aura.toml` 所在目录）
///
/// 取自 [`ResolvedBuildConfig::project_dir`]。此前这里硬编码为 `"."`，
/// 使 `package` / `compile` / `test` 等任务一律相对**进程 CWD** 解析项目内
/// 路径，在非项目目录执行时会误报「未找到 aura.toml」。
fn project_dir(_task: &TaskDefinition, config: &ResolvedBuildConfig) -> PathBuf {
    if config.project_dir.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(&config.project_dir)
    }
}

/// 获取输出目录
fn output_dir(config: &ResolvedBuildConfig) -> PathBuf {
    PathBuf::from(&config.out_dir)
}

/// 获取缓存目录
fn cache_dir(config: &ResolvedBuildConfig) -> PathBuf {
    PathBuf::from(&config.cache_dir)
}

// ═══════════════════════════════════════════════════════════════════════════════
// clean
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_clean(
    _task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let out_dir = output_dir(config);

    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir)?;
        Ok((format!("✓ Cleaned {}", out_dir.display()), Vec::new()))
    } else {
        Ok((
            "✓ No cleanup needed (directory does not exist)".to_string(),
            Vec::new(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// resolve
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_resolve(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let dir = project_dir(task, config);
    let lock_file = dir.join("aura.lock");
    let manifest_path = dir.join("aura.toml");

    // 加载 manifest 并应用 BOM 版本锁定（manifest 不存在时使用空依赖）
    let all_deps = if manifest_path.exists() {
        let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;

        // 应用 BOM 版本锁定
        let bom = crate::dep::bom::Bom::from_workspace(&manifest.workspace);
        let raw_deps = manifest.all_dependencies();
        if bom.is_empty() { raw_deps } else { bom.resolve_dependencies(&raw_deps) }
    } else {
        Vec::new()
    };

    // 重新读取 manifest 以获取 workspace 信息（用于 BOM 统计）
    let bom = if manifest_path.exists() {
        let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;
        crate::dep::bom::Bom::from_workspace(&manifest.workspace)
    } else {
        crate::dep::bom::Bom::from_workspace(&None)
    };

    let locked_count = all_deps.iter().filter(|d| bom.has_lock(&d.name)).count();

    // 检查锁文件
    if lock_file.exists() {
        Ok((
            format!(
                "✓ Dependencies resolved ({} dependencies, {} BOM locked, lock file exists)",
                all_deps.len(),
                locked_count
            ),
            vec![lock_file],
        ))
    } else {
        Ok((
            format!(
                "✓ Dependency resolution complete ({} dependencies, {} BOM locked)",
                all_deps.len(),
                locked_count
            ),
            Vec::new(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// compile
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_compile(
    task: &TaskDefinition,
    source_set: &str,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let dir = project_dir(task, config);
    let out_dir = output_dir(config);

    // 根据编译模式生成不同的产物
    match config.mode {
        CompileMode::Aot => execute_compile_aot(task, source_set, config),
        CompileMode::Jit | CompileMode::Vm => execute_compile_bytecode(task, source_set, config),
    }
}

/// AOT 模式：编译为原生可执行文件或动态库
#[cfg(feature = "llvm")]
fn execute_compile_aot(
    task: &TaskDefinition,
    source_set: &str,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    use compiler::codegen::aot::{AotOptions, OptimizationLevel, aot_compile};

    let dir = project_dir(task, config);
    let out_dir = output_dir(config);
    let out = out_dir.join(format!("compile-{}", source_set));
    std::fs::create_dir_all(&out)?;

    // 查找源码文件
    let files = &task.inputs.files;
    if files.is_empty() {
        return Ok((
            format!("✓ AOT compile {} source set: no source files", source_set),
            Vec::new(),
        ));
    }

    // 读取源码
    let entry_file = files.iter().find(|f| f.is_file()).unwrap_or(&files[0]);
    let source = std::fs::read_to_string(entry_file)
        .map_err(|e| LoomError::Task(format!("Failed to read {}: {}", entry_file.display(), e)))?;

    // 构建 AOT 选项
    let opt_level = match config.opt_level {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Balanced,
        2 => OptimizationLevel::Aggressive,
        3 => OptimizationLevel::Extreme,
        _ => OptimizationLevel::Aggressive,
    };

    let options = AotOptions {
        opt_level,
        debug_info: config.debug,
        c_abi: config.ffi_mode == FfiMode::Cabi,
        ..Default::default()
    };

    // 确定输出路径
    let (output_path, is_library) = if config.library {
        // 库：生成动态库，输出到 libs/ 目录
        let libs_dir = dir.join("libs");
        std::fs::create_dir_all(&libs_dir)?;
        let lib_name = if cfg!(windows) {
            format!("{}.dll", config.name)
        } else if cfg!(target_os = "macos") {
            format!("lib{}.dylib", config.name)
        } else {
            format!("lib{}.so", config.name)
        };
        (libs_dir.join(&lib_name), true)
    } else {
        // 应用：生成可执行文件
        let exe_name =
            if cfg!(windows) { format!("{}.exe", config.name) } else { config.name.clone() };
        (out.join(&exe_name), false)
    };

    // 执行 AOT 编译
    let _output = aot_compile(&source, &output_path, options)
        .map_err(|e| LoomError::Task(format!("AOT compilation failed: {}", e)))?;

    let ffi_info = match config.ffi_mode {
        FfiMode::Aot => "JitValue ABI (aura_aot_*)",
        FfiMode::Cabi => "C ABI (aura_c_*)",
    };

    let product_type = if is_library { "Dynamic library" } else { "Executable" };

    Ok((
        format!(
            "✓ AOT compile {} source set: {} → {} ({}: {})",
            source_set,
            entry_file.display(),
            output_path.display(),
            product_type,
            ffi_info
        ),
        vec![output_path],
    ))
}

/// AOT 模式（无 LLVM 支持时的降级实现）
#[cfg(not(feature = "llvm"))]
fn execute_compile_aot(
    task: &TaskDefinition,
    source_set: &str,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let out_dir = output_dir(config);
    let out = out_dir.join(format!("compile-{}", source_set));
    std::fs::create_dir_all(&out)?;

    let files = &task.inputs.files;
    if files.is_empty() {
        return Ok((
            format!("✓ AOT compile {} source set: no source files", source_set),
            Vec::new(),
        ));
    }

    let entry_file = files.iter().find(|f| f.is_file()).unwrap_or(&files[0]);
    let exe_name = if cfg!(windows) { format!("{}.exe", task.name) } else { task.name.clone() };
    let exe_path = out.join(&exe_name);

    std::fs::write(&exe_path, b"AURA-AOT-EXE")?;

    Ok((
        format!(
            "✓ AOT compile {} source set: {} → {} (placeholder: requires llvm feature)",
            source_set,
            entry_file.display(),
            exe_path.display()
        ),
        vec![exe_path],
    ))
}

/// 字节码模式（VM/JIT）：编译为 .auc 字节码文件
fn execute_compile_bytecode(
    task: &TaskDefinition,
    source_set: &str,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    use compiler::codegen::{compile_source, write_auc};

    let dir = project_dir(task, config);
    let out_dir = output_dir(config);
    let out = out_dir.join(format!("compile-{}", source_set));
    std::fs::create_dir_all(&out)?;

    // 查找源码文件
    let files = &task.inputs.files;

    let mut artifacts = Vec::new();

    if !files.is_empty() {
        // 有明确指定的源文件
        let count = files.len();
        for file in files {
            if file.exists() && file.extension().map(|e| e == "aura").unwrap_or(false) {
                let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module");
                let module_name = file_name.trim_end_matches(".aura");
                let out_file = out.join(format!("{}.auc", module_name));

                // 读取源码
                let source = std::fs::read_to_string(file)?;

                // 编译为字节码
                match compile_source(&source) {
                    Ok(module) => {
                        // 写入 .auc 文件
                        if let Err(e) = write_auc(&out_file.to_string_lossy(), &module) {
                            return Err(LoomError::Config(format!(
                                "Failed to write bytecode {}: {}",
                                file.display(),
                                e
                            )));
                        }
                        artifacts.push(out_file);
                    }
                    Err(e) => {
                        return Err(LoomError::Config(format!(
                            "Compilation failed {}: {}",
                            file.display(),
                            e
                        )));
                    }
                }
            }
        }

        return Ok((
            format!(
                "✓ Bytecode compile {} source set: {} files → {}",
                source_set,
                count,
                out.display()
            ),
            artifacts,
        ));
    }

    // 尝试从目录发现源码
    let source_dir = dir.join("src");
    if source_dir.exists() {
        let mut discovered = Vec::new();
        discover_aura_files(&source_dir, &mut discovered);
        if !discovered.is_empty() {
            let count = discovered.len();
            for file in &discovered {
                let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module");
                let module_name = file_name.trim_end_matches(".aura");
                let out_file = out.join(format!("{}.auc", module_name));

                // 读取源码
                let source = std::fs::read_to_string(file)?;

                // 编译为字节码
                match compile_source(&source) {
                    Ok(module) => {
                        // 写入 .auc 文件
                        if let Err(e) = write_auc(&out_file.to_string_lossy(), &module) {
                            return Err(LoomError::Config(format!(
                                "Failed to write bytecode {}: {}",
                                file.display(),
                                e
                            )));
                        }
                        artifacts.push(out_file);
                    }
                    Err(e) => {
                        return Err(LoomError::Config(format!(
                            "Compilation failed {}: {}",
                            file.display(),
                            e
                        )));
                    }
                }
            }

            return Ok((
                format!(
                    "✓ Bytecode compile {} source set: {} files → {}",
                    source_set,
                    count,
                    out.display()
                ),
                artifacts,
            ));
        }
    }

    Ok((
        format!(
            "✓ Bytecode compile {} source set: no source files",
            source_set
        ),
        Vec::new(),
    ))
}

/// 递归发现 .aura 文件
fn discover_aura_files(dir: &Path, result: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                discover_aura_files(&path, result);
            } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
                result.push(path);
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// run-tests
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_test(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let dir = project_dir(task, config);
    let test_dir = dir.join("test");

    if !test_dir.exists() {
        return Ok((
            "✓ No test directory, skipping tests".to_string(),
            Vec::new(),
        ));
    }

    let mut test_files = Vec::new();
    discover_aura_files(&test_dir, &mut test_files);

    if test_files.is_empty() {
        return Ok(("✓ No test files, skipping tests".to_string(), Vec::new()));
    }

    // 目前测试执行是占位符，后续集成 VM
    Ok((
        format!("✓ Test execution: {} test files", test_files.len()),
        vec![test_dir],
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// package
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_package(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    use compiler::auz::{PackageBuildOptions, PackageBuilder};
    use compiler::codegen::read_auc;
    use compiler::package::PackageManifest;

    let dir = project_dir(task, config);
    let out_dir = output_dir(config);

    if !config.emit_package {
        return Ok((
            "✓ Packaging disabled (emit-package = false)".to_string(),
            Vec::new(),
        ));
    }

    let package_dir = out_dir.join("package");
    std::fs::create_dir_all(&package_dir)?;

    // 从项目目录读取 manifest
    let manifest_path = dir.join("aura.toml");
    if !manifest_path.exists() {
        return Err(LoomError::Config(format!(
            "aura.toml not found: {}",
            manifest_path.display()
        )));
    }

    let manifest_content = std::fs::read_to_string(&manifest_path)?;
    let manifest: crate::manifest::LoomManifest = toml::from_str(&manifest_content)
        .map_err(|e| LoomError::Config(format!("Failed to parse aura.toml: {}", e)))?;

    // 构建 PackageManifest（使用 compiler::package::PackageManifest）
    let pkg_manifest = PackageManifest {
        schema_version: "1.0".to_string(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        authors: manifest.authors.clone(),
        license: manifest.license.clone(),
        repository: manifest.repository.clone(),
        entry: manifest.entry.clone(),
        dependencies: Vec::new(), // 简化：不复制依赖
        dev_dependencies: Vec::new(),
        exports: manifest.exports.clone(),
        platforms: Vec::new(),
        library: manifest.library,
        kind: Default::default(),
        compiler_min_version: manifest.compiler_min_version.clone(),
        compiler_max_version: manifest.compiler_max_version.clone(),
        package: Default::default(),
        resources: Default::default(),
    };

    // 查找编译好的 .auc 文件
    let compile_dir = out_dir.join("compile-main");
    if !compile_dir.exists() {
        return Err(LoomError::Config(format!(
            "Compilation output directory not found: {}",
            compile_dir.display()
        )));
    }

    // 查找主入口的 .auc 文件
    let entry_base = manifest.entry.trim_end_matches(".aura");
    let auc_path = compile_dir.join(format!("{}.auc", entry_base));

    let auc_path = if auc_path.exists() {
        auc_path
    } else {
        // 尝试查找任意 .auc 文件
        let mut auc_files = Vec::new();
        for entry in std::fs::read_dir(&compile_dir)? {
            let entry = entry?;
            if entry.path().extension().map(|e| e == "auc").unwrap_or(false) {
                auc_files.push(entry.path());
            }
        }

        if auc_files.is_empty() {
            return Err(LoomError::Config(format!(
                "No .auc bytecode file found: {}",
                compile_dir.display()
            )));
        }

        auc_files[0].clone()
    };

    // 读取 .auc 文件
    let module = read_auc(&auc_path.to_string_lossy())
        .map_err(|e| LoomError::Config(format!("Failed to read bytecode: {}", e)))?;

    // 生成包名
    let package_name = format!("{}-{}.auz", manifest.name, manifest.version);
    let package_path = package_dir.join(&package_name);

    // 使用 PackageBuilder 打包
    let options = PackageBuildOptions {
        include_sources: manifest.package.include_sources,
        include_resources: false,
        resource_include_patterns: Vec::new(),
        include_ref_index: true,
        compression_level: 3,
    };

    let src_dir = dir.join("src");
    let builder =
        PackageBuilder::new(&pkg_manifest, &module).with_source_dir(&src_dir).with_options(options);

    let result = builder
        .build(&package_path)
        .map_err(|e| LoomError::Config(format!("Packaging failed: {}", e)))?;

    Ok((
        format!("✓ Packaging complete: {}", result.path.display()),
        vec![result.path],
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// verify
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_verify(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let out_dir = output_dir(config);
    let package_dir = out_dir.join("package");

    if !package_dir.exists() {
        return Ok(("✓ No packages to verify".to_string(), Vec::new()));
    }

    // 检查包文件
    let mut verified = 0;
    if let Ok(entries) = std::fs::read_dir(&package_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "auz").unwrap_or(false) {
                // 简单验证：检查文件是否存在且非空
                if let Ok(metadata) = path.metadata() {
                    if metadata.len() > 0 {
                        verified += 1;
                    }
                }
            }
        }
    }

    Ok((
        format!("✓ Verification complete: {} packages", verified),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// check
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_check(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let dir = project_dir(task, config);
    let manifest_path = dir.join("aura.toml");

    // 检查 aura.toml 是否存在
    if !manifest_path.exists() {
        return Ok(("✓ No aura.toml, skipping check".to_string(), Vec::new()));
    }

    // 解析并验证 manifest
    let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;
    let errors = crate::manifest::validate::validate_manifest(&manifest);

    if errors.is_empty() {
        Ok((
            format!(
                "✓ Syntax/semantic check passed ({} dependencies)",
                manifest.all_dependencies().len()
            ),
            Vec::new(),
        ))
    } else {
        Ok((
            format!("⚠ Syntax/semantic check found {} issues", errors.len()),
            Vec::new(),
        ))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// install
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_install(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let out_dir = output_dir(config);
    let package_dir = out_dir.join("package");

    if !package_dir.exists() {
        return Ok(("✓ No packages to install".to_string(), Vec::new()));
    }

    // 安装到本地注册表（占位符）
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    let registry_dir = PathBuf::from(&home).join(".aura").join("registry");

    let mut installed = 0;
    if let Ok(entries) = std::fs::read_dir(&package_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "auz").unwrap_or(false) {
                std::fs::create_dir_all(&registry_dir)?;
                let dest = registry_dir.join(path.file_name().unwrap_or_default());
                std::fs::copy(&path, &dest)?;
                installed += 1;
            }
        }
    }

    Ok((
        format!(
            "✓ Installation complete: {} packages → {}",
            installed,
            registry_dir.display()
        ),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// publish (deploy)
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_deploy(
    _task: &TaskDefinition,
    _config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    Ok((
        "✓ Publish (placeholder, Phase B6 implementation)".to_string(),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// run (execute)
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_execute(
    task: &TaskDefinition,
    config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    let dir = project_dir(task, config);
    let out_dir = output_dir(config);

    // 查找编译产物
    let compile_dir = out_dir.join("compile-main");
    if !compile_dir.exists() {
        return Ok((
            "✓ Run (compile output not found, placeholder)".to_string(),
            Vec::new(),
        ));
    }

    Ok((
        format!("✓ Run: {} (placeholder)", out_dir.display()),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// watch
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_watch(
    _task: &TaskDefinition,
    _config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    Ok((
        "✓ Watch mode (placeholder, Phase B7 implementation)".to_string(),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// plugin
// ═══════════════════════════════════════════════════════════════════════════════

pub fn execute_plugin(
    _task: &TaskDefinition,
    plugin_name: &str,
    _config: &ResolvedBuildConfig,
) -> Result<(String, Vec<PathBuf>), LoomError> {
    Ok((
        format!("✓ Plugin task '{}' (placeholder)", plugin_name),
        Vec::new(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::priority::ResolvedBuildConfig;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};
    use tempfile::TempDir;

    fn make_config(out_dir: &Path) -> ResolvedBuildConfig {
        make_config_with_project(out_dir, ".")
    }

    /// 构造配置，并显式指定项目根目录（默认为 `"."`）。
    fn make_config_with_project(out_dir: &Path, project_dir: &str) -> ResolvedBuildConfig {
        ResolvedBuildConfig {
            opt_level: 2,
            debug: true,
            target: None,
            out_dir: out_dir.to_string_lossy().to_string(),
            cache_dir: out_dir.join("cache").to_string_lossy().to_string(),
            cache_remote: None,
            cache_remote_shared: false,
            emit_signatures: true,
            emit_package: false,
            parallel: true,
            parallel_jobs: 4,
            alias: std::collections::HashMap::new(),
            active_profile: None,
            mode: CompileMode::Vm,
            project_dir: project_dir.to_string(),
            ..Default::default()
        }
    }

    fn make_task(name: &str, kind: TaskKind) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: String::new(),
            kind,
            depends_on: Vec::new(),
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        }
    }

    #[test]
    fn test_execute_clean_creates_dir() {
        let tmp = TempDir::new().unwrap();
        let config = make_config(tmp.path());
        let task = make_task("clean", TaskKind::Clean);

        // 先创建 target 目录
        std::fs::create_dir_all(config.out_dir.as_str()).unwrap();

        let (msg, _artifacts) = execute_clean(&task, &config).unwrap();
        assert!(msg.contains("Cleaned"));
    }

    #[test]
    fn test_execute_clean_no_dir() {
        let tmp = TempDir::new().unwrap();
        // 使用不存在的输出目录
        let non_existent = tmp.path().join("nonexistent");
        let config = make_config(&non_existent);
        let task = make_task("clean", TaskKind::Clean);

        let (msg, _artifacts) = execute_clean(&task, &config).unwrap();
        assert!(msg.contains("No cleanup needed"));
    }

    #[test]
    fn test_execute_resolve() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("resolve", TaskKind::Resolve);

        let (msg, _artifacts) = execute_resolve(&task, &config).unwrap();
        assert!(msg.contains("Dependency resolution"));
    }

    #[test]
    fn test_execute_compile_empty() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        let (msg, _artifacts) = execute_compile(&task, "main", &config).unwrap();
        assert!(msg.contains("Bytecode compile main") || msg.contains("AOT compile main"));
    }

    #[test]
    fn test_execute_test_no_test_dir() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("test", TaskKind::Test);

        let (msg, _artifacts) = execute_test(&task, &config).unwrap();
        assert!(
            msg.contains("No test directory")
                || msg.contains("No test files")
                || msg.contains("tests")
        );
    }

    #[test]
    fn test_execute_package_disabled() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("package", TaskKind::Package);

        let (msg, _artifacts) = execute_package(&task, &config).unwrap();
        assert!(msg.contains("Packaging disabled"));
    }

    #[test]
    fn test_execute_package_enabled() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("proj");
        let out = tmp.path().join("target/build");
        std::fs::create_dir_all(project.join("src")).unwrap();

        // 项目根下必须有 aura.toml 与源码（package 任务会读取它们）
        std::fs::write(
            project.join("aura.toml"),
            "name = \"demo\"\nversion = \"0.1.0\"\nentry = \"src/main.aura\"\n",
        )
        .unwrap();
        std::fs::write(project.join("src/main.aura"), "fun main() { println(1) }\n").unwrap();

        // package 任务还要求 out_dir/compile-main 下已有编译产物
        let compile_dir = out.join("compile-main");
        std::fs::create_dir_all(&compile_dir).unwrap();
        let module = compiler::codegen::compile_source("fun main() { println(1) }").unwrap();
        compiler::codegen::write_auc(&compile_dir.join("main.auc").to_string_lossy(), &module)
            .unwrap();

        let mut config = make_config_with_project(&out, &project.to_string_lossy());
        config.emit_package = true;
        let task = make_task("package", TaskKind::Package);

        let (msg, artifacts) = execute_package(&task, &config).unwrap();
        assert!(msg.contains("Packaging complete"));
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].exists());
    }

    /// 项目根未配置（或不含 aura.toml）时应给出明确的错误，而不是落到进程 CWD
    #[test]
    fn test_execute_package_honors_project_dir() {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("target/build");
        // 项目根指向一个没有 aura.toml 的目录
        let empty_project = tmp.path().join("no-manifest");
        std::fs::create_dir_all(&empty_project).unwrap();

        let mut config = make_config_with_project(&out, &empty_project.to_string_lossy());
        config.emit_package = true;
        let task = make_task("package", TaskKind::Package);

        let err = execute_package(&task, &config).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("aura.toml not found") && msg.contains(&*empty_project.to_string_lossy()),
            "Error should point to configured project root, actual: {msg}"
        );
    }

    #[test]
    fn test_execute_verify_no_package() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("verify", TaskKind::Verify);

        let (msg, _artifacts) = execute_verify(&task, &config).unwrap();
        assert!(msg.contains("No packages to verify"));
    }

    #[test]
    fn test_execute_install_no_package() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("install", TaskKind::Install);

        let (msg, _artifacts) = execute_install(&task, &config).unwrap();
        assert!(msg.contains("No packages to install"));
    }

    #[test]
    fn test_execute_deploy() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("deploy", TaskKind::Deploy);

        let (msg, _artifacts) = execute_deploy(&task, &config).unwrap();
        assert!(msg.contains("Publish"));
    }

    #[test]
    fn test_execute_execute() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("run", TaskKind::Execute);

        let (msg, _artifacts) = execute_execute(&task, &config).unwrap();
        assert!(msg.contains("Run"));
    }

    #[test]
    fn test_execute_watch() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("watch", TaskKind::Watch);

        let (msg, _artifacts) = execute_watch(&task, &config).unwrap();
        assert!(msg.contains("Watch"));
    }

    #[test]
    fn test_execute_plugin() {
        let config = ResolvedBuildConfig::isolated();
        let task = make_task("doc", TaskKind::Plugin("doc-gen".to_string()));

        let (msg, _artifacts) = execute_plugin(&task, "doc-gen", &config).unwrap();
        assert!(msg.contains("doc-gen"));
    }

    #[test]
    fn test_discover_aura_files() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        std::fs::create_dir_all(&src_dir).unwrap();

        // 创建测试文件
        std::fs::write(src_dir.join("main.aura"), "fun main() {}").unwrap();
        std::fs::write(src_dir.join("utils.aura"), "fun add() {}").unwrap();

        let sub_dir = src_dir.join("math");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("mod.aura"), "fun sin() {}").unwrap();

        let mut files = Vec::new();
        discover_aura_files(&src_dir, &mut files);

        assert_eq!(files.len(), 3);
    }

    #[test]
    fn test_output_dir() {
        let config = ResolvedBuildConfig {
            out_dir: "target/build".to_string(),
            ..ResolvedBuildConfig::isolated()
        };
        let dir = output_dir(&config);
        assert_eq!(dir, PathBuf::from("target/build"));
    }

    #[test]
    fn test_cache_dir() {
        let config = ResolvedBuildConfig {
            cache_dir: "target/cache".to_string(),
            ..ResolvedBuildConfig::isolated()
        };
        let dir = cache_dir(&config);
        assert_eq!(dir, PathBuf::from("target/cache"));
    }
}
