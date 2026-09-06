// [Phase B7] Watch 模式
//
// 文件监听 + 增量重编，对应设计文档 §17.2。

pub mod monitor;

pub use monitor::{ChangeStats, FileEvent, FileWatcher, WatchConfig, WatchSession};
