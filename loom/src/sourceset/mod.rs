//! [Phase L2] 源码集：main / test / bench + 目录扫描 + import 解析

pub mod discover;
pub mod resolve;

use crate::manifest::{ModuleInfo, SourceSetConfig};
use std::path::PathBuf;

/// 源码集运行时实例
///
/// 从 Manifest 的 SourceSetConfig 创建，路径已解析为绝对路径。
#[derive(Debug, Clone)]
pub struct SourceSet {
    /// 源码集名称（main / test / bench）
    pub name: String,
    /// 源码目录列表（绝对路径）
    pub source_dirs: Vec<PathBuf>,
    /// 资源目录列表（绝对路径）
    pub resource_dirs: Vec<PathBuf>,
    /// 包含模式
    pub include: Vec<String>,
    /// 排除模式
    pub exclude: Vec<String>,
    /// 依赖的其他源码集名称
    pub depends_on: Vec<String>,
    /// 发现的模块列表（编译时填充）
    pub modules: Vec<ModuleInfo>,
}

impl SourceSet {
    /// 从配置创建源码集
    pub fn from_config(config: &SourceSetConfig, name: &str, project_dir: &std::path::Path) -> Self {
        let source_dirs = config
            .source_dirs
            .iter()
            .map(|d| project_dir.join(d))
            .collect();
        let resource_dirs = config
            .resource_dirs
            .iter()
            .map(|d| project_dir.join(d))
            .collect();

        Self {
            name: name.to_string(),
            source_dirs,
            resource_dirs,
            include: config.include.clone(),
            exclude: config.exclude.clone(),
            depends_on: config.depends_on.clone(),
            modules: Vec::new(),
        }
    }

    /// 主源码集（默认 src/）
    pub fn main(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::main_default(), "main", project_dir)
    }

    /// 测试源码集（默认 test/）
    pub fn test(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::test_default(), "test", project_dir)
    }

    /// 基准源码集（默认 bench/）
    pub fn bench(project_dir: &std::path::Path) -> Self {
        Self::from_config(&SourceSetConfig::bench_default(), "bench", project_dir)
    }

    /// 判断源文件是否属于本源码集
    pub fn contains_file(&self, path: &PathBuf) -> bool {
        self.source_dirs
            .iter()
            .any(|d| path.starts_with(d))
            && path.extension().map(|e| e == "aura").unwrap_or(false)
    }
}

