//! [Phase L3] 构建缓存：本地 + 远程 + fingerprint

pub mod local;
pub mod remote;
pub mod fingerprint;

/// 缓存键
#[derive(Debug, Clone)]
pub struct CacheKey {
    /// SHA-256 哈希
    pub hash: String,
    /// 关联任务名
    pub task_name: String,
}

/// 缓存条目
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub key: CacheKey,
    /// 产物路径列表
    pub artifacts: Vec<std::path::PathBuf>,
    /// 创建时间戳
    pub created_at: u64,
    /// 过期时间戳（0 = 永不过期）
    pub expires_at: u64,
}

