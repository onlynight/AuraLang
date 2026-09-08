//! loom 内置默认 aura.toml 配置 + TOML 值级别深度合并
//!
//! 优先级（从高到低）：
//!   1. CLI 参数
//!   2. 激活的 Profile
//!   3. 项目 aura.toml
//!   4. 本模块的内置默认配置
//!   5. serde 字段级默认
//!
//! 详见 `loom/docs/默认配置合并设计.md`。

use toml::Value;

/// 编译时嵌入的默认 `aura.toml` 内容。
///
/// 单一权威源：标量字段默认由 serde 处理，本文件只声明 serde 无法处理的
/// 结构性默认（嵌套表、有语义的占位值）。
pub const DEFAULT_TOML: &str = include_str!("default.toml");

/// 解析默认 TOML 为 `Value`（用于合并）。
///
/// panic on malformed default TOML — 这是编译时嵌入的字符串，
/// 若解析失败说明源码有问题，应在 CI 阶段暴露。
pub fn default_value() -> Value {
    DEFAULT_TOML.parse().expect("内置默认 aura.toml 解析失败")
}

/// 反序列化默认 TOML 为 `LoomManifest`。
///
/// 用于 `loom new` 和测试，提供"无任何项目覆盖"的完整默认 Manifest。
pub fn default_manifest() -> crate::manifest::LoomManifest {
    Value::try_into(default_value()).expect("内置默认 aura.toml 反序列化失败")
}

/// 深度合并两个 TOML 值：`overlay` 优先于 `base`。
///
/// # 合并规则
///
/// | base | overlay | 结果 |
/// |---|---|---|
/// | Table | Table | 递归合并：overlay 的 key 覆盖 base 同名 key；base 独有 key 保留 |
/// | Table | 非 Table | overlay 整体替换 |
/// | 非 Table | Table | overlay 整体替换 |
/// | Array | Array | **overlay 整体替换**（不做元素级合并） |
/// | 标量 | 任意 | overlay 整体替换 |
///
/// 典型场景：
/// - 项目只写 `[build.source-sets.main] exclude = ["foo"]`
///   → 合并后 `source-dirs`/`resource-dirs`/`include` 保留默认，`exclude` 被覆盖。
/// - 项目只写 `[build] opt-level = 0`
///   → 合并后 `opt-level = 0`，其他 `[build]` 字段走 serde 默认。
/// - 项目省略 `[build.source-sets.test]`
///   → 合并后 test 源码集完整保留（来自默认）。
pub fn merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Table(mut b), Value::Table(o)) => {
            for (k, v) in o {
                let merged = match b.remove(&k) {
                    Some(existing) => merge(existing, v),
                    None => v,
                };
                b.insert(k, merged);
            }
            Value::Table(b)
        }
        // overlay 是任意非 Table（含 Array/标量）→ 整体替换
        (_, overlay) => overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        s.parse().unwrap()
    }

    // ── 默认 TOML 自洽性测试 ──

    #[test]
    fn test_default_toml_parses() {
        let v = default_value();
        assert!(v.is_table());
    }

    #[test]
    fn test_default_manifest_deserializes() {
        let m = default_manifest();
        assert_eq!(m.schema_version, "2.0");
        assert_eq!(m.name, "unnamed");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.entry, "src/main.aura");
        assert!(!m.library);
        assert_eq!(m.kind, "bytecode");
    }

    #[test]
    fn test_default_has_source_sets() {
        let v = default_value();
        let build = v.get("build").and_then(|v| v.get("source-sets")).unwrap();
        assert!(build.get("main").is_some());
        assert!(build.get("test").is_some());
    }

    #[test]
    fn test_default_has_profiles() {
        let v = default_value();
        let profiles = v.get("profiles").unwrap();
        assert!(profiles.get("release").is_some());
        assert!(profiles.get("debug").is_some());
        // activate 默认都是 false
        assert_eq!(profiles["release"]["activate"], Value::Boolean(false));
        assert_eq!(profiles["debug"]["activate"], Value::Boolean(false));
    }

    #[test]
    fn test_default_has_registry() {
        let v = default_value();
        assert_eq!(
            v["repositories"]["central"],
            Value::String("https://registry.aura-lang.dev".to_string())
        );
    }

    // ── 合并语义测试 ──

    #[test]
    fn test_merge_scalar_overlay_wins() {
        let base = parse(
            r#"
name = "default"
version = "0.1.0"
"#,
        );
        let overlay = parse(
            r#"
name = "project"
"#,
        );
        let merged = merge(base, overlay);
        assert_eq!(merged["name"], Value::String("project".to_string()));
        assert_eq!(merged["version"], Value::String("0.1.0".to_string()));
    }

    #[test]
    fn test_merge_array_full_replacement() {
        let base = parse(
            r#"
deps = ["a", "b", "c"]
"#,
        );
        let overlay = parse(
            r#"
deps = ["x"]
"#,
        );
        let merged = merge(base, overlay);
        assert_eq!(merged["deps"].as_array().unwrap().len(), 1);
        assert_eq!(merged["deps"][0], Value::String("x".to_string()));
    }

    #[test]
    fn test_merge_empty_array_project_wins() {
        // 项目写 dependencies = [] → 覆盖默认
        let base = parse(
            r#"
deps = ["a"]
"#,
        );
        let overlay = parse(
            r#"
deps = []
"#,
        );
        let merged = merge(base, overlay);
        assert!(merged["deps"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_merge_missing_key_keeps_default() {
        let base = parse(
            r#"
name = "default"
version = "0.1.0"
"#,
        );
        let overlay = parse(
            r#"
version = "2.0.0"
"#,
        );
        let merged = merge(base, overlay);
        assert_eq!(merged["name"], Value::String("default".to_string()));
        assert_eq!(merged["version"], Value::String("2.0.0".to_string()));
    }

    #[test]
    fn test_merge_nested_table_recursive() {
        // 项目只写 [build.source-sets.main] exclude
        let base = parse(
            r#"
[build.source-sets.main]
source-dirs = ["src"]
resource-dirs = ["resources"]
include = ["**/*.aura"]
exclude = ["**/*.test.aura"]
depends-on = []
"#,
        );
        let overlay = parse(
            r#"
[build.source-sets.main]
exclude = ["foo"]
"#,
        );
        let merged = merge(base, overlay);
        let main = merged["build"]["source-sets"]["main"].clone();
        // 保留默认
        assert_eq!(main["source-dirs"][0], Value::String("src".to_string()));
        assert_eq!(
            main["resource-dirs"][0],
            Value::String("resources".to_string())
        );
        assert_eq!(main["include"][0], Value::String("**/*.aura".to_string()));
        assert_eq!(main["depends-on"].as_array().unwrap().len(), 0);
        // 项目覆盖
        assert_eq!(main["exclude"][0], Value::String("foo".to_string()));
    }

    #[test]
    fn test_merge_new_key_added_by_project() {
        let base = parse(
            r#"
name = "default"
"#,
        );
        let overlay = parse(
            r#"
name = "default"
version = "1.0.0"
"#,
        );
        let merged = merge(base, overlay);
        assert!(merged.get("version").is_some());
    }

    #[test]
    fn test_merge_table_to_scalar_replaces() {
        let base = parse(
            r#"
config = { foo = 1 }
"#,
        );
        let overlay = parse(
            r#"
config = "scalar"
"#,
        );
        let merged = merge(base, overlay);
        assert_eq!(merged["config"], Value::String("scalar".to_string()));
    }

    #[test]
    fn test_merge_scalar_to_table_replaces() {
        let base = parse(
            r#"
config = "scalar"
"#,
        );
        let overlay = parse(
            r#"
[config]
foo = 1
"#,
        );
        let merged = merge(base, overlay);
        assert!(merged["config"].is_table());
        assert_eq!(merged["config"]["foo"], Value::Integer(1));
    }

    #[test]
    fn test_merge_profile_build_partial() {
        // 项目只改 [profiles.release.build] opt-level
        let base = parse(
            r#"
[profiles.release]
activate = false
[profiles.release.build]
opt-level = 3
debug = false
emit-package = true
"#,
        );
        let overlay = parse(
            r#"
[profiles.release.build]
opt-level = 1
"#,
        );
        let merged = merge(base, overlay);
        let release = merged["profiles"]["release"].clone();
        assert_eq!(release["activate"], Value::Boolean(false));
        assert_eq!(release["build"]["opt-level"], Value::Integer(1));
        assert_eq!(release["build"]["debug"], Value::Boolean(false));
        assert_eq!(release["build"]["emit-package"], Value::Boolean(true));
    }

    #[test]
    fn test_merge_project_omits_test_source_set_keeps_default() {
        let base = default_value();
        let overlay = parse(
            r#"
name = "my-app"
version = "1.0.0"
"#,
        );
        let merged = merge(base, overlay);
        // test 源码集保留
        assert!(merged["build"]["source-sets"]["test"].is_table());
        assert_eq!(
            merged["build"]["source-sets"]["test"]["source-dirs"][0],
            Value::String("test".to_string())
        );
    }

    #[test]
    fn test_merge_project_omits_repositories_keeps_default() {
        let base = default_value();
        let overlay = parse(
            r#"
name = "my-app"
version = "1.0.0"
"#,
        );
        let merged = merge(base, overlay);
        assert_eq!(
            merged["repositories"]["central"],
            Value::String("https://registry.aura-lang.dev".to_string())
        );
    }

    #[test]
    fn test_merge_idempotent_when_project_equals_default() {
        // 合并是幂等的：项目值 = 默认值时结果不变
        let base = default_value();
        let overlay = default_value().clone();
        let merged = merge(base, overlay);
        assert_eq!(merged, default_value());
    }

    #[test]
    fn test_merge_empty_overlay_returns_base() {
        let base = default_value();
        // 空 TOML 文档 = 空表
        let overlay = Value::Table(toml::map::Map::new());
        let merged = merge(base, overlay);
        assert_eq!(merged, default_value());
    }
}
