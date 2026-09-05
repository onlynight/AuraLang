//! [Phase B2] 生命周期阶段实现
//!
//! 生命周期与任务的映射：
//! - clean → clean
//! - resolve → resolve
//! - compile → compile-main, compile-test, compile-bench
//! - test → run-tests
//! - package → package
//! - verify → verify
//! - install → install
//! - deploy → publish

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::cache::local::LocalCache;
use crate::cache::remote::CacheService;
use crate::error::LoomError;
use crate::manifest::priority::ResolvedBuildConfig;
use crate::manifest::{LoomManifest};
use crate::plugin::context::PluginContext;
use crate::plugin::PluginRegistry;
use crate::task::scheduler::{Scheduler, SchedulerConfig};
use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs, TaskGraph};

/// 生命周期阶段到任务的映射
pub fn phase_to_tasks(phase: &str) -> Vec<(&'static str, TaskKind)> {
    match phase {
        "clean" => vec![("clean", TaskKind::Clean)],
        "resolve" => vec![("resolve", TaskKind::Resolve)],
        "compile" => vec![
            ("compile-main", TaskKind::Compile("main".to_string())),
            ("compile-test", TaskKind::Compile("test".to_string())),
            ("compile-bench", TaskKind::Compile("bench".to_string())),
        ],
        "test" => vec![("run-tests", TaskKind::Test)],
        "package" => vec![("package", TaskKind::Package)],
        "verify" => vec![("verify", TaskKind::Verify)],
        "install" => vec![("install", TaskKind::Install)],
        "deploy" => vec![("publish", TaskKind::Deploy)],
        "run" => vec![("run", TaskKind::Execute)],
        "watch" => vec![("watch", TaskKind::Watch)],
        _ => Vec::new(),
    }
}

/// 构建完整生命周期任务图
///
/// 返回包含所有内置任务的标准任务图。
pub fn build_standard_task_graph(manifest: &LoomManifest, project_dir: &std::path::Path) -> TaskGraph {
    let mut graph = TaskGraph::new();

    // 1. clean
    graph.add_task(TaskDefinition {
        name: "clean".to_string(),
        description: "清理构建产物".to_string(),
        kind: TaskKind::Clean,
        depends_on: Vec::new(),
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 2. resolve
    graph.add_task(TaskDefinition {
        name: "resolve".to_string(),
        description: "解析依赖".to_string(),
        kind: TaskKind::Resolve,
        depends_on: vec!["clean".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 3. compile-main
    let main_sources = discover_source_files(project_dir, "src");
    graph.add_task(TaskDefinition {
        name: "compile-main".to_string(),
        description: "编译主源码集".to_string(),
        kind: TaskKind::Compile("main".to_string()),
        depends_on: vec!["resolve".to_string()],
        inputs: TaskInputs {
            files: main_sources.clone(),
            options: HashMap::new(),
            dep_fingerprints: HashMap::new(),
        },
        outputs: TaskOutputs::default(),
    });

    // 4. compile-test (如果存在 test 目录)
    let test_sources = discover_source_files(project_dir, "test");
    if !test_sources.is_empty() {
        graph.add_task(TaskDefinition {
            name: "compile-test".to_string(),
            description: "编译测试源码集".to_string(),
            kind: TaskKind::Compile("test".to_string()),
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs {
                files: test_sources,
                options: HashMap::new(),
                dep_fingerprints: HashMap::new(),
            },
            outputs: TaskOutputs::default(),
        });
    }

    // 5. compile-bench (如果存在 bench 目录)
    let bench_sources = discover_source_files(project_dir, "bench");
    if !bench_sources.is_empty() {
        graph.add_task(TaskDefinition {
            name: "compile-bench".to_string(),
            description: "编译基准源码集".to_string(),
            kind: TaskKind::Compile("bench".to_string()),
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs {
                files: bench_sources,
                options: HashMap::new(),
                dep_fingerprints: HashMap::new(),
            },
            outputs: TaskOutputs::default(),
        });
    }

    // 6. run-tests (如果存在 compile-test)
    if graph.contains("compile-test") {
        graph.add_task(TaskDefinition {
            name: "run-tests".to_string(),
            description: "执行测试".to_string(),
            kind: TaskKind::Test,
            depends_on: vec!["compile-test".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });
    }

    // 7. package
    graph.add_task(TaskDefinition {
        name: "package".to_string(),
        description: "打包为 .auz".to_string(),
        kind: TaskKind::Package,
        depends_on: vec!["compile-main".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 8. verify
    graph.add_task(TaskDefinition {
        name: "verify".to_string(),
        description: "验证制品完整性".to_string(),
        kind: TaskKind::Verify,
        depends_on: vec!["package".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 9. install
    graph.add_task(TaskDefinition {
        name: "install".to_string(),
        description: "安装到本地注册表".to_string(),
        kind: TaskKind::Install,
        depends_on: vec!["verify".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 10. publish
    graph.add_task(TaskDefinition {
        name: "publish".to_string(),
        description: "发布到远程仓库".to_string(),
        kind: TaskKind::Deploy,
        depends_on: vec!["install".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 11. run
    graph.add_task(TaskDefinition {
        name: "run".to_string(),
        description: "运行应用".to_string(),
        kind: TaskKind::Execute,
        depends_on: vec!["compile-main".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    // 12. watch
    graph.add_task(TaskDefinition {
        name: "watch".to_string(),
        description: "监听源码变化".to_string(),
        kind: TaskKind::Watch,
        depends_on: vec!["resolve".to_string()],
        inputs: TaskInputs::default(),
        outputs: TaskOutputs::default(),
    });

    graph
}

/// B4.5: 构建包含插件注册任务的任务图
///
/// 在标准任务图基础上，通过插件注册表加载插件，
/// 并收集插件注册的任务添加到图中。
///
/// 执行流程：
/// 1. 构建标准任务图
/// 2. 创建 PluginContext 并配置所有插件
/// 3. 将插件注册的任务添加到图中
/// 4. 验证任务图有效性
pub fn build_task_graph_with_plugins(
    manifest: &LoomManifest,
    project_dir: &std::path::Path,
    plugin_registry: &PluginRegistry,
    build_config: &ResolvedBuildConfig,
) -> Result<TaskGraph, LoomError> {
    // 1. 构建标准任务图
    let mut graph = build_standard_task_graph(manifest, project_dir);

    // 2. 创建 PluginContext 并配置所有插件
    let source_sets = discover_source_sets(manifest, project_dir);
    let mut ctx = PluginContext::new(
        manifest.clone(),
        source_sets,
        project_dir.to_path_buf(),
        build_config.clone(),
    );

    // 3. 配置所有插件（注册任务到 ctx）
    plugin_registry.configure_all(&mut ctx)?;

    // 4. 将插件注册的任务添加到图中
    let mut plugin_task_count = 0;
    for task in &ctx.tasks {
        // 检查是否已存在同名任务（避免重复）
        if !graph.contains(&task.name) {
            graph.add_task(task.clone());
            plugin_task_count += 1;
        }
    }

    if plugin_task_count > 0 {
        tracing::info!("插件注册了 {} 个任务", plugin_task_count);
    }

    // 5. 验证任务图有效性
    graph.validate()?;

    Ok(graph)
}

/// 从 manifest 发现源码集
fn discover_source_sets(manifest: &LoomManifest, project_dir: &std::path::Path) -> Vec<crate::manifest::SourceSet> {
    let mut sets = Vec::new();

    // 从 manifest 配置读取源码集
    for (name, config) in &manifest.build.source_sets {
        let source_set = crate::manifest::SourceSet::from_config(config, name, project_dir);
        sets.push(source_set);
    }

    // 如果 manifest 没有配置源码集，使用默认值
    if sets.is_empty() {
        sets.push(crate::manifest::SourceSet::main(project_dir));
        if project_dir.join("test").exists() {
            sets.push(crate::manifest::SourceSet::test(project_dir));
        }
        if project_dir.join("bench").exists() {
            sets.push(crate::manifest::SourceSet::bench(project_dir));
        }
    }

    sets
}

/// 发现源文件
fn discover_source_files(project_dir: &std::path::Path, source_dir: &str) -> Vec<std::path::PathBuf> {
    let full_dir = project_dir.join(source_dir);
    if !full_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    discover_aura_files_recursive(&full_dir, &mut files);
    files
}

fn discover_aura_files_recursive(dir: &std::path::Path, result: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                discover_aura_files_recursive(&path, result);
            } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
                result.push(path);
            }
        }
    }
}

/// 执行生命周期阶段
pub fn execute_phase(
    phase: &str,
    manifest: &LoomManifest,
    project_dir: &std::path::Path,
    build_config: Arc<ResolvedBuildConfig>,
    cache: Option<Arc<Mutex<LocalCache>>>,
    scheduler_config: SchedulerConfig,
) -> Result<(), LoomError> {
    execute_phase_with_service(
        phase,
        manifest,
        project_dir,
        build_config,
        cache,
        None,
        scheduler_config,
    )
}

/// B3.4: 执行生命周期阶段（带缓存服务）
pub fn execute_phase_with_service(
    phase: &str,
    manifest: &LoomManifest,
    project_dir: &std::path::Path,
    build_config: Arc<ResolvedBuildConfig>,
    cache: Option<Arc<Mutex<LocalCache>>>,
    cache_service: Option<Arc<Mutex<CacheService>>>,
    scheduler_config: SchedulerConfig,
) -> Result<(), LoomError> {
    // 构建任务图
    let graph = build_standard_task_graph(manifest, project_dir);

    // 确定根任务
    let root_task = match phase {
        "build" => "install",
        "compile" => "compile-main",
        "test" => if graph.contains("run-tests") { "run-tests" } else { "compile-main" },
        "clean" => "clean",
        "resolve" => "resolve",
        "package" => "package",
        "verify" => "verify",
        "install" => "install",
        "publish" => "publish",
        "run" => "run",
        "watch" => "watch",
        _ => return Err(LoomError::Task(format!("未知生命周期阶段: {}", phase))),
    };

    let scheduler = Scheduler::with_cache_service(
        Arc::new(graph),
        build_config,
        cache,
        cache_service,
        scheduler_config,
    );

    scheduler.execute(root_task)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse::default_manifest;
    use tempfile::TempDir;

    #[test]
    fn test_build_standard_task_graph() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();

        let graph = build_standard_task_graph(&manifest, tmp.path());

        // 检查基本任务存在
        assert!(graph.contains("clean"));
        assert!(graph.contains("resolve"));
        assert!(graph.contains("compile-main"));
        assert!(graph.contains("package"));
        assert!(graph.contains("verify"));
        assert!(graph.contains("install"));
        assert!(graph.contains("publish"));
        assert!(graph.contains("run"));
        assert!(graph.contains("watch"));

        // 验证任务图有效性
        assert!(graph.validate().is_ok());

        // 验证拓扑排序
        let topo = graph.topological_sort().unwrap();
        assert!(!topo.is_empty());
    }

    #[test]
    fn test_build_graph_with_sources() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();

        // 创建 src 目录和测试文件
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();
        std::fs::write(src.join("utils.aura"), "fun add() {}").unwrap();

        // 创建 test 目录
        let test = tmp.path().join("test");
        std::fs::create_dir_all(&test).unwrap();
        std::fs::write(test.join("main_test.aura"), "fun test_main() {}").unwrap();

        let graph = build_standard_task_graph(&manifest, tmp.path());

        assert!(graph.contains("compile-main"));
        assert!(graph.contains("compile-test"));
        assert!(graph.contains("run-tests"));

        // 验证 compile-main 有源文件
        let compile_main = graph.get("compile-main").unwrap();
        assert!(!compile_main.inputs.files.is_empty());
    }

    #[test]
    fn test_build_graph_without_test() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();

        // 仅创建 src 目录
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();

        let graph = build_standard_task_graph(&manifest, tmp.path());

        assert!(graph.contains("compile-main"));
        assert!(!graph.contains("compile-test"));
        assert!(!graph.contains("run-tests"));
    }

    #[test]
    fn test_phase_to_tasks() {
        let clean = phase_to_tasks("clean");
        assert_eq!(clean.len(), 1);
        assert_eq!(clean[0].0, "clean");

        let compile = phase_to_tasks("compile");
        assert_eq!(compile.len(), 3);

        let test = phase_to_tasks("test");
        assert_eq!(test.len(), 1);
    }

    #[test]
    fn test_discover_source_files() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();
        std::fs::write(src.join("utils.aura"), "fun add() {}").unwrap();

        let files = discover_source_files(tmp.path(), "src");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_discover_source_files_empty() {
        let tmp = TempDir::new().unwrap();
        let files = discover_source_files(tmp.path(), "src");
        assert!(files.is_empty());
    }

    #[test]
    fn test_discover_source_files_recursive() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let math = src.join("math");
        std::fs::create_dir_all(&math).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();
        std::fs::write(math.join("vector.aura"), "fun dot() {}").unwrap();

        let files = discover_source_files(tmp.path(), "src");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_execute_phase_unknown() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();
        let config = Arc::new(ResolvedBuildConfig::default());
        let scheduler_config = SchedulerConfig { dry_run: true, ..Default::default() };

        let result = execute_phase("unknown", &manifest, tmp.path(), config, None, scheduler_config);
        assert!(result.is_err());
    }

    #[test]
    fn test_execute_phase_clean() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();
        let config = Arc::new(ResolvedBuildConfig::default());
        let scheduler_config = SchedulerConfig { dry_run: true, ..Default::default() };

        let result = execute_phase("clean", &manifest, tmp.path(), config, None, scheduler_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_execute_phase_build_dry_run() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();

        // 创建 src
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();

        let config = Arc::new(ResolvedBuildConfig::default());
        let scheduler_config = SchedulerConfig { dry_run: true, ..Default::default() };

        let result = execute_phase("build", &manifest, tmp.path(), config, None, scheduler_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_standard_lifecycle_order() {
        let manifest = default_manifest("test");
        let tmp = TempDir::new().unwrap();

        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();

        let graph = build_standard_task_graph(&manifest, tmp.path());
        let topo = graph.topological_sort().unwrap();

        // clean should be first
        assert_eq!(topo.order[0], "clean");
        // install should be last (for build lifecycle)
        let install_pos = topo.order.iter().position(|n| n == "install").unwrap();
        let publish_pos = topo.order.iter().position(|n| n == "publish").unwrap();
        assert!(install_pos < publish_pos);
    }
}
