//! [Phase L4] 插件系统：BuildPlugin trait + 约定插件 + 外部 .so

pub mod r#trait;
pub mod context;
pub mod convention;
pub mod external;

use crate::error::LoomError;

/// 插件类型
#[derive(Debug, Clone, Copy)]
pub enum PluginKind {
    /// 约定插件：自动激活
    Convention,
    /// 显式插件：需手动启用
    Explicit,
    /// 外部插件：编译为 .so/.dll
    External,
}

/// 构建插件 trait
pub trait BuildPlugin {
    /// 插件名称
    fn name(&self) -> &str;
    /// 插件版本
    fn version(&self) -> &str;
    /// 插件类型
    fn kind(&self) -> PluginKind;
    /// 初始化：注册任务、约定、默认配置
    fn configure(&self, ctx: &mut PluginContext) -> Result<(), LoomError>;
    /// 执行任务（插件自定义任务）
    fn execute(&self, task_name: &str, ctx: &PluginContext) -> Result<TaskResult, LoomError>;
}

/// 插件上下文
#[derive(Debug)]
pub struct PluginContext {
    /// 项目配置
    pub manifest: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    /// 源码集列表
    pub source_sets: Vec<String>,
    /// 任务列表
    pub tasks: Vec<String>,
    /// 构建环境
    pub cache_dir: std::path::PathBuf,
    pub out_dir: std::path::PathBuf,
}

impl PluginContext {
    pub fn new() -> Self {
        Self {
            manifest: std::sync::Arc::new(()),
            source_sets: Vec::new(),
            tasks: Vec::new(),
            cache_dir: std::path::PathBuf::from("target/cache"),
            out_dir: std::path::PathBuf::from("target/build"),
        }
    }
}

/// 任务执行结果
#[derive(Debug)]
pub struct TaskResult {
    pub success: bool,
    pub output: String,
}

