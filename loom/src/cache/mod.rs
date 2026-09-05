//! [Phase B3] 构建缓存：本地 + 远程 + fingerprint
//!
//! 两级缓存架构：
//!   1. 本地缓存（target/cache/）— fingerprint + artifacts
//!   2. 远程缓存（HTTP Build Cache）— 跨项目/跨 CI 共享
//!
//! 缓存查找顺序：本地 → 远程 → 执行 → 写入两级缓存
//!
//! B3.1: 本地缓存 artifact 管理
//! B3.2: 缓存失效策略
//! B3.3: 远程缓存协议（HTTP）
//! B3.4: 缓存上传/下载 + 共享策略
//! B3.5: --no-cache / --clean 参数

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
    /// 过期时间戳（0 = 永不超过）
    pub expires_at: u64,
}
