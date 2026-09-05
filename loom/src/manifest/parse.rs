//! [Phase L1/B1.5] aura.toml 解析

use std::path::Path;

use crate::error::LoomError;
use crate::manifest::{
    BuildConfig, BuildConfigOverride, LoomManifest, PackageOptions, PluginConfig, ProfileConfig,
    RepositoryConfig, ResourceConfig, SourceSetConfig,
};
use super::validate;

/// 从文件路径解析 `aura.toml`
pub fn parse_from_file(path: &Path) -> Result<LoomManifest, LoomError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        LoomError::Config(format!("无法读取 {}: {}", path.display(), e))
    })?;
    parse_from_str(&content)
}

/// 从 TOML 字符串解析
pub fn parse_from_str(s: &str) -> Result<LoomManifest, LoomError> {
    let manifest: LoomManifest =
        toml::from_str(s).map_err(|e| LoomError::Config(format!("TOML 解析错误: {}", e)))?;

    // 验证必填字段
    if manifest.name.is_empty() {
        return Err(LoomError::Config("缺少必填字段: name".to_string()));
    }
    if manifest.version.is_empty() {
        return Err(LoomError::Config("缺少必填字段: version".to_string()));
    }

    Ok(manifest)
}

/// 从文件解析并验证
pub fn parse_and_validate(path: &Path) -> Result<LoomManifest, LoomError> {
    let manifest = parse_from_file(path)?;
    let errors = validate::validate_manifest(&manifest);
    if !errors.is_empty() {
        return Err(LoomError::Config(format!(
            "配置验证失败:\n{}",
            errors.join("\n")
        )));
    }
    Ok(manifest)
}

/// 创建默认 Manifest（用于 `loom new`）
pub fn default_manifest(name: &str) -> LoomManifest {
    LoomManifest {
        schema_version: "2.0".to_string(),
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: Some(format!("{} 项目", name)),
        authors: Vec::new(),
        license: Some("MIT".to_string()),
        repository: Some(format!("https://github.com/aura-lang/{}.git", name)),
        entry: "src/main.aura".to_string(),
        exports: vec!["main".to_string()],
        library: false,
        dependencies: Vec::new(),
        compile_dependencies: Vec::new(),
        runtime_dependencies: Vec::new(),
        dev_dependencies: Vec::new(),
        build_dependencies: Vec::new(),
        build: BuildConfig {
            source_sets: std::collections::HashMap::from([
                ("main".to_string(), SourceSetConfig::main_default()),
                ("test".to_string(), SourceSetConfig::test_default()),
            ]),
            ..BuildConfig::default()
        },
        plugins: PluginConfig::default(),
        profiles: std::collections::HashMap::from([
            ("debug".to_string(), ProfileConfig { activate: true, build: None }),
            (
                "release".to_string(),
                ProfileConfig {
                    activate: false,
                    build: Some(BuildConfigOverride {
                        opt_level: Some(3),
                        debug: Some(false),
                        emit_package: Some(true),
                        ..Default::default()
                    }),
                },
            ),
        ]),
        repositories: RepositoryConfig {
            central: Some("https://registry.aura-lang.dev".to_string()),
            publish: None,
            custom: std::collections::HashMap::new(),
        },
        workspace: None,
        resources: ResourceConfig::default(),
        package: PackageOptions::default(),
        tasks: Vec::new(),
    }
}

