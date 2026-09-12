//! [Phase L1/B1.5] aura.toml 解析
//!
//! 支持"内置默认 + 项目覆盖"的合并语义：
//!   - 项目 aura.toml 不存在 → 返回内置默认 Manifest
//!   - 项目 aura.toml 存在 → TOML 值级别深度合并覆盖内置默认
//!
//! 详见 `loom/docs/默认配置合并设计.md`。

use std::path::Path;

use toml::Value;

use super::{default, validate};
use crate::error::LoomError;
use crate::manifest::LoomManifest;

/// 从 `Value` 合并解析 Manifest（内部 helper）。
///
/// 合并规则：`base = 内置默认`，`overlay = project_value`。
/// 详见 [`default::merge`]。
pub fn parse_merged(project_value: Value) -> Result<LoomManifest, LoomError> {
    let merged = default::merge(default::default_value(), project_value);
    let manifest: LoomManifest = Value::try_into(merged)
        .map_err(|e| LoomError::Config(format!("Manifest deserialization error: {}", e)))?;

    // 验证必填字段（合并后仍检查，以防用户显式设置空值）
    if manifest.name.is_empty() {
        return Err(LoomError::Config(
            "Missing required field: name".to_string(),
        ));
    }
    if manifest.version.is_empty() {
        return Err(LoomError::Config(
            "Missing required field: version".to_string(),
        ));
    }

    Ok(manifest)
}

/// 从文件路径解析 `aura.toml`（合并内置默认）。
///
/// - 文件存在 → 读取内容，与内置默认合并后返回
/// - 文件不存在 → 返回内置默认 Manifest（带占位 name/version，不报错）
pub fn parse_from_file(path: &Path) -> Result<LoomManifest, LoomError> {
    if !path.exists() {
        // 无 aura.toml → 回退到内置默认
        return Ok(default::default_manifest());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| LoomError::Config(format!("Failed to read {}: {}", path.display(), e)))?;

    let project_value: Value = content
        .parse()
        .map_err(|e| LoomError::Config(format!("TOML parse error {}: {}", path.display(), e)))?;

    parse_merged(project_value)
}

/// 从 TOML 字符串解析（合并内置默认）。
///
/// 与 [`parse_from_file`] 行为一致，区别是输入为字符串而非文件路径。
pub fn parse_from_str(s: &str) -> Result<LoomManifest, LoomError> {
    let project_value: Value =
        s.parse().map_err(|e| LoomError::Config(format!("TOML parse error: {}", e)))?;
    parse_merged(project_value)
}

/// 从文件解析并验证。
pub fn parse_and_validate(path: &Path) -> Result<LoomManifest, LoomError> {
    let manifest = parse_from_file(path)?;
    let errors = validate::validate_manifest(&manifest);
    if !errors.is_empty() {
        return Err(LoomError::Config(format!(
            "Configuration validation failed:\n{}",
            errors.join("\n")
        )));
    }
    Ok(manifest)
}

/// 创建默认 Manifest（用于 `loom new` 和测试）。
///
/// 基于内置默认 TOML，仅覆盖与项目名相关的字段（name/version/description/
/// repository/exports）。其余字段全部继承内置默认。
pub fn default_manifest(name: &str) -> LoomManifest {
    let mut manifest = default::default_manifest();
    manifest.name = name.to_string();
    manifest.description = Some(format!("{} project", name));
    manifest.repository = Some(format!("https://github.com/aura-lang/{}.git", name));
    manifest.exports = vec!["main".to_string()];
    manifest
}

/// 生成最小 `aura.toml` 内容（用于 `loom new` 写文件）。
///
/// 只写 name/version/description，其余字段全部由内置默认提供。
pub fn minimal_toml(name: &str) -> String {
    format!(
        r#"# {} project config
# Undeclared fields use loom built-in defaults (see loom/src/manifest/default.toml)
# Priority: CLI > Profile > this file > built-in defaults > serde field defaults

name = "{}"
version = "0.1.0"
description = "{} project"
"#,
        name, name, name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_from_file 测试 ──

    #[test]
    fn test_parse_missing_file_returns_default() {
        let path = Path::new("/tmp/nonexistent_aura_toml_test/aura.toml");
        let manifest = parse_from_file(path).unwrap();
        assert_eq!(manifest.name, "unnamed");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(manifest.entry, "src/main.aura");
    }

    #[test]
    fn test_parse_minimal_project_toml() {
        // 最小项目配置：仅 name/version
        let s = r#"
name = "my-app"
version = "1.0.0"
"#;
        let manifest = parse_from_str(s).unwrap();
        assert_eq!(manifest.name, "my-app");
        assert_eq!(manifest.version, "1.0.0");
        // 其他字段继承内置默认
        assert_eq!(manifest.entry, "src/main.aura");
        assert_eq!(manifest.schema_version, "2.0");
        assert!(!manifest.library);
        // 源码集继承内置默认
        assert!(manifest.build.source_sets.contains_key("main"));
        assert!(manifest.build.source_sets.contains_key("test"));
        // 仓库继承内置默认
        assert_eq!(
            manifest.repositories.central.as_deref(),
            Some("https://registry.aura-lang.dev")
        );
    }

    #[test]
    fn test_parse_partial_source_set_merge() {
        // 项目只改 [build.source-sets.main] exclude
        let s = r#"
name = "test"
version = "1.0.0"

[build.source-sets.main]
exclude = ["foo", "bar"]
"#;
        let manifest = parse_from_str(s).unwrap();
        let main = &manifest.build.source_sets["main"];
        // 项目覆盖
        assert_eq!(
            main.exclude,
            vec![
                "foo", "bar"
            ]
        );
        // 默认继承
        assert_eq!(main.source_dirs, vec!["src"]);
        assert_eq!(main.resource_dirs, vec!["resources"]);
        assert_eq!(main.include, vec!["**/*.aura"]);
        assert!(main.depends_on.is_empty());
    }

    #[test]
    fn test_parse_partial_build_merge() {
        // 项目只改 [build] opt-level
        let s = r#"
name = "test"
version = "1.0.0"

[build]
opt-level = 0
"#;
        let manifest = parse_from_str(s).unwrap();
        assert_eq!(manifest.build.opt_level, 0);
        // 其他 [build] 字段走 serde 默认
        assert!(manifest.build.debug);
        assert_eq!(manifest.build.out_dir, "target/build");
        assert!(manifest.build.parallel);
        // 源码集继承内置默认
        assert!(manifest.build.source_sets.contains_key("main"));
    }

    #[test]
    fn test_parse_profile_partial_merge() {
        // 项目只改 [profiles.release.build] opt-level
        let s = r#"
name = "test"
version = "1.0.0"

[profiles.release.build]
opt-level = 1
"#;
        let manifest = parse_from_str(s).unwrap();
        let release = &manifest.profiles["release"];
        assert!(!release.activate); // 默认 false
        let build = release.build.as_ref().unwrap();
        assert_eq!(build.opt_level, Some(1));
        assert_eq!(build.debug, Some(false)); // 默认继承
        assert_eq!(build.emit_package, Some(true)); // 默认继承
    }

    #[test]
    fn test_parse_full_override() {
        // 项目完整覆盖（老格式）
        let s = r#"
name = "legacy-app"
version = "2.0.0"
entry = "app/main.aura"

[build]
opt-level = 3
debug = false

[build.source-sets.main]
source-dirs = ["app"]
"#;
        let manifest = parse_from_str(s).unwrap();
        assert_eq!(manifest.name, "legacy-app");
        assert_eq!(manifest.version, "2.0.0");
        assert_eq!(manifest.entry, "app/main.aura");
        assert_eq!(manifest.build.opt_level, 3);
        assert!(!manifest.build.debug);
        assert_eq!(manifest.build.source_sets["main"].source_dirs, vec!["app"]);
    }

    #[test]
    fn test_parse_empty_name_still_errors() {
        // 显式写空 name → 合并后仍为空 → 报错
        let s = r#"
name = ""
version = "1.0.0"
"#;
        assert!(parse_from_str(s).is_err());
    }

    #[test]
    fn test_parse_empty_version_still_errors() {
        let s = r#"
name = "test"
version = ""
"#;
        assert!(parse_from_str(s).is_err());
    }

    #[test]
    fn test_default_manifest_api() {
        let m = default_manifest("my-app");
        assert_eq!(m.name, "my-app");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.description.as_deref(), Some("my-app project"));
        assert_eq!(
            m.repository.as_deref(),
            Some("https://github.com/aura-lang/my-app.git")
        );
        assert_eq!(m.exports, vec!["main"]);
        // 其他字段继承内置默认
        assert_eq!(m.entry, "src/main.aura");
        assert!(m.build.source_sets.contains_key("main"));
    }

    #[test]
    fn test_minimal_toml_output() {
        let toml = minimal_toml("hello");
        assert!(toml.contains("name = \"hello\""));
        assert!(toml.contains("version = \"0.1.0\""));
        assert!(toml.contains("description = \"hello project\""));
        // 解析后应能反序列化
        let manifest = parse_from_str(&toml).unwrap();
        assert_eq!(manifest.name, "hello");
    }
}
