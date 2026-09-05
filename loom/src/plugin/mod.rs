//! [Phase B4] 插件系统：BuildPlugin trait + 约定插件 + 显式插件 + 外部插件
//!
//! 插件系统架构：
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │            PluginRegistry                    │
//! │  ┌──────────┐ ┌──────────┐ ┌──────────┐   │
//! │  │ 约定插件  │ │ 显式插件  │ │ 外部插件  │   │
//! │  │(auto)    │ │(explicit)│ │(.so/.dll) │   │
//! │  └──────────┘ └──────────┘ └──────────┘   │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! 生命周期：
//! 1. 插件发现（从 aura.toml [plugins] 表 + 约定插件自动激活）
//! 2. 插件加载（按 kind 分派：Convention / Explicit / External）
//! 3. 插件初始化（configure() 注册任务、约定、默认配置）
//! 4. 插件执行（execute() 处理自定义任务）
//!
//! 对应设计文档 §9（插件系统）和 §18 Phase B4。

pub mod r#trait;
pub mod context;
pub mod convention;
pub mod explicit;
pub mod external;
pub mod registry;

// ═══════════════════════════════════════════════════════════════════════════════
// 核心类型（与 executor 共享，重导出常用类型）
// ═══════════════════════════════════════════════════════════════════════════════

/// 插件类型
///
/// 对应设计文档 §9.1 `PluginKind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginKind {
    /// 约定插件：自动激活，提供默认行为
    /// 示例：aura-stdlib, aura-test-harness, aura-watch
    Convention,
    /// 显式插件：需手动启用（在 aura.toml [plugins] 中设为 true）
    /// 示例：aura-doc-gen, aura-format, aura-aot, aura-ci
    Explicit,
    /// 外部插件：编译为 .so/.dll 的动态库
    /// 通过 libloading 加载，通过 C ABI 调用
    External,
}

impl std::fmt::Display for PluginKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginKind::Convention => write!(f, "convention"),
            PluginKind::Explicit => write!(f, "explicit"),
            PluginKind::External => write!(f, "external"),
        }
    }
}

/// 插件执行结果
///
/// 与 `task::executor::TaskResult` 不同，本类型是插件内部使用的简化结果，
/// 由 Executor 翻译为 executor 的 TaskResult。
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// 执行是否成功
    pub success: bool,
    /// 执行输出消息
    pub output: String,
    /// 生成的产物路径
    pub artifacts: Vec<std::path::PathBuf>,
}

impl TaskResult {
    /// 创建成功结果
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            output: message.into(),
            artifacts: Vec::new(),
        }
    }

    /// 创建成功结果（带产物）
    pub fn ok_with_artifacts(message: impl Into<String>, artifacts: Vec<std::path::PathBuf>) -> Self {
        Self {
            success: true,
            output: message.into(),
            artifacts,
        }
    }

    /// 创建失败结果
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            output: message.into(),
            artifacts: Vec::new(),
        }
    }
}

/// 构建环境信息（传递给插件）
#[derive(Debug, Clone)]
pub struct BuildEnvironment {
    /// 项目根目录
    pub project_dir: std::path::PathBuf,
    /// 构建输出目录
    pub out_dir: std::path::PathBuf,
    /// 缓存目录
    pub cache_dir: std::path::PathBuf,
    /// 远程缓存 URL（如有）
    pub remote_cache_url: Option<String>,
    /// 目标平台三元组（如有）
    pub target: Option<String>,
    /// 优化级别
    pub opt_level: u8,
    /// 是否启用调试信息
    pub debug: bool,
    /// 是否启用并行构建
    pub parallel: bool,
    /// 活跃 Profile 名称
    pub active_profile: Option<String>,
}

/// 插件注册表（重导出）
pub use registry::PluginRegistry;
