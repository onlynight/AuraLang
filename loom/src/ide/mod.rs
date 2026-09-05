//! [Phase L7] IDE 集成：aura-project.json 导出

pub mod model;

/// IDE 项目模型
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdeProjectModel {
    /// 模型版本
    pub version: u32,
    /// 编译器版本
    pub compiler_version: String,
    /// 项目根目录
    pub root_dir: String,
    /// 源码集
    pub source_sets: std::collections::HashMap<String, IdeSourceSet>,
    /// 依赖
    pub dependencies: Vec<IdeDependency>,
    /// 任务列表
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdeSourceSet {
    pub source_dirs: Vec<String>,
    pub modules: Vec<IdeModule>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdeModule {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub entry: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdeDependency {
    pub name: String,
    pub version: String,
    pub path: String,
}

