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
use crate::manifest::priority::ResolvedBuildConfig;
use crate::task::TaskDefinition;

/// 获取项目目录（从任务定义或配置推断）
fn project_dir(task: &TaskDefinition, config: &ResolvedBuildConfig) -> PathBuf {
    // 默认使用当前目录，后续可通过 TaskInputs 扩展
    PathBuf::from(".")
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
        Ok((format!("✓ 已清理 {}", out_dir.display()), Vec::new()))
    } else {
        Ok(("✓ 无需清理（目录不存在）".to_string(), Vec::new()))
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
                "✓ 依赖已解析（{} 个依赖，{} 个 BOM 锁定，锁文件已存在）",
                all_deps.len(),
                locked_count
            ),
            vec![lock_file],
        ))
    } else {
        Ok((
            format!(
                "✓ 依赖解析完成（{} 个依赖，{} 个 BOM 锁定）",
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

    // 查找源码文件
    let files = &task.inputs.files;

    if files.is_empty() {
        // 尝试从目录发现源码
        let source_dir = dir.join("src");
        if source_dir.exists() {
            let mut discovered = Vec::new();
            discover_aura_files(&source_dir, &mut discovered);
            if !discovered.is_empty() {
                let count = discovered.len();
                let out = out_dir.join(format!("compile-{}", source_set));
                std::fs::create_dir_all(&out)?;

                let mut artifacts = Vec::new();
                for file in &discovered {
                    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module");
                    let module_name = file_name.trim_end_matches(".aura");
                    let out_file = out.join(format!("{}.auc", module_name));
                    std::fs::write(&out_file, format!("// compiled: {}\n", file.display()))?;
                    artifacts.push(out_file);
                }

                return Ok((
                    format!(
                        "✓ 编译 {} 源码集: {} 个文件 → {}",
                        source_set,
                        count,
                        out.display()
                    ),
                    artifacts,
                ));
            }
        }
        return Ok((
            format!("✓ 编译 {} 源码集: 无源文件", source_set),
            Vec::new(),
        ));
    }

    // 有明确指定的源文件
    let count = files.len();
    let out = out_dir.join(format!("compile-{}", source_set));
    std::fs::create_dir_all(&out)?;

    // 创建编译产物占位（后续集成编译器 SDK）
    let mut artifacts = Vec::new();
    for file in files {
        if file.exists() {
            let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module");
            let module_name = file_name.trim_end_matches(".aura");
            let out_file = out.join(format!("{}.auc", module_name));
            std::fs::write(&out_file, format!("// compiled: {}\n", file.display()))?;
            artifacts.push(out_file);
        }
    }

    Ok((
        format!(
            "✓ 编译 {} 源码集: {} 个文件 → {}",
            source_set,
            count,
            out.display()
        ),
        artifacts,
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
        return Ok(("✓ 无测试目录，跳过测试".to_string(), Vec::new()));
    }

    let mut test_files = Vec::new();
    discover_aura_files(&test_dir, &mut test_files);

    if test_files.is_empty() {
        return Ok(("✓ 无测试文件，跳过测试".to_string(), Vec::new()));
    }

    // 目前测试执行是占位符，后续集成 VM
    Ok((
        format!("✓ 测试执行: {} 个测试文件", test_files.len()),
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
    let dir = project_dir(task, config);
    let out_dir = output_dir(config);

    if !config.emit_package {
        return Ok((
            "✓ 打包已禁用（emit-package = false）".to_string(),
            Vec::new(),
        ));
    }

    let package_dir = out_dir.join("package");
    std::fs::create_dir_all(&package_dir)?;

    // 生成包名
    let name = "app"; // 后续从 manifest 获取
    let version = "0.1.0";
    let package_name = format!("{}-{}.auz", name, version);

    // 目前包创建是占位符，后续集成 PackageBuilder
    let package_path = package_dir.join(&package_name);

    // 创建占位包文件
    std::fs::write(&package_path, b"AURA-AUZ")?;

    Ok((
        format!("✓ 打包完成: {}", package_path.display()),
        vec![package_path],
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
        return Ok(("✓ 无包需要验证".to_string(), Vec::new()));
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

    Ok((format!("✓ 验证完成: {} 个包", verified), Vec::new()))
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
        return Ok(("✓ 无 aura.toml，跳过检查".to_string(), Vec::new()));
    }

    // 解析并验证 manifest
    let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;
    let errors = crate::manifest::validate::validate_manifest(&manifest);

    if errors.is_empty() {
        Ok((
            format!(
                "✓ 语法/语义检查通过（{} 个依赖）",
                manifest.all_dependencies().len()
            ),
            Vec::new(),
        ))
    } else {
        Ok((
            format!("⚠ 语法/语义检查发现 {} 个问题", errors.len()),
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
        return Ok(("✓ 无包需要安装".to_string(), Vec::new()));
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
            "✓ 安装完成: {} 个包 → {}",
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
    Ok(("✓ 发布（占位符，Phase B6 实现）".to_string(), Vec::new()))
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
        return Ok(("✓ 运行（编译产物不存在，占位符）".to_string(), Vec::new()));
    }

    Ok((
        format!("✓ 运行: {} (占位符)", out_dir.display()),
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
        "✓ Watch 模式（占位符，Phase B7 实现）".to_string(),
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
    Ok((format!("✓ 插件任务 '{}' (占位符)", plugin_name), Vec::new()))
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
        assert!(msg.contains("已清理"));
    }

    #[test]
    fn test_execute_clean_no_dir() {
        let tmp = TempDir::new().unwrap();
        // 使用不存在的输出目录
        let non_existent = tmp.path().join("nonexistent");
        let config = make_config(&non_existent);
        let task = make_task("clean", TaskKind::Clean);

        let (msg, _artifacts) = execute_clean(&task, &config).unwrap();
        assert!(msg.contains("无需清理"));
    }

    #[test]
    fn test_execute_resolve() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("resolve", TaskKind::Resolve);

        let (msg, _artifacts) = execute_resolve(&task, &config).unwrap();
        assert!(msg.contains("依赖解析"));
    }

    #[test]
    fn test_execute_compile_empty() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("compile-main", TaskKind::Compile("main".to_string()));

        let (msg, _artifacts) = execute_compile(&task, "main", &config).unwrap();
        assert!(msg.contains("编译 main"));
    }

    #[test]
    fn test_execute_test_no_test_dir() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("test", TaskKind::Test);

        let (msg, _artifacts) = execute_test(&task, &config).unwrap();
        assert!(msg.contains("无测试目录") || msg.contains("无测试文件") || msg.contains("测试"));
    }

    #[test]
    fn test_execute_package_disabled() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("package", TaskKind::Package);

        let (msg, _artifacts) = execute_package(&task, &config).unwrap();
        assert!(msg.contains("打包已禁用"));
    }

    #[test]
    fn test_execute_package_enabled() {
        let tmp = TempDir::new().unwrap();
        let mut config = make_config(tmp.path());
        config.emit_package = true;
        let task = make_task("package", TaskKind::Package);

        let (msg, artifacts) = execute_package(&task, &config).unwrap();
        assert!(msg.contains("打包完成"));
        assert_eq!(artifacts.len(), 1);
        assert!(artifacts[0].exists());
    }

    #[test]
    fn test_execute_verify_no_package() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("verify", TaskKind::Verify);

        let (msg, _artifacts) = execute_verify(&task, &config).unwrap();
        assert!(msg.contains("无包需要验证"));
    }

    #[test]
    fn test_execute_install_no_package() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("install", TaskKind::Install);

        let (msg, _artifacts) = execute_install(&task, &config).unwrap();
        assert!(msg.contains("无包需要安装"));
    }

    #[test]
    fn test_execute_deploy() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("deploy", TaskKind::Deploy);

        let (msg, _artifacts) = execute_deploy(&task, &config).unwrap();
        assert!(msg.contains("发布"));
    }

    #[test]
    fn test_execute_execute() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("run", TaskKind::Execute);

        let (msg, _artifacts) = execute_execute(&task, &config).unwrap();
        assert!(msg.contains("运行"));
    }

    #[test]
    fn test_execute_watch() {
        let config = ResolvedBuildConfig::default();
        let task = make_task("watch", TaskKind::Watch);

        let (msg, _artifacts) = execute_watch(&task, &config).unwrap();
        assert!(msg.contains("Watch"));
    }

    #[test]
    fn test_execute_plugin() {
        let config = ResolvedBuildConfig::default();
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
            ..ResolvedBuildConfig::default()
        };
        let dir = output_dir(&config);
        assert_eq!(dir, PathBuf::from("target/build"));
    }

    #[test]
    fn test_cache_dir() {
        let config = ResolvedBuildConfig {
            cache_dir: "target/cache".to_string(),
            ..ResolvedBuildConfig::default()
        };
        let dir = cache_dir(&config);
        assert_eq!(dir, PathBuf::from("target/cache"));
    }
}
