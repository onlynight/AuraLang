//! [Phase L5] 多项目管理（Workspace）

pub mod members;

/// Workspace 成员信息
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// 相对路径
    pub path: std::path::PathBuf,
    /// 是否默认构建
    pub is_default: bool,
    /// 包名
    pub name: String,
}

