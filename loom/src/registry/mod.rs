//! [Phase L6] 仓库管理：REST API + 本地注册表

pub mod protocol;
pub mod local;

/// 仓库类型
#[derive(Debug, Clone, Copy)]
pub enum RepoKind {
    /// 中心化制品注册表
    Registry,
    /// Git 仓库（源码）
    Git,
    /// 本地路径（源码）
    Path,
    /// 本地 .auz 文件
    File,
    /// 本地安装目录
    Local,
}

