//! [Phase B4.1] PluginContext 实现
//!
//! 插件上下文提供给插件的 API，包含项目配置、源码集、任务列表、构建环境等。
//! 对应设计文档 §9.1 `PluginContext`。

use crate::manifest::priority::ResolvedBuildConfig;
use crate::manifest::{LoomManifest, SourceSet};
use crate::plugin::BuildEnvironment;
use crate::task::TaskDefinition;
use std::path::{Path, PathBuf};

/// 插件上下文
///
/// 在 `configure()` 阶段可修改（注册任务、修改源码集），
/// 在 `execute()` 阶段只读。
///
/// 对应设计文档 §9.1：
/// ```text
/// struct PluginContext {
///     manifest: PackageManifest,
///     source_sets: Vec<SourceSet>,
///     dependency_graph: DependencyGraph,
///     environment: BuildEnvironment,
///     tasks: Vec<TaskDefinition>,
/// }
/// ```
#[derive(Debug, Clone)]
pub struct PluginContext {
    /// 项目配置清单
    pub manifest: LoomManifest,
    /// 源码集列表
    pub source_sets: Vec<SourceSet>,
    /// 项目根目录
    pub project_dir: PathBuf,
    /// 构建环境
    pub environment: BuildEnvironment,
    /// 已解析的最终构建配置
    pub build_config: ResolvedBuildConfig,
    /// 插件注册的任务列表（configure 阶段填充）
    pub tasks: Vec<TaskDefinition>,
    /// 已激活的插件名称列表
    pub active_plugins: Vec<String>,
}

impl PluginContext {
    /// 创建默认的插件上下文（用于测试）
    pub fn new_default() -> Self {
        let manifest = LoomManifest {
            schema_version: "2.0".to_string(),
            name: "test-project".to_string(),
            version: "0.1.0".to_string(),
            ..Default::default()
        };
        Self {
            manifest,
            source_sets: Vec::new(),
            project_dir: PathBuf::from("."),
            environment: BuildEnvironment::default(),
            build_config: ResolvedBuildConfig::default(),
            tasks: Vec::new(),
            active_plugins: Vec::new(),
        }
    }

    /// 从完整参数创建插件上下文
    pub fn new(
        manifest: LoomManifest,
        source_sets: Vec<SourceSet>,
        project_dir: PathBuf,
        build_config: ResolvedBuildConfig,
    ) -> Self {
        let environment = BuildEnvironment::from_build_config(&build_config, &project_dir);
        Self {
            manifest,
            source_sets,
            project_dir,
            environment,
            build_config,
            tasks: Vec::new(),
            active_plugins: Vec::new(),
        }
    }

    /// 添加已激活的插件名称
    pub fn activate_plugin(&mut self, name: &str) {
        if !self.active_plugins.contains(&name.to_string()) {
            self.active_plugins.push(name.to_string());
        }
    }

    /// 检查插件是否已激活
    pub fn is_plugin_active(&self, name: &str) -> bool {
        self.active_plugins.iter().any(|n| n == name)
    }

    /// 添加任务到上下文
    pub fn add_task(&mut self, task: TaskDefinition) {
        self.tasks.push(task);
    }

    /// 获取输出目录（从构建配置）
    pub fn out_dir(&self) -> PathBuf {
        PathBuf::from(&self.build_config.out_dir)
    }

    /// 获取缓存目录（从构建配置）
    pub fn cache_dir(&self) -> PathBuf {
        PathBuf::from(&self.build_config.cache_dir)
    }
}

impl Default for PluginContext {
    fn default() -> Self {
        Self::new_default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BuildEnvironment 实现
// ═══════════════════════════════════════════════════════════════════════════════

impl BuildEnvironment {
    /// 从 ResolvedBuildConfig 和 project_dir 创建构建环境
    pub fn from_build_config(config: &ResolvedBuildConfig, project_dir: &std::path::Path) -> Self {
        let out_dir =
            if project_dir.join(&config.out_dir).is_relative() || config.out_dir.starts_with('.') {
                project_dir.join(&config.out_dir)
            } else {
                PathBuf::from(&config.out_dir)
            };
        let cache_dir = if project_dir.join(&config.cache_dir).is_relative()
            || config.cache_dir.starts_with('.')
        {
            project_dir.join(&config.cache_dir)
        } else {
            PathBuf::from(&config.cache_dir)
        };

        Self {
            project_dir: project_dir.to_path_buf(),
            out_dir,
            cache_dir,
            remote_cache_url: config.cache_remote.clone(),
            target: config.target.clone(),
            opt_level: config.opt_level,
            debug: config.debug,
            parallel: config.parallel,
            active_profile: config.active_profile.clone(),
        }
    }
}

impl Default for BuildEnvironment {
    fn default() -> Self {
        Self {
            project_dir: PathBuf::from("."),
            out_dir: PathBuf::from("target/build"),
            cache_dir: PathBuf::from("target/cache"),
            remote_cache_url: None,
            target: None,
            opt_level: 2,
            debug: true,
            parallel: true,
            active_profile: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse::default_manifest;
    use crate::manifest::priority::ResolvedBuildConfig;
    use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};

    #[test]
    fn test_context_new_default() {
        let ctx = PluginContext::new_default();
        assert_eq!(ctx.manifest.name, "test-project");
        assert!(ctx.source_sets.is_empty());
        assert!(ctx.tasks.is_empty());
        assert!(ctx.active_plugins.is_empty());
        assert_eq!(ctx.project_dir, PathBuf::from("."));
    }

    #[test]
    fn test_context_new_custom() {
        let manifest = default_manifest("my-app");
        let source_sets = vec![SourceSet::main(Path::new("."))];
        let project_dir = PathBuf::from("/tmp/test");
        let config = ResolvedBuildConfig::isolated();

        let ctx = PluginContext::new(manifest, source_sets, project_dir.clone(), config);
        assert_eq!(ctx.project_dir, project_dir);
        assert_eq!(ctx.source_sets.len(), 1);
        assert_eq!(ctx.source_sets[0].name, "main");
    }

    #[test]
    fn test_context_activate_plugin() {
        let mut ctx = PluginContext::new_default();
        ctx.activate_plugin("aura-stdlib");
        ctx.activate_plugin("aura-test-harness");
        assert_eq!(ctx.active_plugins.len(), 2);
        assert!(ctx.is_plugin_active("aura-stdlib"));
        assert!(ctx.is_plugin_active("aura-test-harness"));
        assert!(!ctx.is_plugin_active("aura-doc-gen"));
    }

    #[test]
    fn test_context_activate_plugin_dedup() {
        let mut ctx = PluginContext::new_default();
        ctx.activate_plugin("aura-stdlib");
        ctx.activate_plugin("aura-stdlib"); // 重复激活
        assert_eq!(ctx.active_plugins.len(), 1);
    }

    #[test]
    fn test_context_add_task() {
        let mut ctx = PluginContext::new_default();
        let task = TaskDefinition {
            name: "doc".to_string(),
            description: "Generate docs".to_string(),
            kind: TaskKind::Plugin("doc-gen".to_string()),
            depends_on: vec!["compile-main".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        };
        ctx.add_task(task);
        assert_eq!(ctx.tasks.len(), 1);
        assert_eq!(ctx.tasks[0].name, "doc");
    }

    #[test]
    fn test_context_out_dir() {
        let mut ctx = PluginContext::new_default();
        ctx.build_config.out_dir = "target/build".to_string();
        assert_eq!(ctx.out_dir(), PathBuf::from("target/build"));
    }

    #[test]
    fn test_context_cache_dir() {
        let mut ctx = PluginContext::new_default();
        ctx.build_config.cache_dir = "target/cache".to_string();
        assert_eq!(ctx.cache_dir(), PathBuf::from("target/cache"));
    }

    #[test]
    fn test_context_default() {
        let ctx = PluginContext::default();
        assert_eq!(ctx.manifest.name, "test-project");
    }

    #[test]
    fn test_build_environment_default() {
        let env = BuildEnvironment::default();
        assert_eq!(env.project_dir, PathBuf::from("."));
        assert_eq!(env.opt_level, 2);
        assert!(env.debug);
        assert!(env.parallel);
    }

    #[test]
    fn test_build_environment_from_config() {
        let config = ResolvedBuildConfig {
            opt_level: 3,
            debug: false,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            out_dir: "target/build".to_string(),
            cache_dir: "target/cache".to_string(),
            ..Default::default()
        };
        let project_dir = Path::new("/tmp/test-project");
        let env = BuildEnvironment::from_build_config(&config, project_dir);
        assert_eq!(env.opt_level, 3);
        assert!(!env.debug);
        assert_eq!(env.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert!(env.project_dir.ends_with("test-project"));
    }

    #[test]
    fn test_build_environment_from_config_active_profile() {
        let config = ResolvedBuildConfig {
            active_profile: Some("release".to_string()),
            ..Default::default()
        };
        let env = BuildEnvironment::from_build_config(&config, Path::new("."));
        assert_eq!(env.active_profile.as_deref(), Some("release"));
    }
}
