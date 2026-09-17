//! 统一错误类型

use thiserror::Error;

/// loom 构建系统统一错误
#[derive(Debug, Error)]
pub enum LoomError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Task error: {0}")]
    Task(String),

    #[error("Dependency error: {0}")]
    Dependency(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("Serialization error: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("Not implemented: {0}")]
    Unimplemented(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("CI configuration error: {0}")]
    Ci(String),

    #[error("IDE error: {0}")]
    Ide(String),
}
