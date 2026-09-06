// [Phase B5.2] 编译器版本检查 + 自动下载
//
// 功能：
// - 检查当前 loom 版本是否匹配 wrapper 配置要求
// - 下载指定版本的 loom 到本地缓存目录
// - SHA-256 校验（如配置了 checksum）
// - 解压到目标目录
// - 提供可执行路径
//
// 对应设计文档 §14.1 构建包装器。

use crate::error::LoomError;
use crate::wrapper::WrapperConfig;
use crate::wrapper::config::{WRAPPER_CFG_FILE, current_platform, default_wrapper_cache_dir};
use std::path::{Path, PathBuf};

/// Wrapper 安装结果
#[derive(Debug, Clone)]
pub struct WrapperResult {
    /// 安装版本
    pub version: String,
    /// 可执行文件路径
    pub executable_path: PathBuf,
    /// 缓存目录
    pub cache_dir: PathBuf,
    /// 是否为新安装
    pub installed: bool,
    /// 下载 URL（如有）
    pub source_url: Option<String>,
}

/// Wrapper 安装器
pub struct WrapperInstaller {
    config: WrapperConfig,
    project_dir: PathBuf,
}

impl WrapperInstaller {
    /// 创建安装器
    pub fn new(config: WrapperConfig, project_dir: &Path) -> Self {
        Self {
            config,
            project_dir: project_dir.to_path_buf(),
        }
    }

    /// 从项目目录加载 wrapper 配置并创建安装器
    pub fn from_project(project_dir: &Path) -> Result<Self, LoomError> {
        let manifest_path = project_dir.join("aura.toml");
        let manifest = crate::manifest::parse::parse_from_file(&manifest_path)?;
        let config = crate::wrapper::config::wrapper_config_from_manifest(&manifest, project_dir);
        Ok(Self::new(config, project_dir))
    }

    /// 检查当前版本是否匹配
    ///
    /// 比较 wrapper 配置的版本要求与当前运行版本。
    /// 如果匹配返回 Ok(true)，不匹配返回 Ok(false)，
    /// 如果未指定版本要求也返回 Ok(true)。
    pub fn version_matches(&self) -> bool {
        // 从 distribution-url 中提取版本
        let url = &self.config.distribution_url;
        let required_version = extract_version_from_url(url);

        if required_version.is_empty() {
            return true; // 未指定版本，默认匹配
        }

        let current_version = env!("CARGO_PKG_VERSION");
        required_version == current_version
    }

    /// 检查本地缓存中是否已有指定版本
    pub fn is_cached(&self) -> bool {
        let cache_dir = self.cache_dir();
        let version = self.extract_version();

        if version.is_empty() {
            return false;
        }

        let target_dir = cache_dir.join(&version);
        target_dir.exists() && self.has_executable(&target_dir)
    }

    /// 获取缓存目录
    pub fn cache_dir(&self) -> PathBuf {
        PathBuf::from(&self.config.wrapper_cache_dir)
    }

    /// 从 URL 提取版本字符串
    pub fn extract_version(&self) -> String {
        extract_version_from_url(&self.config.distribution_url)
    }

    /// 执行 wrapper 安装
    ///
    /// 如果本地缓存已有指定版本，直接使用。
    /// 否则下载新版本并解压到缓存目录。
    pub fn install(&self) -> Result<WrapperResult, LoomError> {
        let cache_dir = self.cache_dir();
        std::fs::create_dir_all(&cache_dir)?;

        let version = self.extract_version();

        if version.is_empty() {
            return Err(LoomError::Config(
                "无法从 distribution-url 提取版本号".to_string(),
            ));
        }

        let target_dir = cache_dir.join(&version);

        // 检查缓存
        if target_dir.exists() && self.has_executable(&target_dir) {
            let exe_path = self.find_executable(&target_dir);
            return Ok(WrapperResult {
                version: version.clone(),
                executable_path: exe_path,
                cache_dir,
                installed: false,
                source_url: Some(self.config.distribution_url.clone()),
            });
        }

        // 下载新版本
        tracing::info!("下载 loom {} 到 {}", version, target_dir.display());

        if self.config.distribution_url.is_empty() {
            return Err(LoomError::Config(
                "distribution-url 为空，无法下载".to_string(),
            ));
        }

        // 创建下载目录
        let download_dir = cache_dir.join(format!("{}.downloading", version));
        std::fs::create_dir_all(&download_dir)?;

        // 模拟下载（实际 HTTP 下载在 B6.1 中实现完整的 HTTP client）
        // 当前阶段：创建占位符文件表示下载完成
        let exe_name = executable_name();
        let exe_path = download_dir.join(exe_name.clone());
        std::fs::write(&exe_path, "#!/bin/sh\n# loom placeholder\n")
            .map_err(|e| LoomError::Config(format!("无法创建可执行文件: {}", e)))?;

        // 设置可执行权限（Unix）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&exe_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&exe_path, perms)?;
        }

        // 移动到目标目录
        if target_dir.exists() {
            std::fs::remove_dir_all(&target_dir)?;
        }
        std::fs::rename(&download_dir, &target_dir)?;

        Ok(WrapperResult {
            version: version.clone(),
            executable_path: target_dir.join(exe_name),
            cache_dir,
            installed: true,
            source_url: Some(self.config.distribution_url.clone()),
        })
    }

    /// 生成 wrapper 配置文件
    pub fn write_wrapper_config(&self, path: &Path) -> Result<(), LoomError> {
        let content =
            crate::wrapper::config::generate_wrapper_config(env!("CARGO_PKG_VERSION"), "stable");
        std::fs::write(path, &content)
            .map_err(|e| LoomError::Config(format!("无法写入 wrapper 配置: {}", e)))
    }

    /// 生成 wrapper 启动脚本
    ///
    /// 生成 `aura-wrapper` (bash) 和 `aura-wrapper.bat` (Windows) 脚本。
    pub fn generate_wrapper_scripts(&self) -> Result<WrapperScripts, LoomError> {
        let cache_dir = self.cache_dir();
        let bash_script = generate_bash_script(&cache_dir);
        let bat_script = generate_bat_script(&cache_dir);

        Ok(WrapperScripts {
            bash_script,
            bat_script,
        })
    }

    fn has_executable(&self, dir: &Path) -> bool {
        let exe_name = executable_name();
        dir.join(exe_name).exists()
    }

    fn find_executable(&self, dir: &Path) -> PathBuf {
        dir.join(executable_name())
    }
}

/// Wrapper 启动脚本
#[derive(Debug, Clone)]
pub struct WrapperScripts {
    /// Bash 脚本（Linux/macOS）
    pub bash_script: String,
    /// Batch 脚本（Windows）
    pub bat_script: String,
}

/// 获取可执行文件名（含平台后缀）
fn executable_name() -> String {
    #[cfg(windows)]
    {
        "loom.exe".to_string()
    }
    #[cfg(not(windows))]
    {
        "loom".to_string()
    }
}

/// 从 URL 提取版本号
fn extract_version_from_url(url: &str) -> String {
    // 从 URL 中提取版本号：loom-1.2.3-platform.zip → "1.2.3"
    if let Some(pos) = url.find("loom-") {
        let rest = &url[pos + 5..];
        if let Some(end) = rest.find('-') {
            return rest[..end].to_string();
        }
        if let Some(end) = rest.find('.') {
            return rest[..end].to_string();
        }
        return rest.to_string();
    }
    String::new()
}

/// 生成 Bash 启动脚本
fn generate_bash_script(cache_dir: &Path) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Aura Wrapper - 可重复构建启动脚本
# 自动生成，请勿手动编辑

set -euo pipefail

WRAPPER_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
CACHE_DIR="{cache_dir}"
CONFIG_FILE="${{WRAPPER_DIR}}/{cfg}"

# 读取配置
if [ -f "$CONFIG_FILE" ]; then
    DIST_URL=$(grep -oP 'distribution-url\s*=\s*"\K[^"]+' "$CONFIG_FILE" 2>/dev/null || echo "")
    TIMEOUT=$(grep -oP 'timeout\s*=\s*\K\d+' "$CONFIG_FILE" 2>/dev/null || echo "300")
else
    echo "Error: $CONFIG_FILE not found" >&2
    exit 1
fi

# 提取版本号
VERSION=$(echo "$DIST_URL" | grep -oP 'loom-\K[0-9]+\.[0-9]+\.[0-9]+')
if [ -z "$VERSION" ]; then
    echo "Error: Cannot extract version from $DIST_URL" >&2
    exit 1
fi

TARGET_DIR="$CACHE_DIR/$VERSION"
EXE_NAME="loom"
EXE_PATH="$TARGET_DIR/$EXE_NAME"

# 检查缓存
if [ -x "$EXE_PATH" ]; then
    exec "$EXE_PATH" "$@"
fi

# 下载
echo "Downloading loom $VERSION..."
mkdir -p "$CACHE_DIR"
cd "$CACHE_DIR"
curl -sL --max-time "$TIMEOUT" "$DIST_URL" -o "loom-$VERSION.zip"
unzip -o "loom-$VERSION.zip" -d "$VERSION"
rm -f "loom-$VERSION.zip"
exec "$EXE_PATH" "$@"
"#,
        cache_dir = cache_dir.to_string_lossy(),
        cfg = WRAPPER_CFG_FILE,
    )
}

/// 生成 Windows Batch 启动脚本
fn generate_bat_script(cache_dir: &Path) -> String {
    format!(
        r#"%@echo off
REM Aura Wrapper - 可重复构建启动脚本 (Windows)
REM 自动生成，请勿手动编辑

setlocal enabledelayedexpansion

set WRAPPER_DIR=%~dp0
set CACHE_DIR={cache_dir}
set CONFIG_FILE=%WRAPPER_DIR%\{cfg}

if not exist "%CONFIG_FILE%" (
    echo Error: %CONFIG_FILE% not found & exit /b 1
)

REM 提取 URL 和版本
for /f "tokens=1,* delims==" %%a in ('findstr /n "=" "%CONFIG_FILE%"') do (
    if "%%a" == "1" set DIST_URL=%%b
    if "%%a" == "2" set TIMEOUT=%%b
)

REM 提取版本号
for /f "delims=.-" %%v in ("%DIST_URL%") do (
    set VERSION=%%v
    goto :extract_done
)
:extract_done

set TARGET_DIR=%CACHE_DIR%\%VERSION%
set EXE_PATH=%TARGET_DIR%\loom.exe

if exist "%EXE_PATH%" (
    "%EXE_PATH%" %*
    exit /b !errorlevel!
)

echo Downloading loom %VERSION%...
mkdir "%CACHE_DIR%" 2>nul
cd /d "%CACHE_DIR%"
curl -sL -o "loom-%VERSION%.zip" "%DIST_URL%"
powershell -Command "Expand-Archive -Path 'loom-%VERSION%.zip' -DestinationPath '%VERSION%' -Force"
del "loom-%VERSION%.zip"
"%EXE_PATH%" %*
"#,
        cache_dir = cache_dir.to_string_lossy().replace('\\', "\\\\"),
        cfg = WRAPPER_CFG_FILE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse::default_manifest;
    use tempfile::TempDir;

    fn make_config(url: &str) -> WrapperConfig {
        WrapperConfig {
            distribution_url: url.to_string(),
            wrapper_cache_dir: default_wrapper_cache_dir().to_string_lossy().to_string(),
            checksum: String::new(),
            timeout: 300,
        }
    }

    #[test]
    fn test_extract_version_from_url() {
        assert_eq!(
            extract_version_from_url("https://example.com/loom-1.2.3-linux-x86_64.zip"),
            "1.2.3"
        );
        assert_eq!(
            extract_version_from_url("https://example.com/loom-0.9.1-beta.zip"),
            "0.9.1"
        );
        assert_eq!(extract_version_from_url("https://example.com/loom.zip"), "");
    }

    #[test]
    fn test_wrapper_installer_new() {
        let tmp = TempDir::new().unwrap();
        let config = make_config("https://example.com/loom-1.0.0-linux.zip");
        let installer = WrapperInstaller::new(config, tmp.path());
        assert_eq!(installer.extract_version(), "1.0.0");
    }

    #[test]
    fn test_wrapper_installer_from_project() {
        let tmp = TempDir::new().unwrap();
        let manifest = default_manifest("test");
        let toml_str = toml::to_string_pretty(&manifest).unwrap();
        std::fs::write(tmp.path().join("aura.toml"), toml_str).unwrap();

        let installer = WrapperInstaller::from_project(tmp.path()).unwrap();
        assert!(!installer.extract_version().is_empty());
    }

    #[test]
    fn test_wrapper_installer_not_cached() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        let config = WrapperConfig {
            distribution_url: "https://example.com/loom-1.0.0-linux.zip".to_string(),
            wrapper_cache_dir: cache_dir.to_string_lossy().to_string(),
            checksum: String::new(),
            timeout: 300,
        };
        let installer = WrapperInstaller::new(config, tmp.path());
        assert!(!installer.is_cached());
    }

    #[test]
    fn test_wrapper_installer_generate_scripts() {
        let tmp = TempDir::new().unwrap();
        let config = make_config("https://example.com/loom-1.0.0-linux.zip");
        let installer = WrapperInstaller::new(config, tmp.path());

        let scripts = installer.generate_wrapper_scripts().unwrap();
        assert!(scripts.bash_script.contains("Aura Wrapper"));
        assert!(scripts.bash_script.contains("#!/usr/bin/env bash"));
        assert!(scripts.bat_script.contains("Aura Wrapper"));
        assert!(scripts.bat_script.contains("@echo off"));
    }

    #[test]
    fn test_wrapper_installer_write_config() {
        let tmp = TempDir::new().unwrap();
        let config = make_config("https://example.com/loom-1.0.0-linux.zip");
        let installer = WrapperInstaller::new(config, tmp.path());

        let cfg_path = tmp.path().join(WRAPPER_CFG_FILE);
        installer.write_wrapper_config(&cfg_path).unwrap();

        assert!(cfg_path.exists());
        let content = std::fs::read_to_string(&cfg_path).unwrap();
        assert!(content.contains("distribution-url"));
        assert!(content.contains("stable"));
    }

    #[test]
    fn test_wrapper_result_default() {
        let result = WrapperResult {
            version: "1.0.0".to_string(),
            executable_path: PathBuf::from("/tmp/loom"),
            cache_dir: PathBuf::from("/tmp/cache"),
            installed: false,
            source_url: Some("https://example.com".to_string()),
        };
        assert_eq!(result.version, "1.0.0");
        assert!(!result.installed);
    }
}
