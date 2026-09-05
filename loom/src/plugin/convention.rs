//! [Phase B4.2] 约定插件加载
//!
//! 约定插件自动激活，提供默认行为。在构建开始时自动加载，无需在 aura.toml 中声明。
//!
//! 内置约定插件：
//! - `aura-stdlib`：自动注册标准库模块（io/math/string/json 等）
//! - `aura-test-harness`：注册 `run-tests` 任务，注入测试框架
//! - `aura-watch`：文件监听 + 增量重编
//!
//! 对应设计文档 §9.2 内置插件清单。

use crate::error::LoomError;
use crate::plugin::context::PluginContext;
use crate::plugin::r#trait::BuildPlugin;
use crate::plugin::{PluginKind, TaskResult};
use crate::task::{TaskDefinition, TaskInputs, TaskKind, TaskOutputs};

// ═══════════════════════════════════════════════════════════════════════════════
// aura-stdlib：标准库插件
// ═══════════════════════════════════════════════════════════════════════════════

/// 标准库插件
///
/// 自动注册标准库模块路径，使编译任务可以导入 `aura.std.*` 模块。
/// 约定插件，自动激活。
pub struct StdlibPlugin;

impl BuildPlugin for StdlibPlugin {
    fn name(&self) -> &str { "aura-stdlib" }
    fn version(&self) -> &str { "1.0.0" }
    fn kind(&self) -> PluginKind { PluginKind::Convention }
    fn description(&self) -> Option<&str> {
        Some("标准库插件：自动注册 io/math/string/json 等标准模块")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-stdlib");

        // 记录标准库模块信息（实际注册由编译器处理）
        tracing::info!("aura-stdlib: 已注册标准库模块");

        // 标准库模块列表（文档用途）
        let std_modules = [
            "aura.std.io",
            "aura.std.math",
            "aura.std.string",
            "aura.std.json",
            "aura.std.collections",
            "aura.std.datetime",
        ];
        tracing::debug!("  标准库模块: {}", std_modules.join(", "));

        Ok(())
    }

    fn execute(
        &self,
        _task_name: &str,
        _ctx: &PluginContext,
    ) -> Result<TaskResult, LoomError> {
        // 标准库插件不执行任务，仅注册模块
        Ok(TaskResult::ok("aura-stdlib: 无任务可执行（模块注册在 configure 阶段完成）"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-test-harness：测试框架插件
// ═══════════════════════════════════════════════════════════════════════════════

/// 测试框架插件
///
/// 注册 `run-tests` 任务，注入测试框架支持。
/// 约定插件，自动激活。
pub struct TestHarnessPlugin;

impl BuildPlugin for TestHarnessPlugin {
    fn name(&self) -> &str { "aura-test-harness" }
    fn version(&self) -> &str { "1.0.0" }
    fn kind(&self) -> PluginKind { PluginKind::Convention }
    fn description(&self) -> Option<&str> {
        Some("测试框架插件：注册 run-tests 任务，注入测试框架")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-test-harness");
        tracing::info!("aura-test-harness: 测试框架已激活");

        // 如果 manifest 没有定义 run-tests 任务，则注册一个
        let has_run_tests = ctx
            .tasks
            .iter()
            .any(|t| t.name == "run-tests");

        if !has_run_tests {
            let task = TaskDefinition {
                name: "run-tests".to_string(),
                description: "执行测试".to_string(),
                kind: TaskKind::Test,
                depends_on: vec!["compile-test".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  已注册任务: run-tests");
        }

        Ok(())
    }

    fn execute(
        &self,
        _task_name: &str,
        _ctx: &PluginContext,
    ) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok("aura-test-harness: 测试框架已就绪"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// aura-watch：Watch 模式插件
// ═══════════════════════════════════════════════════════════════════════════════

/// Watch 模式插件
///
/// 启用文件监听 + 增量重编。
/// 约定插件，自动激活。
pub struct WatchPlugin;

impl BuildPlugin for WatchPlugin {
    fn name(&self) -> &str { "aura-watch" }
    fn version(&self) -> &str { "1.0.0" }
    fn kind(&self) -> PluginKind { PluginKind::Convention }
    fn description(&self) -> Option<&str> {
        Some("Watch 模式插件：文件监听 + 增量重编")
    }

    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> {
        ctx.activate_plugin("aura-watch");
        tracing::info!("aura-watch: Watch 模式插件已激活");

        // 注册 watch 任务（如果尚未存在）
        let has_watch = ctx.tasks.iter().any(|t| t.name == "watch");

        if !has_watch {
            let task = TaskDefinition {
                name: "watch".to_string(),
                description: "监听源码变化，增量重编".to_string(),
                kind: TaskKind::Watch,
                depends_on: vec!["resolve".to_string()],
                inputs: TaskInputs::default(),
                outputs: TaskOutputs::default(),
            };
            ctx.add_task(task);
            tracing::info!("  已注册任务: watch");
        }

        Ok(())
    }

    fn execute(
        &self,
        _task_name: &str,
        _ctx: &PluginContext,
    ) -> Result<TaskResult, LoomError> {
        Ok(TaskResult::ok("aura-watch: Watch 模式已就绪（Phase B7 实现完整功能）"))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 约定插件列表
// ═══════════════════════════════════════════════════════════════════════════════

/// 获取所有内置约定插件
///
/// 这些插件在构建开始时自动激活，无需在 aura.toml 中声明。
/// 用户可通过 `[plugins]` 表中的 `aura-xxx = false` 禁用。
pub fn convention_plugins(manifest: &crate::manifest::LoomManifest) -> Vec<Box<dyn BuildPlugin>> {
    let mut plugins: Vec<Box<dyn BuildPlugin>> = Vec::new();

    if manifest.plugins.aura_stdlib {
        plugins.push(Box::new(StdlibPlugin));
        tracing::debug!("已激活约定插件: aura-stdlib");
    }

    if manifest.plugins.aura_test_harness {
        plugins.push(Box::new(TestHarnessPlugin));
        tracing::debug!("已激活约定插件: aura-test-harness");
    }

    if manifest.plugins.aura_watch {
        plugins.push(Box::new(WatchPlugin));
        tracing::debug!("已激活约定插件: aura-watch");
    }

    plugins
}

// ═══════════════════════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse::default_manifest;

    #[test]
    fn test_stdlib_plugin_name() {
        let plugin = StdlibPlugin;
        assert_eq!(plugin.name(), "aura-stdlib");
        assert_eq!(plugin.version(), "1.0.0");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_stdlib_plugin_description() {
        let plugin = StdlibPlugin;
        assert!(plugin.description().is_some());
        assert!(plugin.description().unwrap().contains("标准库"));
    }

    #[test]
    fn test_stdlib_plugin_configure() {
        let plugin = StdlibPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());
        assert!(ctx.is_plugin_active("aura-stdlib"));
    }

    #[test]
    fn test_stdlib_plugin_execute() {
        let plugin = StdlibPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("any", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("aura-stdlib"));
    }

    #[test]
    fn test_test_harness_plugin_name() {
        let plugin = TestHarnessPlugin;
        assert_eq!(plugin.name(), "aura-test-harness");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_test_harness_plugin_configure_registers_task() {
        let plugin = TestHarnessPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        // 应该注册了 run-tests 任务
        let has_run_tests = ctx.tasks.iter().any(|t| t.name == "run-tests");
        assert!(has_run_tests);

        let run_tests = ctx.tasks.iter().find(|t| t.name == "run-tests").unwrap();
        assert_eq!(run_tests.depends_on, vec!["compile-test".to_string()]);
    }

    #[test]
    fn test_test_harness_plugin_no_duplicate_task() {
        let plugin = TestHarnessPlugin;
        let mut ctx = PluginContext::new_default();

        // 先手动添加 run-tests 任务
        ctx.add_task(TaskDefinition {
            name: "run-tests".to_string(),
            description: "已有".to_string(),
            kind: TaskKind::Test,
            depends_on: vec!["compile-test".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });

        // 配置不应该重复添加
        assert!(plugin.configure(&mut ctx).is_ok());
        let count = ctx.tasks.iter().filter(|t| t.name == "run-tests").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_test_harness_plugin_execute() {
        let plugin = TestHarnessPlugin;
        let ctx = PluginContext::new_default();
        let result = plugin.execute("run-tests", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("测试框架"));
    }

    #[test]
    fn test_watch_plugin_name() {
        let plugin = WatchPlugin;
        assert_eq!(plugin.name(), "aura-watch");
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }

    #[test]
    fn test_watch_plugin_configure_registers_task() {
        let plugin = WatchPlugin;
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());

        let has_watch = ctx.tasks.iter().any(|t| t.name == "watch");
        assert!(has_watch);
    }

    #[test]
    fn test_watch_plugin_no_duplicate_task() {
        let plugin = WatchPlugin;
        let mut ctx = PluginContext::new_default();
        ctx.add_task(TaskDefinition {
            name: "watch".to_string(),
            description: "已有".to_string(),
            kind: TaskKind::Watch,
            depends_on: vec!["resolve".to_string()],
            inputs: TaskInputs::default(),
            outputs: TaskOutputs::default(),
        });

        assert!(plugin.configure(&mut ctx).is_ok());
        let count = ctx.tasks.iter().filter(|t| t.name == "watch").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_convention_plugins_default_manifest() {
        let manifest = default_manifest("test");
        let plugins = convention_plugins(&manifest);
        // 默认情况下所有约定插件都应该启用
        assert!(plugins.len() >= 3);
        assert!(plugins.iter().any(|p| p.name() == "aura-stdlib"));
        assert!(plugins.iter().any(|p| p.name() == "aura-test-harness"));
        assert!(plugins.iter().any(|p| p.name() == "aura-watch"));
    }

    #[test]
    fn test_convention_plugins_disabled() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-stdlib = false
aura-test-harness = false
aura-watch = false
"#;
        let manifest: crate::manifest::LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = convention_plugins(&manifest);
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_convention_plugins_partial_disable() {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[plugins]
aura-stdlib = false
aura-test-harness = true
aura-watch = false
"#;
        let manifest: crate::manifest::LoomManifest = toml::from_str(toml_str).unwrap();
        let plugins = convention_plugins(&manifest);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name(), "aura-test-harness");
    }

    #[test]
    fn test_all_convention_plugins_configure() {
        let manifest = default_manifest("test");
        let plugins = convention_plugins(&manifest);
        let mut ctx = PluginContext::new_default();

        for plugin in &plugins {
            assert!(
                plugin.configure(&mut ctx).is_ok(),
                "Plugin {} should configure successfully",
                plugin.name()
            );
        }

        // 检查所有插件都激活了
        assert!(ctx.is_plugin_active("aura-stdlib"));
        assert!(ctx.is_plugin_active("aura-test-harness"));
        assert!(ctx.is_plugin_active("aura-watch"));
    }
}
