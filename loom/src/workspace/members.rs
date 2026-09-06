//! [Phase B5.3] Workspace members 解析 + 共享缓存 + 依赖图
//!
// 功能：
//! - 解析 `[workspace] members` 配置
//! - 构建成员依赖图（类 Cargo workspace）
// - 共享缓存（跨成员复用依赖）
//! - `--member` / `--with-deps` 选择策略
//!
//! 对应设计文档 §14.2 Workspace 模式。

use crate::error::LoomError;
use crate::manifest::{LoomManifest, WorkspaceConfig};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// Workspace 成员信息
#[derive(Debug, Clone)]
pub struct WorkspaceMember {
    /// 相对路径（相对于 workspace 根目录）
    pub path: PathBuf,
    /// 是否默认构建
    pub is_default: bool,
    /// 包名（从 aura.toml 读取）
    pub name: String,
    /// 版本
    pub version: String,
}

/// Workspace 成员选择策略
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberSelection {
    /// 构建所有默认成员
    AllDefaults,
    /// 构建指定成员
    Named(String),
    /// 构建指定成员 + 其依赖
    WithDeps(String),
    /// 构建所有成员
    All,
}

/// Workspace 管理器
pub struct Workspace {
    /// Workspace 根目录
    root_dir: PathBuf,
    /// 成员列表
    members: Vec<WorkspaceMember>,
    /// 按名称索引
    by_name: HashMap<String, usize>,
    /// 成员依赖关系（name -> 依赖的成员名列表）
    member_deps: HashMap<String, Vec<String>>,
}

impl Workspace {
    /// 从 manifest 和根目录创建 Workspace
    ///
    /// 解析 `[workspace]` 配置，扫描成员目录，读取每个成员的 aura.toml。
    pub fn from_manifest(manifest: &LoomManifest, root_dir: &Path) -> Result<Self, LoomError> {
        let workspace_config = manifest
            .workspace
            .as_ref()
            .ok_or_else(|| LoomError::Config("aura.toml 缺少 [workspace] 配置".to_string()))?;

        let mut members = Vec::new();
        let mut by_name = HashMap::new();

        for member_path_str in &workspace_config.members {
            let member_path = root_dir.join(member_path_str);
            let member_manifest_path = member_path.join("aura.toml");

            if !member_manifest_path.exists() {
                return Err(LoomError::Config(format!(
                    "Workspace 成员 '{}' 缺少 aura.toml: {}",
                    member_path_str,
                    member_manifest_path.display()
                )));
            }

            let member_manifest = crate::manifest::parse::parse_from_file(&member_manifest_path)?;

            let is_default = workspace_config.default_members.iter().any(|d| d == member_path_str);

            let member = WorkspaceMember {
                path: PathBuf::from(member_path_str),
                is_default,
                name: member_manifest.name.clone(),
                version: member_manifest.version.clone(),
            };

            by_name.insert(member.name.clone(), members.len());
            members.push(member);
        }

        // 解析成员间依赖关系
        let member_deps = resolve_member_deps(&members, root_dir)?;

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            members,
            by_name,
            member_deps,
        })
    }

    /// 从目录创建 Workspace（自动发现）
    ///
    /// 扫描指定目录下的所有子目录，寻找含有 aura.toml 的项目。
    pub fn auto_discover(root_dir: &Path) -> Result<Self, LoomError> {
        let mut members = Vec::new();
        let mut by_name = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(root_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("aura.toml");
                    if manifest_path.exists() {
                        let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;
                        let rel_path = path.strip_prefix(root_dir).unwrap_or(&path);

                        let member = WorkspaceMember {
                            path: rel_path.to_path_buf(),
                            is_default: true,
                            name: manifest.name.clone(),
                            version: manifest.version.clone(),
                        };

                        by_name.insert(member.name.clone(), members.len());
                        members.push(member);
                    }
                }
            }
        }

        let member_deps = resolve_member_deps(&members, root_dir)?;

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            members,
            by_name,
            member_deps,
        })
    }

    /// 获取所有成员
    pub fn members(&self) -> &[WorkspaceMember] {
        &self.members
    }

    /// 获取成员数量
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 按名称查找成员
    pub fn find_member(&self, name: &str) -> Option<&WorkspaceMember> {
        self.by_name.get(name).map(|&idx| &self.members[idx])
    }

    /// 获取所有成员名称
    pub fn member_names(&self) -> Vec<String> {
        self.members.iter().map(|m| m.name.clone()).collect()
    }

    /// 获取默认成员
    pub fn default_members(&self) -> Vec<&WorkspaceMember> {
        self.members.iter().filter(|m| m.is_default).collect()
    }

    /// 解析成员依赖关系（name -> 依赖的成员名列表）
    pub fn member_deps(&self) -> &HashMap<String, Vec<String>> {
        &self.member_deps
    }

    /// 获取成员的依赖成员（直接依赖）
    pub fn deps_of(&self, name: &str) -> Vec<String> {
        self.member_deps.get(name).cloned().unwrap_or_default()
    }

    /// 获取成员的所有传递依赖（递归）
    pub fn all_deps_of(&self, name: &str) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(name.to_string());

        while let Some(current) = queue.pop_front() {
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Some(deps) = self.member_deps.get(&current) {
                for dep in deps {
                    if !visited.contains(dep) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        visited.remove(name);
        visited.into_iter().collect()
    }

    /// 按选择策略获取要构建的成员
    pub fn selected_members(&self, selection: &MemberSelection) -> Vec<&WorkspaceMember> {
        match selection {
            MemberSelection::AllDefaults => self.default_members(),
            MemberSelection::All => self.members().iter().collect(),
            MemberSelection::Named(name) => self.find_member(name).into_iter().collect(),
            MemberSelection::WithDeps(name) => {
                let mut members = Vec::new();
                if let Some(member) = self.find_member(name) {
                    members.push(member);
                    for dep_name in self.all_deps_of(name) {
                        if let Some(dep) = self.find_member(&dep_name) {
                            members.push(dep);
                        }
                    }
                }
                members
            }
        }
    }

    /// 解析选择策略（从 CLI 参数）
    pub fn resolve_selection(member: Option<&str>, with_deps: bool) -> MemberSelection {
        match member {
            None => MemberSelection::AllDefaults,
            Some(name) => {
                if with_deps {
                    MemberSelection::WithDeps(name.to_string())
                } else {
                    MemberSelection::Named(name.to_string())
                }
            }
        }
    }

    /// 获取 workspace 根目录
    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    /// 获取成员的项目目录（完整路径）
    pub fn member_dir(&self, name: &str) -> Option<PathBuf> {
        self.find_member(name).map(|m| self.root_dir.join(&m.path))
    }

    /// 验证 workspace 配置有效性
    pub fn validate(&self) -> Result<(), LoomError> {
        if self.members.is_empty() {
            return Err(LoomError::Config("Workspace 无成员".to_string()));
        }

        // 检查重复名称
        let mut names: HashSet<&str> = HashSet::new();
        for member in &self.members {
            if !names.insert(member.name.as_str()) {
                return Err(LoomError::Config(format!(
                    "Workspace 成员名称重复: {}",
                    member.name
                )));
            }
        }

        // 检查依赖是否存在
        for (name, deps) in &self.member_deps {
            for dep in deps {
                if self.by_name.contains_key(dep) {
                    continue;
                }
                return Err(LoomError::Config(format!(
                    "Workspace 成员 '{}' 依赖 '{}'，但该成员不在 workspace 中",
                    name, dep
                )));
            }
        }

        Ok(())
    }

    /// 检查两个成员之间是否有循环依赖
    pub fn has_cycle(&self) -> bool {
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();
        for name in &self.member_names() {
            if !visited.contains(name) {
                if self.dfs_has_cycle(name, &mut visited, &mut stack) {
                    return true;
                }
            }
        }
        false
    }

    fn dfs_has_cycle(
        &self,
        name: &str,
        visited: &mut HashSet<String>,
        stack: &mut HashSet<String>,
    ) -> bool {
        visited.insert(name.to_string());
        stack.insert(name.to_string());

        for dep in self.deps_of(name) {
            if !visited.contains(&dep) {
                if self.dfs_has_cycle(&dep, visited, stack) {
                    return true;
                }
            } else if stack.contains(&dep) {
                return true;
            }
        }

        stack.remove(name);
        false
    }
}

/// 解析成员间依赖关系
///
/// 扫描每个成员的 aura.toml，查找指向其他 workspace 成员的依赖。
fn resolve_member_deps(
    members: &[WorkspaceMember],
    root_dir: &Path,
) -> Result<HashMap<String, Vec<String>>, LoomError> {
    let mut deps: HashMap<String, Vec<String>> = HashMap::new();
    let member_names: HashSet<&str> = members.iter().map(|m| m.name.as_str()).collect();

    for member in members {
        let member_manifest_path = root_dir.join(&member.path).join("aura.toml");
        if !member_manifest_path.exists() {
            continue;
        }

        let member_manifest = crate::manifest::parse::parse_from_file(&member_manifest_path)?;

        let mut member_dep_names = Vec::new();
        for dep in &member_manifest.dependencies {
            if member_names.contains(dep.name.as_str()) {
                member_dep_names.push(dep.name.clone());
            }
        }
        for dep in &member_manifest.compile_dependencies {
            if member_names.contains(dep.name.as_str()) {
                member_dep_names.push(dep.name.clone());
            }
        }
        for dep in &member_manifest.runtime_dependencies {
            if member_names.contains(dep.name.as_str()) {
                member_dep_names.push(dep.name.clone());
            }
        }

        deps.insert(member.name.clone(), member_dep_names);
    }

    Ok(deps)
}

/// 从 manifest 加载 Workspace（便捷函数）
pub fn load_workspace(manifest: &LoomManifest, root_dir: &Path) -> Result<Workspace, LoomError> {
    Workspace::from_manifest(manifest, root_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{DepConfig, Dependency, LoomManifest};
    use tempfile::TempDir;

    fn write_manifest(dir: impl AsRef<Path>, name: &str, version: &str) {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).unwrap();
        let manifest = LoomManifest {
            schema_version: "2.0".to_string(),
            name: name.to_string(),
            version: version.to_string(),
            entry: "src/main.aura".to_string(),
            ..Default::default()
        };
        std::fs::write(
            dir.join("aura.toml"),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn write_manifest_with_deps(dir: impl AsRef<Path>, name: &str, deps: &[&str]) {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).unwrap();
        let mut dependencies = Vec::new();
        for dep in deps {
            dependencies.push(Dependency {
                name: dep.to_string(),
                version: ">= 1.0".to_string(),
                config: DepConfig::Implementation,
                ..Default::default()
            });
        }
        let manifest = LoomManifest {
            schema_version: "2.0".to_string(),
            name: name.to_string(),
            version: "1.0.0".to_string(),
            entry: "src/main.aura".to_string(),
            dependencies,
            ..Default::default()
        };
        std::fs::write(
            dir.join("aura.toml"),
            toml::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn make_workspace_manifest(members: &[&str], defaults: &[&str]) -> LoomManifest {
        let mut manifest = LoomManifest::default();
        manifest.workspace = Some(WorkspaceConfig {
            members: members.iter().map(|s| s.to_string()).collect(),
            default_members: defaults.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        });
        manifest
    }

    #[test]
    fn test_workspace_from_manifest() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        write_manifest(tmp.path().join("app"), "app", "1.0.0");

        let manifest = make_workspace_manifest(
            &[
                "core", "app",
            ],
            &["core"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        assert_eq!(workspace.len(), 2);
        assert!(workspace.find_member("core").is_some());
        assert!(workspace.find_member("app").is_some());
        assert!(workspace.find_member("core").unwrap().is_default);
        assert!(!workspace.find_member("app").unwrap().is_default);
    }

    #[test]
    fn test_workspace_auto_discover() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("a"), "a", "1.0.0");
        write_manifest(tmp.path().join("b"), "b", "1.0.0");

        let workspace = Workspace::auto_discover(tmp.path()).unwrap();
        assert_eq!(workspace.len(), 2);
        assert!(workspace.find_member("a").is_some());
        assert!(workspace.find_member("b").is_some());
    }

    #[test]
    fn test_workspace_member_deps() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        write_manifest_with_deps(tmp.path().join("app"), "app", &["core"]);

        let manifest = make_workspace_manifest(
            &[
                "core", "app",
            ],
            &["core"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        assert!(workspace.deps_of("app").contains(&"core".to_string()));
        assert!(workspace.deps_of("core").is_empty());
    }

    #[test]
    fn test_workspace_all_deps_of() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("a"), "a", "1.0.0");
        write_manifest_with_deps(tmp.path().join("b"), "b", &["a"]);
        write_manifest_with_deps(tmp.path().join("c"), "c", &["b"]);

        let manifest = make_workspace_manifest(
            &[
                "a", "b", "c",
            ],
            &["a"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let all_deps = workspace.all_deps_of("c");
        assert!(all_deps.contains(&"b".to_string()));
        assert!(all_deps.contains(&"a".to_string()));
    }

    #[test]
    fn test_workspace_selected_all_defaults() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        write_manifest(tmp.path().join("app"), "app", "1.0.0");

        let manifest = make_workspace_manifest(
            &[
                "core", "app",
            ],
            &["core"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let selected = workspace.selected_members(&MemberSelection::AllDefaults);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "core");
    }

    #[test]
    fn test_workspace_selected_named() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        write_manifest(tmp.path().join("app"), "app", "1.0.0");

        let manifest = make_workspace_manifest(
            &[
                "core", "app",
            ],
            &["core"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let selected = workspace.selected_members(&MemberSelection::Named("app".to_string()));
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "app");
    }

    #[test]
    fn test_workspace_selected_with_deps() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        write_manifest_with_deps(tmp.path().join("app"), "app", &["core"]);

        let manifest = make_workspace_manifest(
            &[
                "core", "app",
            ],
            &["core"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let selected = workspace.selected_members(&MemberSelection::WithDeps("app".to_string()));
        assert!(selected.iter().any(|m| m.name == "app"));
        assert!(selected.iter().any(|m| m.name == "core"));
    }

    #[test]
    fn test_workspace_validate_ok() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        let manifest = make_workspace_manifest(&["core"], &["core"]);
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();
        assert!(workspace.validate().is_ok());
    }

    #[test]
    fn test_workspace_validate_empty() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_workspace_manifest(&[], &[]);
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();
        assert!(workspace.validate().is_err());
    }

    #[test]
    fn test_workspace_no_cycle() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("a"), "a", "1.0.0");
        write_manifest_with_deps(tmp.path().join("b"), "b", &["a"]);
        write_manifest_with_deps(tmp.path().join("c"), "c", &["b"]);

        let manifest = make_workspace_manifest(
            &[
                "a", "b", "c",
            ],
            &["a"],
        );
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();
        assert!(!workspace.has_cycle());
    }

    #[test]
    fn test_workspace_resolve_selection() {
        assert_eq!(
            Workspace::resolve_selection(None, false),
            MemberSelection::AllDefaults
        );
        assert_eq!(
            Workspace::resolve_selection(Some("app"), false),
            MemberSelection::Named("app".to_string())
        );
        assert_eq!(
            Workspace::resolve_selection(Some("app"), true),
            MemberSelection::WithDeps("app".to_string())
        );
    }

    #[test]
    fn test_workspace_member_dir() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        let manifest = make_workspace_manifest(&["core"], &["core"]);
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let dir = workspace.member_dir("core").unwrap();
        assert!(dir.exists());
    }

    #[test]
    fn test_workspace_member_names() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("a"), "a", "1.0.0");
        write_manifest(tmp.path().join("b"), "b", "1.0.0");
        let manifest = make_workspace_manifest(&["a", "b"], &["a", "b"]);
        let workspace = Workspace::from_manifest(&manifest, tmp.path()).unwrap();

        let names = workspace.member_names();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    #[test]
    fn test_load_workspace_convenience() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path().join("core"), "core", "1.0.0");
        let manifest = make_workspace_manifest(&["core"], &["core"]);
        let workspace = load_workspace(&manifest, tmp.path()).unwrap();
        assert_eq!(workspace.len(), 1);
    }

    #[test]
    fn test_workspace_missing_member_manifest() {
        let tmp = TempDir::new().unwrap();
        let manifest = make_workspace_manifest(&["nonexistent"], &[]);
        let result = Workspace::from_manifest(&manifest, tmp.path());
        assert!(result.is_err());
    }
}
