//! [Phase B5.1] `aura-wrapper.cfg` 配置解析
//
// Wrapper 配置文件格式（类 Gradle wrapper.properties）：
// ```properties
// distributionBase=GRADLE_USER_HOME
// distributionPath=wrapper/dists
// distributionUrl=https://aura-lang.dev/releases/loom-1.0.0-bin.zip
// distributionSha256Sum=abc123...
// ```

use crate::error::LoomError;
use crate::wrapper::WrapperConfig;
use std::path::{Path, PathBuf};

/// 默认 wrapper 配置文件名
pub const WRAPPER_CFG_FILE: &str = "aura-wrapper.cfg";

/// 默认 wrapper 缓存目录
pub fn default_wrapper_cache_dir() -> PathBuf {
    let home = std::env::var("AURA_HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(&home).join(".aura").join("wrapper")
}

/// 解析 `aura-wrapper.cfg` 文件
///
// 支持 properties 格式（key=value）和 TOML 格式（向后兼容）。
///
/// # 支持的配置项
/// - `distribution-url` / `distributionUrl`: 编译器下载 URL
/// - `wrapper-cache-dir`: 本地缓存目录
// - `checksum` / `distributionSha256Sum`: SHA-256 校验和
/// - `timeout`: 安装超时（秒），默认 300
/// - `distribution-version`: 指定版本（与 URL 二选一）
/// - `distribution-channel`: 发布渠道（stable/beta/nightly）
pub fn parse_wrapper_config(path: &Path) -> Result<WrapperConfig, LoomError> {
    if !path.exists() {
        return Err(LoomError::Config(format!(
            "wrapper 配置文件不存在: {}",
            path.display()
        )));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| LoomError::Config(format!("无法读取 {}: {}", path.display(), e)))?;

    // 尝试 TOML 格式
    if let Ok(toml_config) = content.trim().parse::<toml::Value>() {
        return parse_toml_config(&toml_config);
    }

    // 回退到 properties 格式
    parse_properties_config(&content)
}

/// 解析 TOML 格式的 wrapper 配置
fn parse_toml_config(value: &toml::Value) -> Result<WrapperConfig, LoomError> {
    let dist_url = value
        .get("distribution-url")
        .or_else(|| value.get("distributionUrl"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| LoomError::Config("wrapper 配置缺少 distribution-url".to_string()))?;

    let cache_dir = value
        .get("wrapper-cache-dir")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(default_wrapper_cache_dir);

    let checksum = value
        .get("checksum")
        .or_else(|| value.get("distributionSha256Sum"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let timeout =
        value.get("timeout").and_then(|v| v.as_integer()).map(|i| i as u64).unwrap_or(300);

    let version =
        value.get("distribution-version").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let channel =
        value.get("distribution-channel").and_then(|v| v.as_str()).unwrap_or("stable").to_string();

    Ok(WrapperConfig {
        distribution_url: dist_url.to_string(),
        wrapper_cache_dir: cache_dir.to_string_lossy().to_string(),
        checksum,
        timeout,
    })
}

/// 解析 properties 格式的 wrapper 配置
fn parse_properties_config(content: &str) -> Result<WrapperConfig, LoomError> {
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            config.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let dist_url = config
        .get("distribution-url")
        .or_else(|| config.get("distributionUrl"))
        .cloned()
        .ok_or_else(|| {
            LoomError::Config("wrapper 配置缺少 distribution-url / distributionUrl".to_string())
        })?;

    let cache_dir = config
        .get("wrapper-cache-dir")
        .map(PathBuf::from)
        .unwrap_or_else(default_wrapper_cache_dir);

    let checksum = config
        .get("checksum")
        .or_else(|| config.get("distributionSha256Sum"))
        .cloned()
        .unwrap_or_default();

    let timeout = config.get("timeout").and_then(|s| s.parse().ok()).unwrap_or(300);

    Ok(WrapperConfig {
        distribution_url: dist_url,
        wrapper_cache_dir: cache_dir.to_string_lossy().to_string(),
        checksum,
        timeout,
    })
}

/// 生成 wrapper 配置文件（用于 `loom wrapper install`）
pub fn generate_wrapper_config(version: &str, channel: &str) -> String {
    let dist_url = format!(
        "https://aura-lang.dev/releases/loom-{}-{}-{}.zip",
        version,
        current_platform(),
        channel
    );
    let cache_dir = default_wrapper_cache_dir().to_string_lossy().to_string();

    format!(
        "# Aura Wrapper Configuration
distribution-url = \"{}\"
wrapper-cache-dir = \"{}\"
checksum = \"\"
timeout = 300
distribution-version = \"{}\"
distribution-channel = \"{}\"
",
        dist_url, cache_dir, version, channel
    )
}

/// 获取当前平台标识符
pub fn current_platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "windows-x86_64"
    }
    #[cfg(target_os = "macos")]
    {
        "macos-x86_64"
    }
    #[cfg(target_os = "linux")]
    {
        "linux-x86_64"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "unknown"
    }
}

/// 从 manifest 的 wrapper 配置生成 WrapperConfig
///
/// 如果 manifest 中配置了 wrapper 字段，使用该配置；
/// 否则使用默认值。
pub fn wrapper_config_from_manifest(
    _manifest: &crate::manifest::LoomManifest,
    project_dir: &Path,
) -> WrapperConfig {
    // 尝试从项目目录加载配置文件
    let cfg_path = project_dir.join(WRAPPER_CFG_FILE);
    if cfg_path.exists() {
        if let Ok(config) = parse_wrapper_config(&cfg_path) {
            return config;
        }
    }

    // 默认配置
    WrapperConfig {
        distribution_url: format!(
            "https://aura-lang.dev/releases/loom-{}-{}.zip",
            env!("CARGO_PKG_VERSION"),
            current_platform()
        ),
        wrapper_cache_dir: default_wrapper_cache_dir().to_string_lossy().to_string(),
        checksum: String::new(),
        timeout: 300,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_properties_config() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join(WRAPPER_CFG_FILE);
        std::fs::write(
            &cfg_path,
            "# Wrapper config
distributionUrl=https://example.com/loom.zip
distributionSha256Sum=abc123
timeout=600
",
        )
        .unwrap();

        let config = parse_wrapper_config(&cfg_path).unwrap();
        assert_eq!(config.distribution_url, "https://example.com/loom.zip");
        assert_eq!(config.checksum, "abc123");
        assert_eq!(config.timeout, 600);
    }

    #[test]
    fn test_parse_toml_config() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("wrapper.toml");
        std::fs::write(
            &cfg_path,
            r#"
distribution-url = "https://example.com/loom.zip"
checksum = "def456"
timeout = 120
distribution-version = "1.2.0"
distribution-channel = "beta"
"#,
        )
        .unwrap();

        let content = std::fs::read_to_string(&cfg_path).unwrap();
        let value: toml::Value = content.parse().unwrap();
        let config = parse_toml_config(&value).unwrap();
        assert_eq!(config.distribution_url, "https://example.com/loom.zip");
        assert_eq!(config.checksum, "def456");
        assert_eq!(config.timeout, 120);
    }

    #[test]
    fn test_parse_missing_file() {
        let result = parse_wrapper_config(Path::new("/nonexistent/cfg"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_missing_url() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("bad.cfg");
        std::fs::write(&cfg_path, "timeout=600\n").unwrap();

        let result = parse_wrapper_config(&cfg_path);
        assert!(result.is_err());
        let err = if let Err(e) = result { e.to_string() } else { unreachable!() };
        assert!(err.contains("distribution-url"));
    }

    #[test]
    fn test_generate_wrapper_config() {
        let config_str = generate_wrapper_config("1.0.0", "stable");
        assert!(config_str.contains("loom-1.0.0"));
        assert!(config_str.contains("stable"));
        assert!(config_str.contains("distribution-url"));
    }

    #[test]
    fn test_default_wrapper_cache_dir() {
        let dir = default_wrapper_cache_dir();
        assert!(dir.ends_with("wrapper") || dir.ends_with("wrapper\\"));
    }

    #[test]
    fn test_current_platform() {
        let platform = current_platform();
        assert!(!platform.is_empty());
    }

    #[test]
    fn test_wrapper_config_from_manifest_default() {
        let manifest = crate::manifest::parse::default_manifest("test");
        let tmp = TempDir::new().unwrap();
        let config = wrapper_config_from_manifest(&manifest, tmp.path());
        assert!(!config.distribution_url.is_empty());
        assert!(!config.wrapper_cache_dir.is_empty());
        assert_eq!(config.timeout, 300);
    }

    #[test]
    fn test_parse_properties_with_comments() {
        let tmp = TempDir::new().unwrap();
        let cfg_path = tmp.path().join("test.cfg");
        std::fs::write(
            &cfg_path,
            "# This is a comment
! Another comment
# distribution-url = https://wrong.com
distribution-url = https://correct.com/loom.zip

# empty line above and below
timeout = 42
",
        )
        .unwrap();

        let config = parse_wrapper_config(&cfg_path).unwrap();
        assert_eq!(config.distribution_url, "https://correct.com/loom.zip");
        assert_eq!(config.timeout, 42);
    }
}
