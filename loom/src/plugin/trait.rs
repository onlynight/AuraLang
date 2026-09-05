//! [Phase B4.1] BuildPlugin trait 定义
//!
//! 所有构建插件（约定 / 显式 / 外部）均实现此 trait。
//! 对应设计文档 §9.1。

use crate::error::LoomError;
use crate::plugin::{PluginKind, TaskResult};
use super::context::PluginContext;

/// 构建插件接口
///
/// 插件是构建系统的可扩展单元。每个插件可以：
/// - 注册新任务到任务图（通过 `configure`）
/// - 执行自定义任务（通过 `execute`）
/// - 提供默认配置（约定插件）
///
/// 对应设计文档 §9.1 `BuildPlugin` trait。
///
/// 示例：
/// ```text
/// impl BuildPlugin for DocGenPlugin {
///     fn name(&self) -> &str { "aura-doc-gen" }
///     fn version(&self) -> &str { "1.0.0" }
///     fn kind(&self) -> PluginKind { PluginKind::Explicit }
///     fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError> { ... }
///     fn execute(&self, task_name: &str, ctx: &PluginContext) -> Result<TaskResult, LoomError> { ... }
/// }
/// ```
pub trait BuildPlugin: Send + Sync {
    /// 插件名称（唯一标识）
    fn name(&self) -> &str;

    /// 插件版本（SemVer）
    fn version(&self) -> &str;

    /// 插件类型
    fn kind(&self) -> PluginKind;

    /// 插件描述（可选）
    fn description(&self) -> Option<&str> {
        None
    }

    /// 初始化：注册任务、约定、默认配置
    ///
    /// 在构建开始前调用一次。插件在此方法中向 `ctx.tasks` 添加任务定义，
    /// 向 `ctx.source_sets` 修改源码集配置，或设置默认构建选项。
    ///
    /// 约定插件在此方法中注入默认行为（如注册 `run-tests` 任务）。
    /// 显式插件在此方法中注册自定义任务（如 `doc`, `fmt`）。
    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError>;

    /// 执行任务（插件自定义任务）
    ///
    /// 当任务图的调度器执行到 `TaskKind::Plugin(name)` 任务时，
    /// 执行器查找该任务名对应的插件并调用此方法。
    ///
    /// `task_name` 参数用于插件支持多个任务（如 aura-format 可同时注册 `fmt` 和 `fmt-check`）。
    fn execute(
        &self,
        task_name: &str,
        ctx: &PluginContext,
    ) -> Result<TaskResult, LoomError>;
}

/// 插件构建器 trait（可选辅助 trait）
///
/// 为插件提供链式构建接口，简化复杂插件的初始化。
///
/// 示例：
/// ```text
/// let plugin = DocGenPlugin::builder()
///     .output_dir("docs")
///     .format("markdown")
///     .build();
/// ```
pub trait PluginBuilder<T: BuildPlugin> {
    /// 构建插件实例
    fn build(self) -> T;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::PluginKind;
    use crate::plugin::context::PluginContext;

    /// 测试用最小插件实现
    struct TestPlugin {
        name: String,
        version: String,
        kind: PluginKind,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                version: "0.1.0".to_string(),
                kind: PluginKind::Explicit,
            }
        }
    }

    impl BuildPlugin for TestPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        fn kind(&self) -> PluginKind {
            self.kind
        }

        fn configure(&self, _ctx: &mut PluginContext) -> Result<(), LoomError> {
            Ok(())
        }

        fn execute(
            &self,
            _task_name: &str,
            _ctx: &PluginContext,
        ) -> Result<TaskResult, LoomError> {
            Ok(TaskResult::ok(format!("{} executed", self.name)))
        }
    }

    #[test]
    fn test_plugin_name() {
        let plugin = TestPlugin::new("test-plugin");
        assert_eq!(plugin.name(), "test-plugin");
    }

    #[test]
    fn test_plugin_version() {
        let plugin = TestPlugin::new("test-plugin");
        assert_eq!(plugin.version(), "0.1.0");
    }

    #[test]
    fn test_plugin_kind() {
        let plugin = TestPlugin::new("test-plugin");
        assert_eq!(plugin.kind(), PluginKind::Explicit);
    }

    #[test]
    fn test_plugin_execute() {
        let plugin = TestPlugin::new("test-plugin");
        let ctx = PluginContext::new_default();
        let result = plugin.execute("test-task", &ctx).unwrap();
        assert!(result.success);
        assert!(result.output.contains("test-plugin"));
    }

    #[test]
    fn test_plugin_configure_ok() {
        let plugin = TestPlugin::new("test-plugin");
        let mut ctx = PluginContext::new_default();
        assert!(plugin.configure(&mut ctx).is_ok());
    }

    #[test]
    fn test_plugin_description_default_none() {
        let plugin = TestPlugin::new("test-plugin");
        assert!(plugin.description().is_none());
    }

    /// 带描述的插件测试
    struct DescribedPlugin;

    impl BuildPlugin for DescribedPlugin {
        fn name(&self) -> &str { "described-plugin" }
        fn version(&self) -> &str { "1.0.0" }
        fn kind(&self) -> PluginKind { PluginKind::Convention }
        fn description(&self) -> Option<&str> { Some("A plugin with description") }
        fn configure(&self, _ctx: &mut PluginContext) -> Result<(), LoomError> { Ok(()) }
        fn execute(&self, _task_name: &str, _ctx: &PluginContext) -> Result<TaskResult, LoomError> {
            Ok(TaskResult::ok("executed"))
        }
    }

    #[test]
    fn test_plugin_description_some() {
        let plugin = DescribedPlugin;
        assert_eq!(plugin.description(), Some("A plugin with description"));
        assert_eq!(plugin.kind(), PluginKind::Convention);
    }
}
