//! 统一错误类型

use thiserror::Error;

/// loom 构建系统统一错误
#[derive(Debug, Error)]
pub enum LoomError {
    #[error("配置错误: {0}")]
    Config(String),

    #[error("任务错误: {0}")]
    Task(String),

    #[error("依赖错误: {0}")]
    Dependency(String),

    #[error("缓存错误: {0}")]
    Cache(String),

    #[error("插件错误: {0}")]
    Plugin(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML 解析错误: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("序列化错误: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("未实现: {0}")]
    Unimplemented(String),
}
