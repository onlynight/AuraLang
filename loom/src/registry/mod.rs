//! [Phase B6] 仓库管理（Registry）
//!
//! 实现中心包注册表 REST API 客户端 + 本地注册表管理。
//! 对应设计文档 §13 仓库管理。
//!
//! 架构：
//! ```text
//! RegistryClient ──→ REST API ──→ 远程仓库
//! LocalRegistry  ──→ 文件系统 ──→ ~/.aura/registry/
//! ```

pub mod client;
pub mod local;

pub use client::{PackageInfo, RegistryClient, SearchResult, VersionInfo};
pub use local::{CacheEntry, CacheIndex, LocalRegistry, RegistryStats};
