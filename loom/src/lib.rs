//! aura-loom: Aura 语言构建系统
//!
//! 纯 TOML 声明式配置、任务 DAG 引擎、增量构建、插件扩展。
//! 详见 `docs/构建系统设计.md`。
//!
//! # 职责边界（第四阶段收敛后）
//!
//! loom 仅负责**构建编排**，不拥有语言语义或包生态。
//! 包生态职责（install / publish / deps / package / verify）由 `aura` 承担。
//!
//! ## 与 aura 的对接协议
//!
//! 1. **编译调用模式**
//!    - **库链接（默认）**：loom 依赖 `compiler` crate，直接调用 `compile_source()` 等 API
//!    - **子进程调用（备选）**：通过 `Command::new("aura").arg("build")` 调用 aura 二进制，
//!      用于隔离构建环境或第三方构建场景
//!
//! 2. **`.auz` 制品 API 归属**
//!    - aura 拥有 builder / reader（`compiler/src/auz/`）
//!    - loom 仅作为调用方，不重新实现制品格式
//!
//! 3. **本地包缓存目录约定**
//!    - 唯一位置：`~/.aura/cache/packages/`
//!    - loom 的 `resolve` 命令仅**读取**该目录，不写入
//!    - aura 的 `install` / `update` 命令负责**写入**该目录
//!
//! 详见 `docs/多进程与CLI架构分析报告.md` §4。

pub mod cache;
pub mod ci;
pub mod cli;
pub mod dep;
pub mod error;
pub mod ffi;
pub mod ide;
pub mod lifecycle;
pub mod manifest;
pub mod package;
pub mod plugin;
pub mod registry;
pub mod sourceset;
pub mod stdlib;
pub mod task;
pub mod watch;
pub mod workspace;
pub mod wrapper;

pub use error::LoomError;
pub use manifest::parse::{
    default_manifest, minimal_toml, parse_from_file, parse_from_str, parse_merged,
};
pub use manifest::{DepConfig, Dependency, LoomManifest, SourceSetConfig, default};
pub use task::TaskGraph;
