//! [Phase L1-L6] 依赖管理：版本求解 + 冲突解析 + 锁文件 + BOM

pub mod resolver;
pub mod graph;
pub mod lock;
pub mod bom;

use crate::manifest::{Dependency, DepConfig};

/// 解析后的依赖条目（含配置类型）
#[derive(Debug, Clone)]
pub struct ResolvedDependency {
    /// 包名
    pub name: String,
    /// 版本约束
    pub version: String,
    /// 配置类型
    pub config: DepConfig,
    /// 特性列表
    pub features: Vec<String>,
    /// 目标平台限制
    pub target: Option<String>,
}

impl ResolvedDependency {
    pub fn from_dependency(dep: &Dependency) -> Self {
        Self {
            name: dep.name.clone(),
            version: dep.version.clone(),
            config: dep.config,
            features: dep.features.clone(),
            target: dep.target.clone(),
        }
    }
}

