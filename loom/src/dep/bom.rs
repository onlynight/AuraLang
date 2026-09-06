//! [Phase B6] BOM 版本锁定（dependency-management）
//!
// 实现 Workspace 级别的依赖版本统一管理。
// 当多个子项目依赖同一包时，统一解析为 BOM 声明的版本。
//! 对应设计文档 §8.3 依赖锁定（BOM 机制）。

use crate::manifest::{Dependency, WorkspaceConfig};
use std::collections::HashMap;

/// BOM 版本锁定管理器
///
/// 从 Workspace 配置中读取 `dependency-management`，
/// 提供依赖版本覆盖查询。
pub struct Bom {
    /// 版本锁定表：包名 → 版本约束
    locks: HashMap<String, String>,
}

impl Bom {
    /// 从 Workspace 配置创建 BOM
    pub fn from_workspace(workspace: &Option<WorkspaceConfig>) -> Self {
        let mut locks = HashMap::new();

        if let Some(ws) = workspace {
            for (name, version) in &ws.dependency_management {
                locks.insert(name.clone(), version.clone());
            }
        }

        Self { locks }
    }

    /// 检查是否有版本锁定
    pub fn has_lock(&self, name: &str) -> bool {
        self.locks.contains_key(name)
    }

    /// 获取锁定版本
    pub fn locked_version(&self, name: &str) -> Option<&str> {
        self.locks.get(name).map(|s| s.as_str())
    }

    /// 覆盖依赖版本（如果有 BOM 锁定）
    ///
    /// 返回修改后的依赖列表，BOM 锁定的依赖会被替换为锁定版本。
    pub fn resolve_dependencies(&self, dependencies: &[Dependency]) -> Vec<Dependency> {
        dependencies
            .iter()
            .map(|dep| {
                if let Some(locked_version) = self.locked_version(&dep.name) {
                    Dependency {
                        version: locked_version.to_string(),
                        ..dep.clone()
                    }
                } else {
                    dep.clone()
                }
            })
            .collect()
    }

    /// 获取所有锁定的包名
    pub fn locked_packages(&self) -> Vec<&str> {
        self.locks.keys().map(|s| s.as_str()).collect()
    }

    /// 获取锁定表大小
    pub fn len(&self) -> usize {
        self.locks.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.locks.is_empty()
    }

    /// 合并多个 BOM（后者覆盖前者）
    pub fn merge(self, other: Bom) -> Bom {
        let mut locks = self.locks;
        locks.extend(other.locks);
        Bom { locks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::DepConfig;

    #[test]
    fn test_bom_from_workspace() {
        let ws = WorkspaceConfig {
            dependency_management: HashMap::from([
                ("aura-json".to_string(), "^1.2".to_string()),
                ("aura-http".to_string(), ">=2.0".to_string()),
            ]),
            ..Default::default()
        };

        let bom = Bom::from_workspace(&Some(ws));
        assert_eq!(bom.len(), 2);
        assert!(bom.has_lock("aura-json"));
        assert_eq!(bom.locked_version("aura-json"), Some("^1.2"));
    }

    #[test]
    fn test_bom_empty() {
        let bom = Bom::from_workspace(&None);
        assert!(bom.is_empty());
        assert!(!bom.has_lock("any"));
    }

    #[test]
    fn test_bom_resolve_dependencies() {
        let bom = Bom {
            locks: HashMap::from([("aura-json".to_string(), "^1.2".to_string())]),
        };

        let deps = vec![
            Dependency {
                name: "aura-json".to_string(),
                version: ">=1.0".to_string(),
                config: DepConfig::Implementation,
                ..Default::default()
            },
            Dependency {
                name: "other-pkg".to_string(),
                version: "^2.0".to_string(),
                config: DepConfig::Implementation,
                ..Default::default()
            },
        ];

        let resolved = bom.resolve_dependencies(&deps);
        assert_eq!(resolved[0].version, "^1.2");
        assert_eq!(resolved[1].version, "^2.0");
    }

    #[test]
    fn test_bom_merge() {
        let bom1 = Bom {
            locks: HashMap::from([("a".to_string(), "1.0".to_string())]),
        };
        let bom2 = Bom {
            locks: HashMap::from([
                ("a".to_string(), "2.0".to_string()),
                ("b".to_string(), "1.0".to_string()),
            ]),
        };

        let merged = bom1.merge(bom2);
        assert_eq!(merged.locked_version("a"), Some("2.0"));
        assert_eq!(merged.locked_version("b"), Some("1.0"));
    }
}
