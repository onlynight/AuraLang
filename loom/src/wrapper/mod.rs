//! [Phase L5] 构建包装器（aura-wrapper）

pub mod config;
pub mod installer;

/// Wrapper 配置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

