//! [Phase B5] 构建包装器（aura-wrapper）
//!
//! 构建包装器确保团队成员使用相同版本的 loom 构建工具。
//! 对应设计文档 §14.1。

pub mod config;
pub mod installer;

use std::path::PathBuf;

/// Wrapper 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct WrapperConfig {
    /// 编译器下载 URL
    pub distribution_url: String,
    /// 本地缓存目录
    pub wrapper_cache_dir: String,
    /// 校验和（SHA-256）
    pub checksum: String,
    /// 安装超时（秒）
    pub timeout: u64,
}

impl WrapperConfig {
    /// 获取缓存目录 PathBuf
    pub fn cache_dir_path(&self) -> PathBuf {
        PathBuf::from(&self.wrapper_cache_dir)
    }
}

impl Default for WrapperConfig {
    fn default() -> Self {
        Self {
            distribution_url: String::new(),
            wrapper_cache_dir: String::new(),
            checksum: String::new(),
            timeout: 300,
        }
    }
}
