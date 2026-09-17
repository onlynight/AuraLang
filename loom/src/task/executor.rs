//! [Phase B2-B3] 任务执行器：分派任务到对应的处理器
//!
//! 执行流程：
//! 1. 增量检查（fingerprint 比对）
//! 2. 缓存查找（本地 → 远程 → 执行）
//! 3. 分派到对应的任务处理器（clean/resolve/compile/test/package/verify/run）
//! 4. 更新缓存（本地 + 远程）
//!
//! B3.4: 集成 CacheService（本地 + 远程两级缓存）
//! B3.5: --no-cache / --clean 参数

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::cache::fingerprint::Fingerprint;
use crate::cache::local::LocalCache;
use crate::cache::remote::CacheService;
use crate::error::LoomError;
use crate::manifest::priority::ResolvedBuildConfig;
use crate::plugin::PluginRegistry;
use crate::task::scheduler::SchedulerConfig;
use crate::task::{TaskDefinition, TaskGraph, TaskKind};

/// 任务执行结果
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// 任务名称
    pub task_name: String,
    /// 是否执行（true = 执行，false = 跳过/up-to-date）
    pub executed: bool,
    /// 执行时间（毫秒）
    pub elapsed_ms: u64,
    /// 成功/失败
    pub success: bool,
    /// 输出消息
    pub message: String,
    /// 产物路径
    pub artifacts: Vec<std::path::PathBuf>,
    /// 缓存命中
    pub cache_hit: bool,
    /// 缓存来源（"local" / "remote" / "miss" / 空）
    pub cache_source: String,
}

impl TaskResult {
    pub fn skipped(task_name: &str) -> Self {
        Self {
            task_name: task_name.to_string(),
            executed: false,
            elapsed_ms: 0,
            success: true,
            message: String::new(),
            artifacts: Vec::new(),
            cache_hit: false,
            cache_source: String::new(),
        }
    }

    pub fn up_to_date(task_name: &str, fingerprint: &str, source: &str) -> Self {
        Self {
            task_name: task_name.to_string(),
            executed: false,
            elapsed_ms: 0,
            success: true,
            message: format!(
                "✓ up-to-date (fingerprint: {}..., source: {})",
                &fingerprint[..8.min(fingerprint.len())],
                source
            ),
            artifacts: Vec::new(),
            cache_hit: true,
            cache_source: source.to_string(),
        }
    }

    pub fn success(
        task_name: &str,
        elapsed_ms: u64,
        message: String,
        artifacts: Vec<std::path::PathBuf>,
        cache_hit: bool,
        cache_source: &str,
    ) -> Self {
        Self {
            task_name: task_name.to_string(),
            executed: true,
            elapsed_ms,
            success: true,
            message,
            artifacts,
            cache_hit,
            cache_source: cache_source.to_string(),
        }
    }

    pub fn failure(task_name: &str, elapsed_ms: u64, message: String) -> Self {
        Self {
            task_name: task_name.to_string(),
            executed: true,
            elapsed_ms,
            success: false,
            message,
            artifacts: Vec::new(),
            cache_hit: false,
            cache_source: String::new(),
        }
    }
}

/// 任务执行器
pub struct Executor {
    _graph: Arc<TaskGraph>,
    _build_config: Arc<ResolvedBuildConfig>,
    _cache: Option<Arc<Mutex<LocalCache>>>,
    /// B3.4: 缓存服务（本地 + 远程）
    cache_service: Option<Arc<Mutex<CacheService>>>,
    config: SchedulerConfig,
    /// B4.5: 插件注册表（用于执行插件任务）
    plugin_registry: Option<Arc<PluginRegistry>>,
}

impl Executor {
    pub fn new(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            _graph: graph,
            _build_config: build_config,
            _cache: cache,
            cache_service: None,
            config,
            plugin_registry: None,
        }
    }

    /// B3.4: 创建带缓存服务的执行器
    pub fn with_cache_service(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        cache_service: Option<Arc<Mutex<CacheService>>>,
        config: SchedulerConfig,
    ) -> Self {
        Self {
            _graph: graph,
            _build_config: build_config,
            _cache: cache,
            cache_service,
            config,
            plugin_registry: None,
        }
    }

    /// B4.5: 创建带缓存服务和插件注册表的执行器
    pub fn with_plugins(
        graph: Arc<TaskGraph>,
        build_config: Arc<ResolvedBuildConfig>,
        cache: Option<Arc<Mutex<LocalCache>>>,
        cache_service: Option<Arc<Mutex<CacheService>>>,
        config: SchedulerConfig,
        plugin_registry: Option<Arc<PluginRegistry>>,
    ) -> Self {
        Self {
            _graph: graph,
            _build_config: build_config,
            _cache: cache,
            cache_service,
            config,
            plugin_registry,
        }
    }

    /// 执行单个任务
    pub fn execute_task(&self, task: &TaskDefinition) -> TaskResult {
        if self.config.dry_run {
            return TaskResult::skipped(&task.name);
        }

        // 0. 处理 --clean：清除该任务及所有缓存
        if self.config.clean {
            if let Some(ref cache_service) = self.cache_service {
                if let Ok(mut svc) = cache_service.lock() {
                    let _ = svc.invalidate(task);
                    tracing::info!("--clean: Cleared cache for task '{}'", task.name);
                }
            } else if let Some(ref cache) = self._cache {
                if let Ok(mut c) = cache.lock() {
                    let _ = c.clear();
                    tracing::info!("--clean: Cleared local cache");
                }
            }
        }

        // 1. 增量检查（B3.4: 使用 CacheService 查找本地 + 远程缓存）
        if self.config.use_cache {
            // 使用 CacheService（如果可用）
            if let Some(ref cache_service) = self.cache_service {
                if let Ok(mut svc) = cache_service.lock() {
                    match svc.lookup(task) {
                        Ok((true, source)) => {
                            let source_str = match source {
                                crate::cache::remote::CacheHitSource::Local => "local",
                                crate::cache::remote::CacheHitSource::Remote => "remote",
                                crate::cache::remote::CacheHitSource::Miss => "miss",
                            };
                            if let Ok(fp) = Self::compute_fingerprint(task) {
                                return TaskResult::up_to_date(&task.name, &fp, source_str);
                            }
                        }
                        Ok((false, _)) => {
                            // 缓存未命中，需要执行
                        }
                        Err(e) => {
                            tracing::warn!("Cache lookup failed: {}", e);
                        }
                    }
                }
            }
            // 回退到仅本地缓存（兼容旧代码路径）
            else if let Some(cache) = &self._cache {
                let cache_guard = match cache.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        return TaskResult::failure(
                            &task.name,
                            0,
                            "Cache lock acquisition failed".to_string(),
                        );
                    }
                };
                match Self::check_up_to_date(task, &cache_guard) {
                    Some(fingerprint) => {
                        return TaskResult::up_to_date(&task.name, &fingerprint, "local");
                    }
                    None => {
                        // Out-of-date，需要执行
                    }
                }
            }
        }

        // 2. 尝试从缓存恢复产物（B3.4）
        if self.config.use_cache && !self.config.clean {
            if let Some(ref cache_service) = self.cache_service {
                if let Ok(mut svc) = cache_service.lock() {
                    let out_dir = self._build_config.out_dir.clone();
                    let dest = std::path::PathBuf::from(&out_dir);
                    if let Ok(Some(restored)) = svc.restore(task, &dest) {
                        let elapsed = 0;
                        let fp = Self::compute_fingerprint(task).unwrap_or_default();
                        let source_str = match svc.stats().remote_hits > 0 {
                            true => "remote",
                            false => "local",
                        };
                        return TaskResult {
                            task_name: task.name.clone(),
                            executed: false,
                            elapsed_ms: elapsed,
                            success: true,
                            message: format!(
                                "✓ Restored {} files from cache (source: {})",
                                restored.len(),
                                source_str
                            ),
                            artifacts: restored,
                            cache_hit: true,
                            cache_source: source_str.to_string(),
                        };
                    }
                }
            }
        }

        // 3. 执行任务
        let start = Instant::now();
        match self.dispatch_task(task) {
            Ok((message, artifacts)) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let result =
                    TaskResult::success(&task.name, elapsed, message, artifacts.clone(), false, "");

                // 4. 更新缓存（B3.4: 存储到本地 + 远程）
                if self.config.use_cache && !self.config.clean {
                    if let Some(ref cache_service) = self.cache_service {
                        if let Ok(mut svc) = cache_service.lock() {
                            let _ = svc.store(task, &artifacts);
                        }
                    } else if let Some(cache) = &self._cache {
                        if let Ok(fp) = Self::compute_fingerprint(task) {
                            if let Ok(mut cache_guard) = cache.lock() {
                                let _ = cache_guard.store_fingerprint(&task.name, &fp);
                                let _ = cache_guard.store_artifacts(&task.name, &artifacts);
                            }
                        }
                    }
                }

                result
            }
            Err(err) => {
                let elapsed = start.elapsed().as_millis() as u64;
                TaskResult::failure(&task.name, elapsed, err.to_string())
            }
        }
    }

    /// 分派任务到对应的处理器
    fn dispatch_task(
        &self,
        task: &TaskDefinition,
    ) -> Result<(String, Vec<std::path::PathBuf>), LoomError> {
        use crate::task::builtin;
        let config = &self._build_config;

        match &task.kind {
            TaskKind::Clean => builtin::execute_clean(task, config),
            TaskKind::Resolve => builtin::execute_resolve(task, config),
            TaskKind::Compile(source_set) => builtin::execute_compile(task, source_set, config),
            TaskKind::Test => builtin::execute_test(task, config),
            TaskKind::Package => builtin::execute_package(task, config),
            TaskKind::Verify => builtin::execute_verify(task, config),
            TaskKind::Check => builtin::execute_check(task, config),
            TaskKind::Install => builtin::execute_install(task, config),
            TaskKind::Deploy => builtin::execute_deploy(task, config),
            TaskKind::Execute => builtin::execute_execute(task, config),
            TaskKind::Watch => builtin::execute_watch(task, config),
            TaskKind::Plugin(plugin_id) => {
                // B4.5: 通过插件注册表执行插件任务
                if let Some(ref registry) = self.plugin_registry {
                    // 构建 PluginContext 用于传递给插件
                    let project_dir = std::path::PathBuf::from(".");
                    let ctx = crate::plugin::context::PluginContext::new(
                        crate::manifest::LoomManifest::default(),
                        Vec::new(),
                        project_dir,
                        config.as_ref().clone(),
                    );
                    // 传递任务名（而非 plugin_id）给插件执行
                    let result = registry.execute_task(&task.name, &ctx)?;
                    Ok((result.output, result.artifacts))
                } else {
                    // 回退到占位符实现
                    builtin::execute_plugin(task, plugin_id, config)
                }
            }
        }
    }

    /// 计算任务 fingerprint
    fn compute_fingerprint(task: &TaskDefinition) -> Result<String, LoomError> {
        Fingerprint::compute(task)
    }

    /// 检查任务是否 up-to-date
    fn check_up_to_date(task: &TaskDefinition, cache: &LocalCache) -> Option<String> {
        if let Ok(fp) = Fingerprint::compute(task) {
            if let Ok(cached) = cache.lookup_fingerprint(&task.name) {
                if cached.as_deref() == Some(fp.as_str()) {
                    return Some(fp);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::local::LocalCache;
    use crate::cache::remote::CacheService;
    use crate::manifest::priority::{CliOverrides, ResolvedBuildConfig};
    use crate::task::scheduler::SchedulerConfig;
    use crate::task::{TaskDefinition, TaskGraph, TaskInputs, TaskKind, TaskOutputs};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn make_task(name: &str, kind: TaskKind) -> TaskDefinition {
        TaskDefinition {
            name: name.to_string(),
            description: format!("test task {}", name),
            kind,
            depends_on: Vec::new(),
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        }
    }

    fn make_executor(config: SchedulerConfig) -> Executor {
        Executor::new(
            Arc::new(TaskGraph::new()),
            Arc::new(ResolvedBuildConfig::isolated()),
            None,
            config,
        )
    }

    fn make_executor_with_cache(
        config: SchedulerConfig,
    ) -> (Executor, Arc<Mutex<LocalCache>>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(Mutex::new(LocalCache::new(tmp.path()).unwrap()));
        let executor = Executor::new(
            Arc::new(TaskGraph::new()),
            Arc::new(ResolvedBuildConfig::isolated()),
            Some(cache.clone()),
            config,
        );
        (executor, cache, tmp)
    }

    fn make_executor_with_service(
        config: SchedulerConfig,
    ) -> (Executor, Arc<Mutex<CacheService>>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(Mutex::new(LocalCache::new(tmp.path()).unwrap()));
        let build_config = Arc::new(ResolvedBuildConfig::isolated());
        let service = Arc::new(Mutex::new(
            CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
        ));
        let executor = Executor::with_cache_service(
            Arc::new(TaskGraph::new()),
            build_config,
            Some(cache),
            Some(service.clone()),
            config,
        );
        (executor, service, tmp)
    }

    #[test]
    fn test_execute_clean() {
        let executor = make_executor(SchedulerConfig::default());
        let task = make_task("clean", TaskKind::Clean);
        let result = executor.execute_task(&task);
        assert!(result.success);
    }

    #[test]
    fn test_execute_dry_run() {
        let config = SchedulerConfig {
            dry_run: true,
            ..Default::default()
        };
        let executor = make_executor(config);
        let task = make_task("clean", TaskKind::Clean);
        let result = executor.execute_task(&task);
        assert!(!result.executed);
    }

    #[test]
    fn test_task_result_skipped() {
        let result = TaskResult::skipped("test");
        assert_eq!(result.task_name, "test");
        assert!(!result.executed);
        assert!(result.success);
        assert!(!result.cache_hit);
    }

    #[test]
    fn test_task_result_up_to_date() {
        let result = TaskResult::up_to_date("test", "abc123def456", "local");
        assert_eq!(result.task_name, "test");
        assert!(!result.executed);
        assert!(result.success);
        assert!(result.cache_hit);
        assert_eq!(result.cache_source, "local");
    }

    #[test]
    fn test_task_result_up_to_date_remote() {
        let result = TaskResult::up_to_date("test", "abc123def456", "remote");
        assert_eq!(result.cache_source, "remote");
    }

    #[test]
    fn test_task_result_success() {
        let result = TaskResult::success("test", 100, "ok".to_string(), Vec::new(), false, "");
        assert_eq!(result.task_name, "test");
        assert!(result.executed);
        assert!(result.success);
        assert_eq!(result.elapsed_ms, 100);
        assert_eq!(result.message, "ok");
    }

    #[test]
    fn test_task_result_failure() {
        let result = TaskResult::failure("test", 50, "error".to_string());
        assert_eq!(result.task_name, "test");
        assert!(result.executed);
        assert!(!result.success);
        assert_eq!(result.elapsed_ms, 50);
    }

    #[test]
    fn test_compute_fingerprint() {
        let executor = make_executor(SchedulerConfig::default());
        let task = make_task("compile", TaskKind::Compile("main".to_string()));
        let fp = Executor::compute_fingerprint(&task).unwrap();
        assert_eq!(fp.len(), 64);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // B3.5: --no-cache / --clean 测试
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_no_cache_disables_incremental() {
        let config = SchedulerConfig {
            use_cache: false,
            ..Default::default()
        };
        let (executor, _cache, _tmp) = make_executor_with_cache(config);
        let task = make_task("compile", TaskKind::Compile("main".to_string()));

        // 第一次执行
        let result1 = executor.execute_task(&task);
        assert!(result1.success);
        assert!(result1.executed); // 应该执行

        // 第二次执行（no-cache 模式，应该再次执行）
        let result2 = executor.execute_task(&task);
        assert!(result2.executed); // 仍然执行
    }

    #[test]
    fn test_cache_incremental() {
        let config = SchedulerConfig {
            use_cache: true,
            ..Default::default()
        };
        let (executor, _cache, _tmp) = make_executor_with_cache(config);
        let task = make_task("clean", TaskKind::Clean);

        // 第一次执行（无缓存）
        let result1 = executor.execute_task(&task);
        assert!(result1.executed);
        assert!(!result1.cache_hit);

        // 第二次执行（应该有缓存命中）
        let result2 = executor.execute_task(&task);
        assert!(result2.cache_hit);
        assert!(!result2.executed);
    }

    #[test]
    fn test_clean_clears_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = Arc::new(Mutex::new(LocalCache::new(tmp.path()).unwrap()));
        let build_config = Arc::new(ResolvedBuildConfig::isolated());
        let service = Arc::new(Mutex::new(
            CacheService::new(cache.clone(), None, build_config.clone()).unwrap(),
        ));

        let config = SchedulerConfig {
            use_cache: true,
            clean: true,
            ..Default::default()
        };
        let executor = Executor::with_cache_service(
            Arc::new(TaskGraph::new()),
            build_config,
            Some(cache),
            Some(service),
            config,
        );

        let task = make_task("clean", TaskKind::Clean);

        // 第一次执行（clean 模式，应该清除缓存并执行）
        let result1 = executor.execute_task(&task);
        assert!(result1.executed);

        // 第二次执行（clean 模式，应该再次清除并执行）
        let result2 = executor.execute_task(&task);
        assert!(result2.executed); // clean 模式每次都应该执行
    }

    #[test]
    fn test_executor_with_cache_service() {
        let config = SchedulerConfig::default();
        let (executor, service, tmp) = make_executor_with_service(config);

        // 创建源文件
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.aura"), "fun main() {}").unwrap();

        let task = TaskDefinition {
            name: "compile-main".to_string(),
            description: String::new(),
            kind: TaskKind::Compile("main".to_string()),
            depends_on: Vec::new(),
            inputs: TaskInputs {
                files: vec![src.join("main.aura")],
                options: std::collections::HashMap::new(),
                dep_fingerprints: std::collections::HashMap::new(),
            },
            outputs: TaskOutputs::default(),
        };

        // 第一次执行（缓存未命中）
        let result1 = executor.execute_task(&task);
        assert!(result1.success);
        assert!(result1.executed);

        // 第二次执行（应该缓存命中）
        let result2 = executor.execute_task(&task);
        assert!(result2.success);
        assert!(result2.cache_hit);
        assert!(!result2.executed);
        assert_eq!(result2.cache_source, "local");
    }

    #[test]
    fn test_executor_cache_service_store_artifacts() {
        let config = SchedulerConfig::default();
        let (executor, service, tmp) = make_executor_with_service(config);
        let task = make_task("package", TaskKind::Package);

        // 创建输出目录
        let out_dir = tmp.path().join("target/build");
        std::fs::create_dir_all(&out_dir).unwrap();

        // 第一次执行
        let result1 = executor.execute_task(&task);
        assert!(result1.success);

        // 检查服务统计
        let binding = service.lock().unwrap();
        let stats = binding.stats();
        assert!(stats.misses >= 1);
    }
}
