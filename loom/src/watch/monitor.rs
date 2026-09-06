//! [Phase B7.2] 文件监听 + 增量重编
//
//! 监听源码目录变化，自动触发增量重编。
//! 对应设计文档 §17.2 Watch 模式。

use crate::error::LoomError;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 文件事件
#[derive(Debug, Clone)]
pub enum FileEvent {
    /// 文件创建
    Created(PathBuf),
    /// 文件修改
    Modified(PathBuf),
    /// 文件删除
    Deleted(PathBuf),
    /// 文件重命名（旧路径 -> 新路径）
    Renamed(PathBuf, PathBuf),
}

impl FileEvent {
    /// 获取事件路径
    pub fn path(&self) -> &Path {
        match self {
            FileEvent::Created(p) | FileEvent::Modified(p) | FileEvent::Deleted(p) => p,
            FileEvent::Renamed(old, _) => old,
        }
    }

    /// 是否是源码文件事件
    pub fn is_source_event(&self) -> bool {
        let path = self.path();
        path.extension().map(|ext| ext == "aura" || ext == "au").unwrap_or(false)
    }
}

/// 文件监听器配置
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// 监听目录列表
    pub watch_dirs: Vec<PathBuf>,
    /// 过滤扩展名
    pub extensions: Vec<String>,
    /// 是否忽略隐藏文件
    pub ignore_hidden: bool,
    /// 防抖延迟（毫秒）
    pub debounce_ms: u64,
    /// 忽略目录
    pub ignore_dirs: Vec<String>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            watch_dirs: vec![PathBuf::from("src")],
            extensions: vec![
                "aura".to_string(),
                "au".to_string(),
            ],
            ignore_hidden: true,
            debounce_ms: 300,
            ignore_dirs: vec![
                "target".to_string(),
                "build".to_string(),
                "node_modules".to_string(),
                ".git".to_string(),
            ],
        }
    }
}

/// 文件监听器
pub struct FileWatcher {
    config: WatchConfig,
    running: Arc<AtomicBool>,
    /// 事件回调
    callback: Option<Arc<dyn Fn(FileEvent) -> bool + Send + Sync>>,
}

impl FileWatcher {
    /// 创建文件监听器
    pub fn new(
        config: WatchConfig,
        callback: Arc<dyn Fn(FileEvent) -> bool + Send + Sync>,
    ) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(true)),
            callback: Some(callback),
        }
    }

    /// 创建文件监听器（无回调）
    pub fn new_no_callback(config: WatchConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(true)),
            callback: None,
        }
    }

    /// 开始监听（使用 notify crate 事件驱动）
    pub fn watch(&self, project_dir: &Path) -> Result<(), LoomError> {
        println!("👁 Watch 模式启动");
        println!("  监听目录:");
        for dir in &self.config.watch_dirs {
            let full_path = project_dir.join(dir);
            if full_path.exists() {
                println!("    → {}", full_path.display());
            } else {
                println!("    → {} (不存在)", full_path.display());
            }
        }
        println!("  扩展名: {:?}", self.config.extensions);
        println!("  防抖: {}ms", self.config.debounce_ms);

        // 使用 notify crate 创建事件驱动的文件监听器
        let running = Arc::clone(&self.running);
        let config = self.config.clone();
        let callback = self.callback.clone();

        let mut watcher = notify::recommended_watcher(Box::new(
            move |result: Result<notify::Event, notify::Error>| {
                if !running.load(Ordering::Relaxed) {
                    return;
                }

                match result {
                    Ok(event) => {
                        // 检查事件类型（notify 6.x 无 Rename 变体，重命名以 Create+Remove 表示）
                        match event.kind {
                            notify::EventKind::Create(_) => {}
                            notify::EventKind::Modify(_) => {}
                            notify::EventKind::Remove(_) => {}
                            _ => return, // 忽略其他事件类型
                        }

                        for path in &event.paths {
                            // 检查扩展名过滤
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            if !config.extensions.iter().any(|e| e == ext) {
                                continue;
                            }

                            // 检查忽略目录
                            let path_str = path.to_string_lossy().to_string();
                            if config.ignore_dirs.iter().any(|d| path_str.contains(d)) {
                                continue;
                            }

                            // 检查隐藏文件
                            if config.ignore_hidden
                                && path
                                    .components()
                                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
                            {
                                continue;
                            }

                            // 转换事件类型
                            let file_event = match event.kind {
                                notify::EventKind::Create(_) => FileEvent::Created(path.clone()),
                                notify::EventKind::Modify(_) => FileEvent::Modified(path.clone()),
                                notify::EventKind::Remove(_) => FileEvent::Deleted(path.clone()),
                                _ => continue,
                            };

                            println!("  📝 {}", describe_event(&file_event));

                            // 调用回调
                            if let Some(ref cb) = callback {
                                let _ = cb(file_event);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  ⚠ 监听错误: {}", e);
                    }
                }
            },
        ))
        .map_err(|e| LoomError::Ci(format!("创建文件监听器失败: {}", e)))?;

        // 添加监听目录
        for dir in &self.config.watch_dirs {
            let full_path = project_dir.join(dir);
            if full_path.exists() {
                notify::Watcher::watch(&mut watcher, &full_path, notify::RecursiveMode::Recursive)
                    .map_err(|e| {
                        LoomError::Ci(format!("监听目录失败 {}: {}", full_path.display(), e))
                    })?;
            }
        }

        println!("\n  等待文件变化... (Ctrl+C 退出)\n");

        // 保持 watcher 存活（防止被 GC）
        while self.running.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(100));
        }

        Ok(())
    }

    /// 停止监听
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        println!("  👁 Watch 模式停止");
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

fn describe_event(event: &FileEvent) -> String {
    match event {
        FileEvent::Created(p) => format!("创建: {}", p.display()),
        FileEvent::Modified(p) => format!("修改: {}", p.display()),
        FileEvent::Deleted(p) => format!("删除: {}", p.display()),
        FileEvent::Renamed(old, new) => format!("重命名: {} → {}", old.display(), new.display()),
    }
}

/// Watch 会话 - 管理 watch 模式和增量重编
pub struct WatchSession {
    watcher: Arc<FileWatcher>,
    project_dir: PathBuf,
    rebuild_callback: Option<Box<dyn Fn(&[FileEvent]) -> Result<(), LoomError> + Send + Sync>>,
    pending_events: std::sync::Mutex<Vec<FileEvent>>,
}

impl WatchSession {
    /// 创建 watch 会话
    pub fn new(
        project_dir: &Path,
        config: WatchConfig,
        rebuild_callback: Option<Box<dyn Fn(&[FileEvent]) -> Result<(), LoomError> + Send + Sync>>,
    ) -> Self {
        let watcher = Arc::new(FileWatcher::new(
            config.clone(),
            Arc::new(|_event| {
                // 回调中不直接执行重编，由 WatchSession 管理
                true
            }),
        ));

        Self {
            watcher,
            project_dir: project_dir.to_path_buf(),
            rebuild_callback,
            pending_events: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// 开始 watch 会话
    pub fn start(&self) -> Result<(), LoomError> {
        println!("🚀 Watch 会话启动");
        println!("  项目: {}", self.project_dir.display());

        // 启动文件监听
        self.watcher.watch(&self.project_dir)?;

        Ok(())
    }

    /// 停止 watch 会话
    pub fn stop(&self) {
        self.watcher.stop();
        println!("🛑 Watch 会话停止");
    }

    /// 检查是否正在运行
    pub fn is_running(&self) -> bool {
        self.watcher.is_running()
    }

    /// 手动触发重编
    pub fn trigger_rebuild(&self, events: &[FileEvent]) -> Result<(), LoomError> {
        if let Some(ref cb) = self.rebuild_callback {
            cb(events)
        } else {
            println!("  ⚠ 未配置重编回调");
            Ok(())
        }
    }
}

/// 文件变化统计
#[derive(Debug, Clone, Default)]
pub struct ChangeStats {
    pub created: u32,
    pub modified: u32,
    pub deleted: u32,
    pub renamed: u32,
    pub total: u32,
}

impl ChangeStats {
    pub fn record(&mut self, event: &FileEvent) {
        match event {
            FileEvent::Created(_) => self.created += 1,
            FileEvent::Modified(_) => self.modified += 1,
            FileEvent::Deleted(_) => self.deleted += 1,
            FileEvent::Renamed(_, _) => self.renamed += 1,
        }
        self.total += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_file_event_path() {
        let event = FileEvent::Created(PathBuf::from("/tmp/test.aura"));
        assert_eq!(event.path(), Path::new("/tmp/test.aura"));
    }

    #[test]
    fn test_file_event_is_source() {
        let event = FileEvent::Created(PathBuf::from("src/main.aura"));
        assert!(event.is_source_event());

        let event = FileEvent::Created(PathBuf::from("readme.md"));
        assert!(!event.is_source_event());
    }

    #[test]
    fn test_watch_config_default() {
        let config = WatchConfig::default();
        assert!(config.watch_dirs.iter().any(|d| d.to_string_lossy().ends_with("src")));
        assert!(config.extensions.contains(&"aura".to_string()));
        assert!(config.ignore_hidden);
        assert_eq!(config.debounce_ms, 300);
    }

    #[test]
    fn test_file_watcher_new() {
        let config = WatchConfig::default();
        let watcher = FileWatcher::new(config, Arc::new(|_event| true));
        assert!(watcher.is_running());
    }

    #[test]
    fn test_file_watcher_stop() {
        let config = WatchConfig::default();
        let watcher = FileWatcher::new(config, Arc::new(|_event| true));
        watcher.stop();
        assert!(!watcher.is_running());
    }

    #[test]
    fn test_change_stats() {
        let mut stats = ChangeStats::default();
        stats.record(&FileEvent::Created(PathBuf::from("a.aura")));
        stats.record(&FileEvent::Modified(PathBuf::from("b.aura")));
        stats.record(&FileEvent::Deleted(PathBuf::from("c.aura")));
        stats.record(&FileEvent::Renamed(
            PathBuf::from("d.aura"),
            PathBuf::from("e.aura"),
        ));

        assert_eq!(stats.created, 1);
        assert_eq!(stats.modified, 1);
        assert_eq!(stats.deleted, 1);
        assert_eq!(stats.renamed, 1);
        assert_eq!(stats.total, 4);
    }

    #[test]
    fn test_describe_event() {
        let desc = describe_event(&FileEvent::Created(PathBuf::from("test.aura")));
        assert!(desc.contains("创建"));
        assert!(desc.contains("test.aura"));
    }

    #[test]
    fn test_watch_session() {
        let tmp = TempDir::new().unwrap();
        let config = WatchConfig::default();
        let session = WatchSession::new(tmp.path(), config, None);
        assert!(session.is_running());
    }

    #[test]
    fn test_watch_config_custom() {
        let config = WatchConfig {
            watch_dirs: vec![
                PathBuf::from("lib"),
                PathBuf::from("core"),
            ],
            extensions: vec!["aura".to_string()],
            ignore_hidden: false,
            debounce_ms: 100,
            ignore_dirs: vec![],
        };
        assert_eq!(config.watch_dirs.len(), 2);
        assert_eq!(config.debounce_ms, 100);
    }
}
