//! aura-loom: Aura 语言构建系统
//!
//! 纯 TOML 声明式配置、任务 DAG 引擎、增量构建、插件扩展。
//! 详见 `docs/构建系统设计.md`。

pub mod cli;
pub mod manifest;
pub mod task;
pub mod lifecycle;
pub mod sourceset;
pub mod dep;
pub mod cache;
pub mod plugin;
pub mod workspace;
pub mod wrapper;
pub mod registry;
pub mod ci;
pub mod ide;
pub mod watch;
pub mod error;

pub use error::LoomError;
pub use task::TaskGraph;
pub use manifest::{LoomManifest, Dependency, DepConfig, SourceSetConfig};
pub use manifest::parse::{parse_from_file, parse_from_str, default_manifest};
