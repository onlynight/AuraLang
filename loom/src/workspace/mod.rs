//! [Phase B5] 多项目管理（Workspace）
//!
//! Workspace 模式允许在单一配置中管理多个项目（类 Cargo workspace）。
//! 对应设计文档 §14.2。

pub mod members;

pub use members::{MemberSelection, Workspace, WorkspaceMember};
