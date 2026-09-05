//! [Phase L7] Watch 模式：文件监听 + 增量重编

pub mod monitor;

/// Watch 配置
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// 监听目录列表
    pub watch_dirs: Vec<std::path::PathBuf>,
    /// 监听文件扩展名
    pub extensions: Vec<String>,
    /// 防抖时间（毫秒）
    pub debounce_ms: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            watch_dirs: vec![std::path::PathBuf::from("src")],
            extensions: vec!["aura".to_string()],
            debounce_ms: 300,
        }
    }
}

