//! [Phase B7.1] .loom/aura-project.json 导出
//
//! 为 IDE 提供项目信息 JSON 文件，支持：
//! - 项目结构识别
//! - 源码导航
//! - 任务图可视化
//! - 依赖关系展示
//
//! 配置文件位于 `.loom/` 目录下，默认加入 .gitignore。
//! 对应设计文档 §17.1 IDE 集成。

use crate::error::LoomError;
use crate::manifest::LoomManifest;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// IDE 项目文件（.loom/aura-project.json）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdeProject {
    /// 文件格式版本
    pub schema_version: String,
    /// 项目名称
    pub name: String,
    /// 项目版本
    pub version: String,
    /// 项目根目录（相对路径）
    pub root: String,
    /// 入口文件
    pub entry: String,
    /// 源码根目录列表
    #[serde(default)]
    pub source_roots: Vec<String>,
    /// 依赖列表
    #[serde(default)]
    pub dependencies: Vec<IdeDependency>,
    /// 构建配置
    pub build: IdeBuildConfig,
    /// 任务列表
    #[serde(default)]
    pub tasks: Vec<IdeTask>,
    /// 插件列表
    #[serde(default)]
    pub plugins: Vec<IdePlugin>,
    /// 文件列表（可选，用于大型项目索引）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<IdeFile>>,
}

/// IDE 依赖信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdeDependency {
    pub name: String,
    pub version: String,
    /// 依赖配置类型
    pub config: String,
    /// 是否本地路径依赖
    pub is_local: bool,
}

/// IDE 构建配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdeBuildConfig {
    /// 目标目录
    pub out_dir: Option<String>,
    /// 缓存目录
    pub cache_dir: Option<String>,
    /// 并行任务数
    pub parallel: Option<usize>,
    /// 默认 profile
    pub profile: Option<String>,
    /// 目标平台
    pub target: Option<String>,
    /// 优化级别
    pub opt_level: Option<String>,
}

/// IDE 任务信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdeTask {
    pub name: String,
    /// 任务类型
    pub kind: String,
    /// 依赖的任务列表
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 任务描述
    pub description: Option<String>,
}

/// IDE 插件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdePlugin {
    pub name: String,
    pub version: Option<String>,
    pub kind: String,
    pub description: Option<String>,
}

/// IDE 文件信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct IdeFile {
    /// 相对路径
    pub path: String,
    /// 文件类型
    pub kind: String,
    /// 是否源码文件
    pub is_source: bool,
    /// 行数
    pub line_count: Option<usize>,
}

impl IdeProject {
    /// 从 manifest 生成 IDE 项目文件
    pub fn from_manifest(manifest: &LoomManifest, project_dir: &Path) -> Self {
        let mut deps = Vec::new();
        for dep in &manifest.dependencies {
            deps.push(IdeDependency {
                name: dep.name.clone(),
                version: dep.version.clone(),
                config: dep.config.to_string(),
                is_local: false,
            });
        }
        for dep in &manifest.compile_dependencies {
            deps.push(IdeDependency {
                name: dep.name.clone(),
                version: dep.version.clone(),
                config: "compileOnly".to_string(),
                is_local: false,
            });
        }
        for dep in &manifest.runtime_dependencies {
            deps.push(IdeDependency {
                name: dep.name.clone(),
                version: dep.version.clone(),
                config: "runtimeOnly".to_string(),
                is_local: false,
            });
        }
        for dep in &manifest.dev_dependencies {
            deps.push(IdeDependency {
                name: dep.name.clone(),
                version: dep.version.clone(),
                config: "dev".to_string(),
                is_local: false,
            });
        }

        // 发现源码目录
        let mut source_roots = Vec::new();
        for dir in &[
            "src", "lib", "core", "app",
        ] {
            if project_dir.join(dir).is_dir() {
                source_roots.push(dir.to_string());
            }
        }

        Self {
            schema_version: "1.0".to_string(),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            root: project_dir.to_string_lossy().to_string(),
            entry: manifest.entry.clone(),
            source_roots,
            dependencies: deps,
            build: IdeBuildConfig {
                out_dir: Some(manifest.build.out_dir.clone()),
                cache_dir: Some(manifest.build.cache_dir.clone()),
                parallel: Some(if manifest.build.parallel { 4 } else { 1 }),
                profile: None,
                target: manifest.build.target.clone(),
                opt_level: Some(manifest.build.opt_level.to_string()),
            },
            tasks: Vec::new(),
            plugins: Vec::new(),
            files: None,
        }
    }

    /// 添加任务信息
    pub fn with_task(mut self, task: IdeTask) -> Self {
        self.tasks.push(task);
        self
    }

    /// 添加插件信息
    pub fn with_plugin(mut self, plugin: IdePlugin) -> Self {
        self.plugins.push(plugin);
        self
    }

    /// 添加文件列表
    pub fn with_files(mut self, files: Vec<IdeFile>) -> Self {
        self.files = Some(files);
        self
    }

    /// 序列化为 JSON 字符串
    pub fn to_json(&self) -> Result<String, LoomError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| LoomError::Ide(format!("Serialization failed: {}", e)))
    }

    /// 写入文件（自动创建父目录）
    pub fn write_to(&self, path: &Path) -> Result<(), LoomError> {
        if let Some(parent) = path.parent() {
            if !parent.to_string_lossy().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| LoomError::Ide(format!("Failed to create directory: {}", e)))?;
            }
        }
        let json = self.to_json()?;
        std::fs::write(path, json)
            .map_err(|e| LoomError::Ide(format!("Failed to write file: {}", e)))
    }

    /// 从 JSON 字符串解析
    pub fn from_json(content: &str) -> Result<Self, LoomError> {
        serde_json::from_str(content).map_err(|e| LoomError::Ide(format!("Parse failed: {}", e)))
    }

    /// 从文件加载
    pub fn from_file(path: &Path) -> Result<Self, LoomError> {
        if !path.exists() {
            return Err(LoomError::Ide(format!(
                "IDE project file not found: {}",
                path.display()
            )));
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| LoomError::Ide(format!("Failed to read file: {}", e)))?;
        Self::from_json(&content)
    }

    /// 获取 IDE 项目文件默认路径（位于 .loom/ 目录）
    pub fn default_path(project_dir: &Path) -> PathBuf {
        project_dir.join(".loom").join("aura-project.json")
    }
}

/// 文件扫描器 - 扫描项目文件并生成 IDE 文件列表
pub struct FileScanner {
    project_dir: PathBuf,
    include_sources: bool,
}

impl FileScanner {
    /// 创建文件扫描器
    pub fn new(project_dir: &Path, include_sources: bool) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            include_sources,
        }
    }

    /// 扫描项目文件
    pub fn scan(&self) -> Result<Vec<IdeFile>, LoomError> {
        let mut files = Vec::new();

        for entry in walkdir::WalkDir::new(&self.project_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let rel_path =
                path.strip_prefix(&self.project_dir).unwrap_or(path).to_string_lossy().to_string();

            // 跳过隐藏文件和 target 目录
            if rel_path.starts_with('.')
                || rel_path.contains("target/")
                || rel_path.contains("node_modules/")
            {
                continue;
            }

            let is_source = self.is_source_file(rel_path.as_str());
            if !self.include_sources && !is_source {
                continue;
            }

            let kind = file_kind(rel_path.as_str());
            let line_count = if is_source { read_line_count(path).ok() } else { None };

            files.push(IdeFile {
                path: rel_path,
                kind,
                is_source,
                line_count,
            });
        }

        Ok(files)
    }

    fn is_source_file(&self, path: &str) -> bool {
        path.ends_with(".aura")
            || path.ends_with(".au")
            || path.ends_with(".auz")
            || path.contains("/src/")
            || path.contains("\\src\\")
    }
}

fn file_kind(path: &str) -> String {
    if path.ends_with(".aura") || path.ends_with(".au") {
        "aura-source".to_string()
    } else if path.ends_with(".auz") {
        "aura-package".to_string()
    } else if path.ends_with(".toml") {
        "config".to_string()
    } else if path.ends_with(".json") {
        "json".to_string()
    } else if path.ends_with(".rs") {
        "rust".to_string()
    } else {
        "other".to_string()
    }
}

fn read_line_count(path: &Path) -> Result<usize, std::io::Error> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.lines().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{DepConfig, Dependency};
    use tempfile::TempDir;

    fn make_manifest() -> LoomManifest {
        let mut manifest = LoomManifest::default();
        manifest.name = "test-project".to_string();
        manifest.version = "1.0.0".to_string();
        manifest.entry = "src/main.aura".to_string();
        manifest.dependencies.push(Dependency {
            name: "dep1".to_string(),
            version: ">=1.0".to_string(),
            config: DepConfig::Implementation,
            ..Default::default()
        });
        manifest
    }

    #[test]
    fn test_ide_project_from_manifest() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        let manifest = make_manifest();

        let project = IdeProject::from_manifest(&manifest, tmp.path());
        assert_eq!(project.name, "test-project");
        assert_eq!(project.version, "1.0.0");
        assert_eq!(project.dependencies.len(), 1);
        assert!(project.source_roots.contains(&"src".to_string()));
    }

    #[test]
    fn test_ide_project_to_json() {
        let project = IdeProject::default();
        let json = project.to_json().unwrap();
        assert!(json.contains("schema-version"));
    }

    #[test]
    fn test_ide_project_from_json() {
        let json = r#"{
            "schema-version": "1.0",
            "name": "test",
            "version": "1.0.0",
            "root": "/tmp/test",
            "entry": "src/main.aura",
            "dependencies": [],
            "build": {},
            "tasks": [],
            "plugins": []
        }"#;
        let project = IdeProject::from_json(json).unwrap();
        assert_eq!(project.name, "test");
        assert_eq!(project.schema_version, "1.0");
    }

    #[test]
    fn test_ide_project_write_and_read() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("aura-project.json");
        let manifest = make_manifest();
        let project = IdeProject::from_manifest(&manifest, tmp.path());

        project.write_to(&path).unwrap();
        assert!(path.exists());

        let loaded = IdeProject::from_file(&path).unwrap();
        assert_eq!(loaded.name, "test-project");
    }

    #[test]
    fn test_ide_project_with_task() {
        let project = IdeProject::default().with_task(IdeTask {
            name: "build".to_string(),
            kind: "compile".to_string(),
            depends_on: vec![],
            description: Some("Build project".to_string()),
        });
        assert_eq!(project.tasks.len(), 1);
        assert_eq!(project.tasks[0].name, "build");
    }

    #[test]
    fn test_ide_project_with_plugin() {
        let project = IdeProject::default().with_plugin(IdePlugin {
            name: "aura-stdlib".to_string(),
            version: Some("1.0.0".to_string()),
            kind: "Convention".to_string(),
            description: Some("Standard library".to_string()),
        });
        assert_eq!(project.plugins.len(), 1);
    }

    #[test]
    fn test_file_scanner() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.aura"), "fn main() {}").unwrap();
        std::fs::write(tmp.path().join("aura.toml"), "name = \"test\"").unwrap();

        let scanner = FileScanner::new(tmp.path(), true);
        let files = scanner.scan().unwrap();
        assert!(!files.is_empty());
        assert!(files.iter().any(|f| f.path.contains("main.aura")));
    }

    #[test]
    fn test_file_scanner_sources_only() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::write(tmp.path().join("src/main.aura"), "fn main() {}").unwrap();
        std::fs::write(tmp.path().join("aura.toml"), "name = \"test\"").unwrap();

        let scanner = FileScanner::new(tmp.path(), false);
        let files = scanner.scan().unwrap();
        // 只返回源码文件
        assert!(files.iter().all(|f| f.is_source));
        assert!(files.iter().any(|f| f.path.contains("main.aura")));
    }

    #[test]
    fn test_file_kind() {
        assert_eq!(file_kind("src/main.aura"), "aura-source");
        assert_eq!(file_kind("lib/core.au"), "aura-source");
        assert_eq!(file_kind("package.auz"), "aura-package");
        assert_eq!(file_kind("aura.toml"), "config");
        assert_eq!(file_kind("config.json"), "json");
        assert_eq!(file_kind("build.rs"), "rust");
        assert_eq!(file_kind("README.md"), "other");
    }

    #[test]
    fn test_ide_dependency_serde() {
        let json = r#"{
            "name": "dep1",
            "version": ">=1.0",
            "config": "implementation",
            "is-local": false
        }"#;
        let dep: IdeDependency = serde_json::from_str(json).unwrap();
        assert_eq!(dep.name, "dep1");
        assert!(!dep.is_local);
    }

    #[test]
    fn test_ide_build_config_serde() {
        let json = r#"{
            "out-dir": "build",
            "parallel": 4,
            "profile": "release"
        }"#;
        let config: IdeBuildConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.out_dir.unwrap(), "build");
        assert_eq!(config.parallel.unwrap(), 4);
    }

    #[test]
    fn test_ide_task_serde() {
        let json = r#"{
            "name": "build",
            "kind": "compile",
            "depends-on": ["resolve"],
            "description": "Build project"
        }"#;
        let task: IdeTask = serde_json::from_str(json).unwrap();
        assert_eq!(task.name, "build");
        assert_eq!(task.depends_on.len(), 1);
    }

    #[test]
    fn test_ide_plugin_serde() {
        let json = r#"{
            "name": "aura-stdlib",
            "version": "1.0.0",
            "kind": "Convention",
            "description": "Standard library"
        }"#;
        let plugin: IdePlugin = serde_json::from_str(json).unwrap();
        assert_eq!(plugin.name, "aura-stdlib");
    }

    #[test]
    fn test_ide_file_serde() {
        let json = r#"{
            "path": "src/main.aura",
            "kind": "aura-source",
            "is-source": true,
            "line-count": 42
        }"#;
        let file: IdeFile = serde_json::from_str(json).unwrap();
        assert_eq!(file.path, "src/main.aura");
        assert_eq!(file.line_count.unwrap(), 42);
    }

    #[test]
    fn test_ide_project_default_path() {
        let path = IdeProject::default_path(Path::new("/tmp/project"));
        let s = path.to_string_lossy();
        assert!(
            s.ends_with(".loom/aura-project.json") || s.ends_with(".loom\\aura-project.json"),
            "default path should be under .loom/, got: {}",
            s
        );
    }

    #[test]
    fn test_ide_project_with_files() {
        let files = vec![IdeFile {
            path: "src/main.aura".to_string(),
            kind: "aura-source".to_string(),
            is_source: true,
            line_count: Some(10),
        }];
        let project = IdeProject::default().with_files(files);
        assert!(project.files.is_some());
        assert_eq!(project.files.unwrap().len(), 1);
    }

    #[test]
    fn test_ide_project_complex_manifest() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("lib")).unwrap();
        std::fs::create_dir_all(tmp.path().join("app")).unwrap();

        let mut manifest = make_manifest();
        manifest.compile_dependencies.push(Dependency {
            name: "compile-dep".to_string(),
            version: ">=2.0".to_string(),
            config: DepConfig::CompileOnly,
            ..Default::default()
        });

        let project = IdeProject::from_manifest(&manifest, tmp.path());
        assert_eq!(project.dependencies.len(), 2);
        assert!(project.source_roots.contains(&"src".to_string()));
        assert!(project.source_roots.contains(&"lib".to_string()));
        assert!(project.source_roots.contains(&"app".to_string()));
    }
}
