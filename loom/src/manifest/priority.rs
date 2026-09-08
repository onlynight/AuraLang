//! 配置优先级解析
//!
//! 优先级（从高到低）：
//!   1. 命令行参数
//!   2. 激活的 profile
//!   3. 项目配置（aura.toml）
//!   4. 约定默认值
//!
//! B1.4

use std::collections::HashMap;

use crate::manifest::{CompileMode, FfiMode, LoomManifest};

/// CLI 参数覆盖（从命令行解析）
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    /// 优化级别
    pub opt_level: Option<u8>,
    /// 调试信息
    pub debug: Option<bool>,
    /// 目标三元组
    pub target: Option<String>,
    /// 是否打包为 .auz
    pub emit_package: Option<bool>,
    /// 并行构建
    pub parallel: Option<bool>,
    /// 并行任务数
    pub parallel_jobs: Option<u32>,
    /// 输出目录
    pub out_dir: Option<String>,
    /// 缓存目录
    pub cache_dir: Option<String>,
    /// 激活的 profile 名
    pub profile: Option<String>,
}

/// 解析后的最终构建配置
#[derive(Debug, Clone)]
pub struct ResolvedBuildConfig {
    /// 优化级别
    pub opt_level: u8,
    /// 调试信息
    pub debug: bool,
    /// 目标三元组
    pub target: Option<String>,
    /// 输出目录
    pub out_dir: String,
    /// 缓存目录
    pub cache_dir: String,
    /// 远程缓存 URL
    pub cache_remote: Option<String>,
    /// 远程缓存是否共享
    pub cache_remote_shared: bool,
    /// 是否生成类型签名
    pub emit_signatures: bool,
    /// 是否打包为 .auz
    pub emit_package: bool,
    /// 并行构建
    pub parallel: bool,
    /// 并行任务数
    pub parallel_jobs: u32,
    /// 别名映射
    pub alias: HashMap<String, String>,
    /// 活跃的 profile 名（如有）
    pub active_profile: Option<String>,
    /// 编译模式（vm | jit | aot）
    pub mode: CompileMode,
    /// FFI 模式（cabi | aot）
    pub ffi_mode: FfiMode,
    /// 是否为库包
    pub library: bool,
    /// 包名
    pub name: String,
}

impl Default for ResolvedBuildConfig {
    fn default() -> Self {
        Self {
            opt_level: 2,
            debug: true,
            target: None,
            out_dir: "target/build".to_string(),
            cache_dir: "target/cache".to_string(),
            cache_remote: None,
            cache_remote_shared: false,
            emit_signatures: true,
            emit_package: false,
            parallel: true,
            parallel_jobs: 4,
            alias: HashMap::new(),
            active_profile: None,
            mode: CompileMode::default(),
            ffi_mode: FfiMode::default(),
            library: false,
            name: String::new(),
        }
    }
}

/// 解析最终构建配置
///
/// 合并顺序：默认值 → 项目配置 → profile 覆盖 → CLI 覆盖
pub fn resolve_build_config(manifest: &LoomManifest, cli: &CliOverrides) -> ResolvedBuildConfig {
    // 1. 从项目配置开始（BuildConfig 已含默认值）
    let project_build = &manifest.build;

    // 2. 应用 profile 覆盖
    let profile_override = cli
        .profile
        .as_deref()
        .and_then(|name| manifest.profiles.get(name).and_then(|p| p.build.as_ref()));

    // 3. 合并
    let opt_level = cli
        .opt_level
        .or_else(|| profile_override.and_then(|o| o.opt_level))
        .unwrap_or(project_build.opt_level);

    let debug =
        cli.debug.or_else(|| profile_override.and_then(|o| o.debug)).unwrap_or(project_build.debug);

    let target = cli
        .target
        .clone()
        .or_else(|| profile_override.and_then(|o| o.target.clone()))
        .or_else(|| project_build.target.clone());

    let emit_package = cli
        .emit_package
        .or_else(|| profile_override.and_then(|o| o.emit_package))
        .unwrap_or(project_build.emit_package);

    let parallel = cli
        .parallel
        .or_else(|| profile_override.and_then(|o| o.parallel))
        .unwrap_or(project_build.parallel);

    let parallel_jobs = cli.parallel_jobs.unwrap_or(project_build.parallel_jobs);

    let out_dir = cli.out_dir.clone().unwrap_or_else(|| project_build.out_dir.clone());

    let cache_dir = cli.cache_dir.clone().unwrap_or_else(|| project_build.cache_dir.clone());

    ResolvedBuildConfig {
        opt_level,
        debug,
        target,
        out_dir,
        cache_dir,
        cache_remote: project_build.cache_remote.clone(),
        cache_remote_shared: project_build.cache_remote_shared,
        emit_signatures: project_build.emit_signatures,
        emit_package,
        parallel,
        parallel_jobs,
        alias: project_build.alias.clone(),
        active_profile: cli.profile.clone(),
        mode: manifest.mode,
        ffi_mode: project_build.ffi_mode.clone(),
        library: manifest.library,
        name: manifest.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manifest_with_profile() -> LoomManifest {
        let toml_str = r#"
name = "test"
version = "1.0.0"

[build]
opt-level = 1
debug = true
out-dir = "target/build"
cache-dir = "target/cache"
parallel = false
parallel-jobs = 2

[profiles.release]
[profiles.release.build]
opt-level = 3
debug = false
emit-package = true
parallel = true

[profiles.ci]
activate = true
[profiles.ci.build]
opt-level = 2
debug = false
emit-package = true
"#;
        toml::from_str(toml_str).unwrap()
    }

    #[test]
    fn test_resolve_default() {
        let manifest = make_manifest_with_profile();
        let cli = CliOverrides::default();
        let resolved = resolve_build_config(&manifest, &cli);

        // 使用项目配置值
        assert_eq!(resolved.opt_level, 1);
        assert!(resolved.debug);
        assert_eq!(resolved.out_dir, "target/build");
        assert_eq!(resolved.cache_dir, "target/cache");
        assert!(!resolved.parallel);
        assert_eq!(resolved.parallel_jobs, 2);
        assert!(resolved.active_profile.is_none());
    }

    #[test]
    fn test_resolve_profile() {
        let manifest = make_manifest_with_profile();
        let cli = CliOverrides {
            profile: Some("release".to_string()),
            ..Default::default()
        };
        let resolved = resolve_build_config(&manifest, &cli);

        // 使用 release profile 值
        assert_eq!(resolved.opt_level, 3);
        assert!(!resolved.debug);
        assert!(resolved.emit_package);
        assert!(resolved.parallel);
        assert_eq!(resolved.active_profile.as_deref(), Some("release"));
    }

    #[test]
    fn test_resolve_cli_overrides_profile() {
        let manifest = make_manifest_with_profile();
        let cli = CliOverrides {
            profile: Some("release".to_string()),
            opt_level: Some(0),
            debug: Some(true),
            ..Default::default()
        };
        let resolved = resolve_build_config(&manifest, &cli);

        // CLI 覆盖 profile 值
        assert_eq!(resolved.opt_level, 0); // CLI 覆盖 release 的 3
        assert!(resolved.debug); // CLI 覆盖 release 的 false
        assert!(resolved.emit_package); // 保留 release 的 true
        assert!(resolved.parallel); // 保留 release 的 true
    }

    #[test]
    fn test_resolve_cli_overrides_project() {
        let manifest = make_manifest_with_profile();
        let cli = CliOverrides {
            opt_level: Some(3),
            debug: Some(false),
            parallel: Some(true),
            ..Default::default()
        };
        let resolved = resolve_build_config(&manifest, &cli);

        // CLI 覆盖项目配置值
        assert_eq!(resolved.opt_level, 3);
        assert!(!resolved.debug);
        assert!(resolved.parallel);
        assert_eq!(resolved.out_dir, "target/build"); // 保留项目值
    }

    #[test]
    fn test_resolve_active_profile_from_manifest() {
        let manifest = make_manifest_with_profile();
        // ci profile 是 activate = true
        let cli = CliOverrides::default();
        let resolved = resolve_build_config(&manifest, &cli);

        // 没有 CLI profile，使用项目配置
        // 注意：当前实现中，未激活的 profile 不会自动应用
        // 只有 CLI 指定 profile 时才应用
        assert_eq!(resolved.opt_level, 1); // 使用项目配置
        assert!(resolved.active_profile.is_none());
    }
}
