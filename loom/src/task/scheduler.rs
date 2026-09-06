//! [Phase B2] 任务调度器：并行调度 + 执行层管理
//!
//! 调度策略：
//! 1. 计算执行层（同一层的任务无依赖关系，可并行执行）
//! 2. 按层顺序执行，每层内的任务并行执行（最多 parallel_jobs 个）
//! 3. 支持 --parallel / --no-cache / --dry-run 参数

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::cache::local::LocalCache;
use crate::cache::remote::CacheService;
use crate::error::LoomError;
use crate::manifest::priority::ResolvedBuildConfig;
use crate::plugin::PluginRegistry;
use crate::task::TaskGraph;
use crate::task::executor::{Executor, TaskResult};

/// 调度配置
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// 是否启用并行执行
    pub parallel: bool,
    /// 最大并行任务数（0 = 自动，CPU 核心数）
    pub max_jobs: u32,
    /// 是否使用缓存（增量检查）
    pub use_cache: bool,
    /// 是否仅显示将要执行的任务（不实际执行）
    pub dry_run: bool,
    /// 详细输出
    pub verbose: bool,
    /// B3.5: 清理后重编（清除缓存）
    pub clean: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            parallel: true,
            max_jobs: 0,
            use_cache: true,
            dry_run: false,
            verbose: false,
            clean: false,
        }
    }
}

impl SchedulerConfig {
    /// 获取实际并行任务数
    pub fn effective_jobs(&self) -> usize {
        if self.max_jobs == 0 {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        } else {
            self.max_jobs as usize
        }
    }
}

/// 任务调度器
pub struct Scheduler {
    config: SchedulerConfig,
    graph: Arc<TaskGraph>,
    build_config: Arc<ResolvedBuildConfig>,
    cache: Option<Arc<Mutex<LocalCache>>>,
    /// B3.4: 缓存服务（本地 + 远程）
    cache_service: Option<Arc<Mutex<CacheService>>>,
    /// B4.5: 插件注册表
    plugin_registry: Option<Arc<PluginRegistry>>,
    results: Arc<Mutex<Vec<TaskResult>>>,
}

impl Scheduler {
    pub fn new(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            config,
            graph,
            build_config,
            cache,
            cache_service: None,
            plugin_registry: None,
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// B3.4: 创建带缓存服务的调度器
    pub fn with_cache_service(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        cache_service: Option<Arc<Mutex<CacheService>>>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            config,
            graph,
            build_config,
            cache,
            cache_service,
            plugin_registry: None,
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// B4.5: 创建带缓存服务和插件注册表的调度器
    pub fn with_plugins(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        cache_service: Option<Arc<Mutex<CacheService>>>,
        config: SchedulerConfig,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> Self {
        Self {
            config,
            graph,
            build_config,
            cache,
            cache_service,
            plugin_registry,
            results: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 获取所有执行结果
    pub fn results(&self) -> Vec<TaskResult> {
        self.results.lock().unwrap().clone()
    }

    /// 检查是否有失败的任务
    pub fn has_failures(&self) -> bool {
        self.results.lock().unwrap().iter().any(|r| !r.success)
    }

    /// 调度并执行任务
    pub fn execute(&self, root_task: &str) -> Result<(), LoomError> {
        // 1. 验证任务存在
        if !self.graph.contains(root_task) {
            return Err(LoomError::Task(format!("任务 '{}' 不存在", root_task)));
        }

        // 2. 验证任务图
        self.graph.validate()?;

        // 3. 找到可达任务
        let reachable = self.graph.find_reachable(root_task)?;

        // 4. 构建可达子图
        let subgraph = self.build_subgraph(&reachable)?;

        // 5. 拓扑排序
        let topo = subgraph.topological_sort()?;
        let layers = topo.layers;

        if self.config.dry_run {
            self.print_dry_run_plan(&layers, &subgraph);
            return Ok(());
        }

        // 6. 逐层执行
        let total_tasks = layers.iter().map(|l| l.len()).sum::<usize>();
        let total_layers = layers.len();

        if self.config.verbose {
            println!(
                "调度: {} 个任务, {} 层, 并行: {}",
                total_tasks,
                total_layers,
                self.config.effective_jobs()
            );
        }

        for (layer_idx, layer) in layers.iter().enumerate() {
            if self.config.parallel && layer.len() > 1 {
                self.execute_layer_parallel(layer, layer_idx, &subgraph);
            } else {
                self.execute_layer_sequential(layer, layer_idx, &subgraph);
            }
        }

        // 7. 检查失败
        if self.has_failures() {
            let failures: Vec<_> = self
                .results
                .lock()
                .unwrap()
                .iter()
                .filter(|r| !r.success)
                .map(|r| format!("  ✗ {} — {}", r.task_name, r.message))
                .collect();
            return Err(LoomError::Task(format!(
                "构建失败 ({} 个任务失败):\n{}",
                failures.len(),
                failures.join("\n")
            )));
        }

        Ok(())
    }

    fn build_subgraph(&self, names: &[String]) -> Result<TaskGraph, LoomError> {
        let mut sub = TaskGraph::new();
        for name in names {
            if let Some(task) = self.graph.get(name) {
                sub.add_task(task.clone());
            }
        }
        Ok(sub)
    }

    fn print_dry_run_plan(&self, layers: &[Vec<String>], subgraph: &TaskGraph) {
        println!("📋 执行计划 (dry-run):");
        for (layer_idx, layer) in layers.iter().enumerate() {
            println!("  层 {}:", layer_idx);
            for name in layer {
                if let Some(task) = subgraph.get(name) {
                    let kind = format_task_kind(&task.kind);
                    let deps = task.depends_on.join(", ");
                    println!("    - {} [{}] deps=[{}]", name, kind, deps);
                }
            }
        }
    }

    fn execute_layer_sequential(&self, layer: &[String], layer_idx: usize, subgraph: &TaskGraph) {
        for name in layer {
            if let Some(task) = subgraph.get(name) {
                let executor = Executor::with_plugins(
                    self.graph.clone(),
                    self.build_config.clone(),
                    self.cache.clone(),
                    self.cache_service.clone(),
                    self.config.clone(),
                    self.plugin_registry.clone(),
                );
                let result = executor.execute_task(task);
                self.results.lock().unwrap().push(result);
            }
        }
    }

    fn execute_layer_parallel(&self, layer: &[String], layer_idx: usize, subgraph: &TaskGraph) {
        let max_jobs = self.config.effective_jobs();

        let tasks: Vec<_> = layer.iter().filter_map(|name| subgraph.get(name).cloned()).collect();

        let mut handles: VecDeque<std::thread::JoinHandle<()>> = VecDeque::new();

        for task in tasks {
            if handles.len() >= max_jobs {
                if let Some(handle) = handles.pop_front() {
                    let _ = handle.join();
                }
            }

            let graph_clone = self.graph.clone();
            let config_clone = self.config.clone();
            let cache_clone = self.cache.clone();
            let service_clone = self.cache_service.clone();
            let build_config_clone = self.build_config.clone();
            let results_clone = self.results.clone();
            let plugin_registry_clone = self.plugin_registry.clone();

            let handle = std::thread::spawn(move || {
                let executor = Executor::with_plugins(
                    graph_clone,
                    build_config_clone,
                    cache_clone,
                    service_clone,
                    config_clone,
                    plugin_registry_clone,
                );
                let result = executor.execute_task(&task);
                results_clone.lock().unwrap().push(result);
            });

            handles.push_back(handle);
        }

        for handle in handles {
            let _ = handle.join();
        }
    }
}

fn format_task_kind(kind: &crate::task::TaskKind) -> String {
    match kind {
        crate::task::TaskKind::Clean => "clean".to_string(),
        crate::task::TaskKind::Resolve => "resolve".to_string(),
        crate::task::TaskKind::Compile(ss) => format!("compile:{}", ss),
        crate::task::TaskKind::Test => "test".to_string(),
        crate::task::TaskKind::Package => "package".to_string(),
        crate::task::TaskKind::Verify => "verify".to_string(),
        crate::task::TaskKind::Check => "check".to_string(),
        crate::task::TaskKind::Install => "install".to_string(),
        crate::task::TaskKind::Deploy => "deploy".to_string(),
        crate::task::TaskKind::Execute => "execute".to_string(),
        crate::task::TaskKind::Watch => "watch".to_string(),
        crate::task::TaskKind::Plugin(name) => format!("plugin:{}", name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::priority::ResolvedBuildConfig;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};
    use std::sync::Arc;

    fn make_task(name: &str, deps: &[&str]) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: format!("test task {}", name),
            kind: TaskKind::Clean,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        }
    }

    fn make_scheduler(graph: TaskGraph) -> Scheduler {
        let config = SchedulerConfig {
            parallel: false,
            dry_run: false,
            use_cache: false,
            verbose: false,
            ..Default::default()
        };
        Scheduler::new(
            Arc::new(graph),
            Arc::new(ResolvedBuildConfig::default()),
            None,
            config,
        )
    }

    #[test]
    fn test_scheduler_execute_simple() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        let scheduler = make_scheduler(graph);

        assert!(scheduler.execute("clean").is_ok());
        let results = scheduler.results();
        assert_eq!(results.len(), 1);
        assert!(results[0].success);
    }

    #[test]
    fn test_scheduler_execute_chain() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &[]));
        graph.add_task(make_task("b", &["a"]));
        let scheduler = make_scheduler(graph);

        assert!(scheduler.execute("b").is_ok());
        let results = scheduler.results();
        assert_eq!(results.len(), 2);
        // a should be executed first
        assert_eq!(results[0].task_name, "a");
        assert_eq!(results[1].task_name, "b");
    }

    #[test]
    fn test_scheduler_dry_run() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        graph.add_task(make_task("compile", &["clean"]));

        let config = SchedulerConfig {
            dry_run: true,
            ..Default::default()
        };
        let scheduler = Scheduler::new(
            Arc::new(graph),
            Arc::new(ResolvedBuildConfig::default()),
            None,
            config,
        );

        assert!(scheduler.execute("compile").is_ok());
        // Dry run should not produce any real results
        assert!(scheduler.results().is_empty());
    }

    #[test]
    fn test_scheduler_nonexistent_task() {
        let graph = TaskGraph::new();
        let scheduler = make_scheduler(graph);

        assert!(scheduler.execute("nonexistent").is_err());
    }

    #[test]
    fn test_scheduler_cycle_detection() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &["b"]));
        graph.add_task(make_task("b", &["a"]));
        let scheduler = make_scheduler(graph);

        assert!(scheduler.execute("a").is_err());
    }

    #[test]
    fn test_scheduler_missing_dependency() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("a", &["nonexistent"]));
        let scheduler = make_scheduler(graph);

        assert!(scheduler.execute("a").is_err());
    }

    #[test]
    fn test_scheduler_effective_jobs() {
        let config = SchedulerConfig {
            max_jobs: 0,
            ..Default::default()
        };
        let jobs = config.effective_jobs();
        assert!(jobs >= 1); // Should be at least 1

        let config2 = SchedulerConfig {
            max_jobs: 4,
            ..Default::default()
        };
        assert_eq!(config2.effective_jobs(), 4);
    }

    #[test]
    fn test_scheduler_has_failures() {
        let mut graph = TaskGraph::new();
        graph.add_task(make_task("clean", &[]));
        let scheduler = make_scheduler(graph);

        assert!(!scheduler.has_failures());
        scheduler.execute("clean").unwrap();
        assert!(!scheduler.has_failures());
    }

    #[test]
    fn test_scheduler_format_task_kind() {
        assert_eq!(format_task_kind(&TaskKind::Clean), "clean");
        assert_eq!(
            format_task_kind(&TaskKind::Compile("main".to_string())),
            "compile:main"
        );
        assert_eq!(
            format_task_kind(&TaskKind::Plugin("doc-gen".to_string())),
            "plugin:doc-gen"
        );
    }
}
