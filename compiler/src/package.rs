//! P11: 包管理器
//!
//! 声明式依赖（`// @depends`）、去中心化 Git 发布、语义化版本。
//! 详见技术方案.md 第八章。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ─────────────────────────────────────────────────────────────────────────────
// 语义化版本
// ─────────────────────────────────────────────────────────────────────────────

/// 语义化版本号（Semantic Versioning 2.0.0）
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub prerelease: Vec<String>,
    pub build: Vec<String>,
}

impl Version {
    /// 解析版本号字符串，如 "1.2.3"、"1.2.3-rc.1"、"1.2.3+build.123"、"1.0"
    pub fn parse(s: &str) -> Result<Version, PackageError> {
        let (main, build) = match s.split_once('+') {
            Some((m, b)) => (m, Some(b.to_string())),
            None => (s, None),
        };
        let (version_part, prerelease_part) = match main.split_once('-') {
            Some((v, p)) => (v, Some(p.to_string())),
            None => (main, None),
        };

        let parts: Vec<&str> = version_part.split('.').collect();
        if parts.len() < 2 {
            return Err(PackageError::InvalidVersion(format!(
                "invalid version number: {}",
                s
            )));
        }

        let major: u64 = parts[0]
            .parse()
            .map_err(|_| PackageError::InvalidVersion(format!("invalid version number: {}", s)))?;
        let minor: u64 = parts[1]
            .parse()
            .map_err(|_| PackageError::InvalidVersion(format!("invalid version number: {}", s)))?;
        let patch: u64 = if parts.len() >= 3 {
            parts[2].parse().map_err(|_| {
                PackageError::InvalidVersion(format!("invalid version number: {}", s))
            })?
        } else {
            0
        };

        let prerelease = prerelease_part
            .map(|p| p.split('.').map(|x| x.to_string()).collect())
            .unwrap_or_default();
        let build =
            build.map(|b| b.split('.').map(|x| x.to_string()).collect()).unwrap_or_default();

        Ok(Version {
            major,
            minor,
            patch,
            prerelease,
            build,
        })
    }

    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
            prerelease: vec![],
            build: vec![],
        }
    }

    /// 比较版本：是否满足约束（返回 true 表示满足）
    pub fn matches_constraint(&self, constraint: &VersionConstraint) -> bool {
        match constraint {
            VersionConstraint::Exact(v) => self == v,
            VersionConstraint::GreaterThanEqual(v) => self >= v,
            VersionConstraint::GreaterThan(v) => self > v,
            VersionConstraint::LessThan(v) => self < v,
            VersionConstraint::Tilde(v) => {
                // ~1.0 表示 >=1.0, <2.0
                self.major == v.major && self.minor >= v.minor && self.major < v.major + 1
            }
            VersionConstraint::Compatible(v) => {
                // ^1.0 表示 >=1.0, <2.0
                // ^0.5 表示 >=0.5, <0.6
                // ^0.0.5 表示 >=0.0.5, <0.0.6
                if v.major > 0 {
                    self.major == v.major
                } else if v.minor > 0 {
                    self.major == 0 && self.minor == v.minor
                } else {
                    self.major == 0 && self.minor == 0 && self.patch == v.patch
                }
            }
            VersionConstraint::Range(start, end) => self >= start && self < end,
            VersionConstraint::Any => true,
        }
    }

    pub fn to_string(&self) -> String {
        let mut s = format!("{}.{}.{}", self.major, self.minor, self.patch);
        if !self.prerelease.is_empty() {
            s.push('-');
            s.push_str(&self.prerelease.join("."));
        }
        if !self.build.is_empty() {
            s.push('+');
            s.push_str(&self.build.join("."));
        }
        s
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 版本约束
// ─────────────────────────────────────────────────────────────────────────────

/// 版本约束（技术方案 §8.2 依赖格式）
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum VersionConstraint {
    /// == 1.2 精确版本
    Exact(Version),
    /// >= 1.0 大于等于
    GreaterThanEqual(Version),
    /// > 1.0 大于
    GreaterThan(Version),
    /// < 1.0 小于
    LessThan(Version),
    /// ~> 1.0 兼容版本（1.0 <= version < 2.0）
    Tilde(Version),
    /// ^1.0 兼容版本（Maven 风格）
    Compatible(Version),
    /// >= 1.0, < 2.0 范围
    Range(Version, Version),
    /// * 任意版本
    Any,
}

impl VersionConstraint {
    /// 解析版本约束字符串
    /// 支持: >= 1.0, == 1.2, ~> 1.0, ^1.0, *
    pub fn parse(s: &str) -> Result<VersionConstraint, PackageError> {
        let s = s.trim();
        if s == "*" || s == "latest" {
            return Ok(VersionConstraint::Any);
        }

        // 检查是否带操作符
        for op in [
            "~>", "^", ">=", "<=", ">", "<", "==", "=",
        ] {
            if s.starts_with(op) {
                let version_str = s.trim_start_matches(op).trim();
                let version = Version::parse(version_str)?;
                return match op {
                    "~>" => Ok(VersionConstraint::Tilde(version)),
                    "^" => Ok(VersionConstraint::Compatible(version)),
                    ">=" => Ok(VersionConstraint::GreaterThanEqual(version)),
                    "<=" => Ok(VersionConstraint::LessThan(Version::parse(&format!(
                        "{}.{}.{}",
                        version.major,
                        version.minor + 1,
                        0
                    ))?)),
                    ">" => Ok(VersionConstraint::GreaterThan(version)),
                    "<" => Ok(VersionConstraint::LessThan(version)),
                    "==" | "=" => Ok(VersionConstraint::Exact(version)),
                    _ => unreachable!(),
                };
            }
        }

        // 无操作符：默认精确版本
        let version = Version::parse(s)?;
        Ok(VersionConstraint::Exact(version))
    }

    /// 格式化约束为字符串
    pub fn to_string(&self) -> String {
        match self {
            VersionConstraint::Exact(v) => format!("== {}", v),
            VersionConstraint::GreaterThanEqual(v) => format!(">= {}", v),
            VersionConstraint::GreaterThan(v) => format!("> {}", v),
            VersionConstraint::LessThan(v) => format!("< {}", v),
            VersionConstraint::Tilde(v) => format!("~> {}", v),
            VersionConstraint::Compatible(v) => format!("^{}", v),
            VersionConstraint::Range(s, e) => format!("{} .. {}", s, e),
            VersionConstraint::Any => "*".to_string(),
        }
    }
}

impl std::fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 依赖声明
// ─────────────────────────────────────────────────────────────────────────────

/// 依赖来源
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DependencySource {
    /// Git 仓库 URL
    Git(String),
    /// 本地路径
    Path(PathBuf),
    /// 二进制制品（.auz）路径（Phase 1 新增）
    Binary(PathBuf),
}

/// 依赖声明（技术方案 §8.2）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Dependency {
    /// 包名，如 "aura-json"
    pub name: String,
    /// 版本约束
    pub version: VersionConstraint,
    /// 来源（Git 仓库 URL 或本地路径）
    pub source: DependencySource,
}

impl Dependency {
    pub fn new(name: &str, version: VersionConstraint, source: DependencySource) -> Self {
        Self {
            name: name.to_string(),
            version,
            source,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 依赖声明解析（// @depends）
// ─────────────────────────────────────────────────────────────────────────────

/// 从源码中解析 `// @depends` 声明
///
/// 格式：
///   // @depends aura-json >= 1.0
///   // @depends aura-http from git@github.com:user/http.git
///   // @depends aura-raylib == 5.0
///   // @depends aura-http == 2.1 from git@github.com:user/http.git
pub fn parse_depends(source: &str) -> Vec<Dependency> {
    let mut deps = Vec::new();

    for line in source.lines() {
        let line = line.trim();
        // 仅处理 // @depends 行
        if !line.starts_with("//") || !line.to_lowercase().contains("@depends") {
            continue;
        }

        // 提取 @depends 之后的部分
        let rest = match line.split_once("@depends") {
            Some((_, r)) => r.trim().trim_start_matches('{').trim(),
            None => continue,
        };

        // 解析格式：name [version-constraint] [from source-url]
        let rest = rest.trim();
        if rest.is_empty() {
            continue;
        }

        // 分割：从右向左查找 "from" 关键字
        let (name_ver, source_part) = if let Some(idx) = rest.rfind(" from ") {
            (&rest[..idx], &rest[idx + 6..])
        } else {
            (rest, "")
        };

        // 解析包名和版本约束
        let parts: Vec<&str> = name_ver.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        let version_str = parts.get(1).map(|s| s.to_string()).unwrap_or_else(|| "*".to_string());

        let version = VersionConstraint::parse(&version_str).unwrap_or(VersionConstraint::Any);

        // 解析来源
        let source = if !source_part.trim().is_empty() {
            let source_str = source_part.trim();
            if source_str.starts_with("git@")
                || source_str.starts_with("https://")
                || source_str.starts_with("http://")
                || source_str.starts_with("ssh://")
            {
                DependencySource::Git(source_str.to_string())
            } else {
                DependencySource::Path(PathBuf::from(source_str))
            }
        } else {
            // 默认使用包名作为 Git 仓库
            DependencySource::Git(format!("https://github.com/aura-lang/{}.git", name))
        };

        deps.push(Dependency::new(&name, version, source));
    }

    deps
}

// ─────────────────────────────────────────────────────────────────────────────
// 包类型（Phase 1）
// ─────────────────────────────────────────────────────────────────────────────

/// 包类型：决定 `.auz` 内 `lib/` 和 `native/` 的必需性（设计方案 §9.1.1）
///
/// - `Bytecode`：仅字节码（`lib/` 必需，`native/` 不需要）
/// - `Hybrid`  ：混合（`lib/` + `native/` 都必需，推荐）
/// - `Native`  ：仅原生（`native/` 必需，`lib/` 可选）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageKind {
    /// 仅字节码（跨平台、性能不敏感）
    #[default]
    Bytecode,
    /// 混合（字节码 + AOT 原生，推荐）
    Hybrid,
    /// 仅原生库（平台锁定、性能敏感）
    Native,
}

impl PackageKind {
    /// 从字符串解析（TOML / CLI 输入）
    pub fn parse(s: &str) -> Result<PackageKind, PackageError> {
        match s.trim().to_lowercase().as_str() {
            "bytecode" | "bc" => Ok(PackageKind::Bytecode),
            "hybrid" | "mixed" => Ok(PackageKind::Hybrid),
            "native" | "aot" => Ok(PackageKind::Native),
            other => Err(PackageError::ParseError(format!(
                "invalid package type: {} (supported: bytecode, hybrid, native)",
                other
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PackageKind::Bytecode => "bytecode",
            PackageKind::Hybrid => "hybrid",
            PackageKind::Native => "native",
        }
    }
}

impl std::fmt::Display for PackageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 制品选项（Phase 1）
// ─────────────────────────────────────────────────────────────────────────────

/// `.auz` 打包选项（设计方案 §9.1 `[package]`）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PackageOptions {
    /// 容器格式：`apkg`（tar+zstd）| `source`（纯源码）
    #[serde(default = "default_pkg_format")]
    pub format: String,
    /// 是否打包源码附件到 `src/`
    #[serde(default, rename = "include-sources")]
    pub include_sources: bool,
    /// 是否打包文档到 `docs/`
    #[serde(default, rename = "include-docs")]
    pub include_docs: bool,
    /// 是否打包 AOT 原生库到 `native/`（hybrid/native 时必须为 true）
    #[serde(default, rename = "include-native")]
    pub include_native: bool,
    /// AOT 目标三元组列表（如 `["x86_64-pc-windows-msvc"]`）
    #[serde(default, rename = "native-targets")]
    pub native_targets: Vec<String>,
    /// AOT 优化级别（0/1/2/3）
    #[serde(default, rename = "aot-opt-level")]
    pub aot_opt_level: u32,
}

fn default_pkg_format() -> String {
    "auz".to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// 包清单（包元数据）
// ─────────────────────────────────────────────────────────────────────────────

/// 包清单（`aura.toml` 或 `.aura.toml`）
///
/// Phase 1 扩展字段（设计方案 §9.1）：
/// - `library` / `kind` / `compiler_min_version` / `compiler_max_version`
/// - `package`：制品选项
/// - `resources`：资源包含/排除模式
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackageManifest {
    /// Manifest 结构版本（与字节码版本独立）
    #[serde(default = "default_schema_version", rename = "schema-version")]
    pub schema_version: String,
    /// 包名
    pub name: String,
    /// 版本
    pub version: String,
    /// 描述
    #[serde(default)]
    pub description: Option<String>,
    /// 作者
    #[serde(default)]
    pub authors: Vec<String>,
    /// 许可证
    #[serde(default)]
    pub license: Option<String>,
    /// 仓库 URL
    #[serde(default)]
    pub repository: Option<String>,
    /// 入口文件（应用必填；库包可省略）
    #[serde(default = "default_entry")]
    pub entry: String,
    /// 依赖
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    /// 开发依赖
    #[serde(default, rename = "dev-dependencies")]
    pub dev_dependencies: Vec<Dependency>,
    /// 导出符号 / 模块名列表
    #[serde(default)]
    pub exports: Vec<String>,
    /// 平台限制（triple 列表）
    #[serde(default)]
    pub platforms: Vec<String>,

    // ── Phase 1 新增 ──
    /// 是否为库包（无 `entry` 入口）
    #[serde(default)]
    pub library: bool,
    /// 包类型（bytecode / hybrid / native）
    #[serde(default)]
    pub kind: PackageKind,
    /// 最低兼容编译器版本
    #[serde(default, rename = "compiler-min-version")]
    pub compiler_min_version: Option<String>,
    /// 最高兼容编译器版本
    #[serde(default, rename = "compiler-max-version")]
    pub compiler_max_version: Option<String>,
    /// 制品选项（`[package]` 表）
    #[serde(default)]
    pub package: PackageOptions,
    /// 资源包含/排除模式（`[resources]` 表）
    #[serde(default)]
    pub resources: ResourceConfig,
}

fn default_schema_version() -> String {
    "1.0".to_string()
}

fn default_entry() -> String {
    "main.aura".to_string()
}

/// 资源包含/排除配置（设计方案 §9.1 `[resources]`）
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResourceConfig {
    /// 包含模式（如 `["**/*.json", "**/*.html"]`）
    #[serde(default)]
    pub include: Vec<String>,
    /// 排除模式（如 `["**/*.test.*"]`）
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl PackageManifest {
    /// 从 TOML 文件解析包清单
    pub fn from_toml_file(path: &Path) -> Result<Self, PackageError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| PackageError::IoError(format!("cannot read {}: {}", path.display(), e)))?;
        Self::from_toml(&content)
    }

    /// 从 TOML 字符串解析（Phase 1 起改用真正的 TOML 序列化）
    pub fn from_toml(s: &str) -> Result<Self, PackageError> {
        toml::from_str(s)
            .map_err(|e| PackageError::ParseError(format!("cannot parse package manifest: {}", e)))
    }

    /// 序列化为 TOML
    pub fn to_toml(&self) -> Result<String, PackageError> {
        toml::to_string_pretty(self)
            .map_err(|e| PackageError::ParseError(format!("serialization failed: {}", e)))
    }

    /// 写入文件
    pub fn write_to_file(&self, path: &Path) -> Result<(), PackageError> {
        let content = self.to_toml()?;
        std::fs::write(path, content)
            .map_err(|e| PackageError::IoError(format!("cannot write {}: {}", path.display(), e)))
    }

    /// 判断是否为库包（Phase 1）
    pub fn is_library(&self) -> bool {
        self.library || self.entry.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 锁文件（aura.lock）
// ─────────────────────────────────────────────────────────────────────────────

/// 锁文件条目
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LockEntry {
    /// 包名
    pub name: String,
    /// 解析后的精确版本
    pub version: Version,
    /// 来源
    pub source: DependencySource,
    /// 提交哈希（Git）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// 校验和（SHA256）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// 锁文件
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LockFile {
    /// 锁文件版本
    pub version: u32,
    /// 锁定的依赖
    pub dependencies: Vec<LockEntry>,
}

impl LockFile {
    pub const FILENAME: &'static str = "aura.lock";
    pub const CURRENT_VERSION: u32 = 1;

    /// 从文件加载
    pub fn from_file(path: &Path) -> Result<Self, PackageError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| PackageError::IoError(format!("cannot read {}: {}", path.display(), e)))?;
        serde_json::from_str(&content).map_err(|e| {
            PackageError::ParseError(format!("cannot parse {}: {}", path.display(), e))
        })
    }

    /// 写入文件
    pub fn write_to_file(&self, path: &Path) -> Result<(), PackageError> {
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            PackageError::ParseError(format!("failed to serialize lock file: {}", e))
        })?;
        std::fs::write(path, content)
            .map_err(|e| PackageError::IoError(format!("cannot write {}: {}", path.display(), e)))
    }

    /// 查找包
    pub fn find(&self, name: &str) -> Option<&LockEntry> {
        self.dependencies.iter().find(|d| d.name == name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Git 操作封装
// ─────────────────────────────────────────────────────────────────────────────

/// Git 操作结果
#[derive(Debug)]
pub struct GitResult {
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

/// 执行 git 命令
fn git_command(args: &[&str], cwd: Option<&Path>) -> GitResult {
    let mut cmd = Command::new("git");
    for arg in args {
        cmd.arg(arg);
    }
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }

    let output = cmd.output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if o.status.success() {
                GitResult {
                    success: true,
                    output: stdout.trim().to_string(),
                    error: None,
                }
            } else {
                GitResult {
                    success: false,
                    output: stdout.trim().to_string(),
                    error: Some(stderr.trim().to_string()),
                }
            }
        }
        Err(e) => GitResult {
            success: false,
            output: String::new(),
            error: Some(e.to_string()),
        },
    }
}

/// 克隆 Git 仓库
pub fn git_clone(url: &str, dest: &Path) -> Result<String, PackageError> {
    // 检查目录是否已存在
    if dest.exists() {
        let rev = git_command(
            &[
                "rev-parse",
                "HEAD",
            ],
            Some(dest),
        );
        if rev.success {
            return Ok(rev.output);
        }
    }

    let result = git_command(
        &[
            "clone",
            "--depth",
            "1",
            url,
            dest.to_str().unwrap_or("."),
        ],
        None,
    );
    if result.success {
        let rev = git_command(
            &[
                "rev-parse",
                "HEAD",
            ],
            Some(dest),
        );
        Ok(rev.output)
    } else {
        Err(PackageError::GitError(format!(
            "git clone {} failed: {}",
            url,
            result.error.unwrap_or_else(|| result.output.clone())
        )))
    }
}

/// 获取 Git 仓库的最新版本标签
pub fn git_latest_tag(url: &str, cache_dir: &Path) -> Result<Version, PackageError> {
    let cache_path = cache_dir.join("latest_tags");
    let _ = std::fs::create_dir_all(&cache_path);

    let cache_file = cache_path.join(format!(
        "{}.tag",
        url.replace('/', "_").replace(':', "_").replace(' ', "_")
    ));

    // 检查缓存
    if cache_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&cache_file) {
            if let Ok(v) = Version::parse(content.trim()) {
                return Ok(v);
            }
        }
    }

    // 浅克隆获取标签
    let temp_dir = cache_path.join(format!(
        "{}_{}",
        url.replace('/', "_").replace(':', "_").replace(' ', "_"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let result = git_command(
        &[
            "clone",
            "--depth",
            "1",
            "--tags",
            url,
            temp_dir.to_str().unwrap_or("."),
        ],
        None,
    );
    if !result.success {
        return Err(PackageError::GitError(format!(
            "failed to get tags: {}",
            result.error.unwrap_or_default()
        )));
    }

    let tag_result = git_command(
        &[
            "describe",
            "--tags",
            "--abbrev=0",
        ],
        Some(&temp_dir),
    );
    let _ = std::fs::remove_dir_all(&temp_dir);

    if tag_result.success {
        let tag = tag_result.output.trim();
        let version = Version::parse(tag.trim_start_matches('v').trim_start_matches('V'));
        match version {
            Ok(v) => {
                let _ = std::fs::write(&cache_file, v.to_string());
                Ok(v)
            }
            Err(_) => Err(PackageError::InvalidVersion(format!(
                "invalid tag: {}",
                tag
            ))),
        }
    } else {
        Err(PackageError::GitError(format!(
            "tag not found: {}",
            tag_result.error.unwrap_or_default()
        )))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 包缓存管理
// ─────────────────────────────────────────────────────────────────────────────

/// 包缓存目录
#[derive(Debug, Clone)]
pub struct PackageCache {
    root: PathBuf,
}

impl PackageCache {
    /// 获取默认缓存目录
    pub fn default_cache() -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        Self {
            root: PathBuf::from(home).join(".aura").join("cache"),
        }
    }

    /// 从环境变量获取缓存目录
    pub fn from_env() -> Self {
        std::env::var("AURA_CACHE_DIR")
            .map(|p| Self {
                root: PathBuf::from(p),
            })
            .unwrap_or_else(|_| Self::default_cache())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 获取包的缓存路径
    pub fn package_dir(&self, name: &str) -> PathBuf {
        self.root.join("packages").join(name)
    }

    /// 获取包的版本目录
    pub fn version_dir(&self, name: &str, version: &Version) -> PathBuf {
        self.root.join("packages").join(name).join(version.to_string())
    }

    /// 检查包是否已缓存
    pub fn is_cached(&self, name: &str, version: &Version) -> bool {
        self.version_dir(name, version).join("installed").exists()
    }

    /// 标记包已安装
    pub fn mark_installed(&self, name: &str, version: &Version) -> Result<(), PackageError> {
        let dir = self.version_dir(name, version);
        std::fs::create_dir_all(&dir)
            .map_err(|e| PackageError::IoError(format!("cannot create cache directory: {}", e)))?;
        std::fs::write(dir.join("installed"), "")
            .map_err(|e| PackageError::IoError(format!("cannot mark as installed: {}", e)))
    }

    /// 获取缓存统计信息
    pub fn cache_stats(&self) -> CacheStats {
        let packages_dir = self.root.join("packages");
        let mut count = 0;
        let mut size = 0;
        if packages_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&packages_dir) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        count += 1;
                        size += dir_size(&entry.path());
                    }
                }
            }
        }
        CacheStats {
            package_count: count,
            total_size: size,
        }
    }

    /// 清理缓存
    pub fn clear(&self) -> Result<u64, PackageError> {
        let packages_dir = self.root.join("packages");
        let size = if packages_dir.exists() { dir_size(&packages_dir) } else { 0 };
        if packages_dir.exists() {
            std::fs::remove_dir_all(&packages_dir)
                .map_err(|e| PackageError::IoError(format!("cannot clear cache: {}", e)))?;
        }
        Ok(size)
    }
}

/// 缓存统计
#[derive(Debug, Default)]
pub struct CacheStats {
    pub package_count: u32,
    pub total_size: u64,
}

impl std::fmt::Display for CacheStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let size_mb = self.total_size as f64 / 1024.0 / 1024.0;
        write!(f, "{} packages, {:.1} MB", self.package_count, size_mb)
    }
}

fn dir_size(path: &Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                size += dir_size(&p);
            } else if let Ok(m) = entry.metadata() {
                size += m.len();
            }
        }
    }
    size
}

// ─────────────────────────────────────────────────────────────────────────────
// 依赖图
// ─────────────────────────────────────────────────────────────────────────────

/// 依赖图节点
#[derive(Debug, Clone)]
pub struct DepNode {
    pub name: String,
    pub version: Version,
    pub dependencies: Vec<String>,
    pub installed: bool,
}

/// 依赖图
#[derive(Debug)]
pub struct DependencyGraph {
    nodes: Vec<DepNode>,
    root: Option<String>,
}

impl DependencyGraph {
    pub fn new(root: Option<&str>) -> Self {
        Self {
            nodes: Vec::new(),
            root: root.map(|s| s.to_string()),
        }
    }

    pub fn add_node(&mut self, name: &str, version: Version, deps: Vec<String>) {
        self.nodes.push(DepNode {
            name: name.to_string(),
            version,
            dependencies: deps,
            installed: false,
        });
    }

    pub fn mark_installed(&mut self, name: &str) {
        for node in &mut self.nodes {
            if node.name == name {
                node.installed = true;
            }
        }
    }

    /// 生成依赖树文本表示
    pub fn to_tree_string(&self, indent: usize) -> String {
        let mut lines = Vec::new();
        for (i, node) in self.nodes.iter().enumerate() {
            if let Some(root) = &self.root {
                if node.name != *root {
                    continue;
                }
            }
            let prefix = "  ".repeat(indent);
            let bullet = if i == 0 { "└── " } else { "├── " };
            let status = if node.installed { "✓" } else { "○" };
            lines.push(format!(
                "{}{} {} v{} {}",
                prefix, bullet, node.name, node.version, status
            ));
            // 递归输出子依赖
            for dep in &node.dependencies {
                if let Some(dep_node) = self.nodes.iter().find(|n| n.name == *dep) {
                    lines.push(format!(
                        "{}    └── {} v{} {}",
                        prefix,
                        dep_node.name,
                        dep_node.version,
                        if dep_node.installed { "✓" } else { "○" }
                    ));
                }
            }
        }
        lines.join("\n")
    }

    /// 检查是否有版本冲突
    pub fn has_conflict(&self) -> Vec<String> {
        let mut conflicts = Vec::new();
        let mut seen: HashMap<String, Version> = HashMap::new();
        for node in &self.nodes {
            if let Some(prev_version) = seen.get(&node.name) {
                if prev_version != &node.version {
                    conflicts.push(format!(
                        "version conflict: {} needs v{} but v{} already exists",
                        node.name, node.version, prev_version
                    ));
                }
            } else {
                seen.insert(node.name.clone(), node.version.clone());
            }
        }
        conflicts
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 错误类型
// ─────────────────────────────────────────────────────────────────────────────

/// 包管理错误
#[derive(Debug)]
pub enum PackageError {
    InvalidVersion(String),
    ParseError(String),
    GitError(String),
    IoError(String),
    NotFound(String),
    Conflict(String),
    LockError(String),
    NetworkError(String),
    CacheError(String),
}

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackageError::InvalidVersion(msg) => write!(f, "Invalid version: {}", msg),
            PackageError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            PackageError::GitError(msg) => write!(f, "Git error: {}", msg),
            PackageError::IoError(msg) => write!(f, "IO error: {}", msg),
            PackageError::NotFound(msg) => write!(f, "Not found: {}", msg),
            PackageError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            PackageError::LockError(msg) => write!(f, "Lockfile error: {}", msg),
            PackageError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            PackageError::CacheError(msg) => write!(f, "Cache error: {}", msg),
        }
    }
}

impl std::error::Error for PackageError {}

// ─────────────────────────────────────────────────────────────────────────────
// 包管理器核心
// ─────────────────────────────────────────────────────────────────────────────

/// 包管理器配置
#[derive(Debug, Clone)]
pub struct PackageManagerConfig {
    pub cache: PackageCache,
    pub offline: bool,
    pub force_update: bool,
}

impl Default for PackageManagerConfig {
    fn default() -> Self {
        Self {
            cache: PackageCache::from_env(),
            offline: false,
            force_update: false,
        }
    }
}

/// 包管理器
pub struct PackageManager {
    config: PackageManagerConfig,
    manifest: Option<PackageManifest>,
    lock: Option<LockFile>,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            config: PackageManagerConfig::default(),
            manifest: None,
            lock: None,
        }
    }

    pub fn with_config(config: PackageManagerConfig) -> Self {
        Self {
            config,
            manifest: None,
            lock: None,
        }
    }

    /// 加载项目清单
    pub fn load_project(&mut self, project_dir: &Path) -> Result<(), PackageError> {
        let manifest_path = project_dir.join("aura.toml");
        if manifest_path.exists() {
            self.manifest = Some(PackageManifest::from_toml_file(&manifest_path)?);
        }

        let lock_path = project_dir.join(LockFile::FILENAME);
        if lock_path.exists() {
            self.lock = Some(LockFile::from_file(&lock_path)?);
        }

        Ok(())
    }

    /// 获取清单
    pub fn manifest(&self) -> &Option<PackageManifest> {
        &self.manifest
    }

    /// 获取锁文件
    pub fn lock(&self) -> &Option<LockFile> {
        &self.lock
    }

    /// 安装依赖（11.4）
    pub fn install(
        &mut self,
        project_dir: &Path,
        deps: &[Dependency],
    ) -> Result<LockFile, PackageError> {
        let lock_path = project_dir.join(LockFile::FILENAME);
        let mut lock = LockFile {
            version: LockFile::CURRENT_VERSION,
            dependencies: Vec::new(),
        };

        // 加载现有锁文件
        if lock_path.exists() {
            lock = LockFile::from_file(&lock_path)?;
        }

        // 处理每个依赖
        for dep in deps {
            let resolved = self.resolve_dependency(dep)?;
            lock.dependencies.push(resolved);
        }

        // 写入锁文件
        lock.write_to_file(&lock_path)?;
        self.lock = Some(lock.clone());

        Ok(lock)
    }

    /// 解析依赖（版本求解 + 下载）
    fn resolve_dependency(&mut self, dep: &Dependency) -> Result<LockEntry, PackageError> {
        // 查找版本
        let version = self.resolve_version(dep)?;

        // 克隆仓库
        let _cache_dir = self.config.cache.package_dir(&dep.name);
        let version_dir = self.config.cache.version_dir(&dep.name, &version);

        let rev = match &dep.source {
            DependencySource::Git(url) => {
                if self.config.offline {
                    // 离线模式：检查缓存
                    if !self.config.cache.is_cached(&dep.name, &version) {
                        return Err(PackageError::CacheError(format!(
                            "{} v{} not cached in offline mode",
                            dep.name, version
                        )));
                    }
                    let rev = git_command(
                        &[
                            "rev-parse",
                            "HEAD",
                        ],
                        Some(&version_dir),
                    );
                    rev.output
                } else {
                    git_clone(url, &version_dir)?
                }
            }
            DependencySource::Path(path) => {
                let rev = git_command(
                    &[
                        "rev-parse",
                        "HEAD",
                    ],
                    Some(path),
                );
                rev.output
            }
            DependencySource::Binary(path) => {
                // Phase 1: 二进制制品 — 用文件路径作为 rev
                path.display().to_string()
            }
        };

        // 标记已安装
        self.config.cache.mark_installed(&dep.name, &version)?;

        Ok(LockEntry {
            name: dep.name.clone(),
            version,
            source: dep.source.clone(),
            rev: Some(rev),
            checksum: None,
        })
    }

    /// 版本求解（11.2）
    fn resolve_version(&self, dep: &Dependency) -> Result<Version, PackageError> {
        match &dep.source {
            DependencySource::Path(path) => {
                // 本地路径：从 aura.toml 获取版本
                let manifest_path = path.join("aura.toml");
                if manifest_path.exists() {
                    let manifest = PackageManifest::from_toml_file(&manifest_path)?;
                    Version::parse(&manifest.version)
                } else {
                    Err(PackageError::NotFound(format!(
                        "local path {} has no aura.toml",
                        path.display()
                    )))
                }
            }
            DependencySource::Git(url) => {
                // Git 仓库：获取标签
                if self.config.offline {
                    Err(PackageError::CacheError(
                        "cannot resolve version in offline mode".to_string(),
                    ))
                } else {
                    git_latest_tag(url, self.config.cache.root())
                }
            }
            DependencySource::Binary(path) => {
                // Phase 1: 从 .auz 文件的 manifest 读取版本
                use crate::auz::PackageReader;
                let content = PackageReader::from_file(path).map_err(|e| {
                    PackageError::ParseError(format!(
                        "failed to read .auz {}: {}",
                        path.display(),
                        e
                    ))
                })?;
                Version::parse(&content.manifest.version)
            }
        }
    }

    /// 更新依赖（11.5）
    pub fn update(
        &mut self,
        project_dir: &Path,
        force_all: bool,
    ) -> Result<Vec<String>, PackageError> {
        let manifest = self
            .manifest
            .as_ref()
            .ok_or_else(|| PackageError::ParseError("aura.toml not found".to_string()))?
            .clone();

        let mut updated = Vec::new();
        let mut new_lock = LockFile {
            version: LockFile::CURRENT_VERSION,
            dependencies: Vec::new(),
        };

        for dep in &manifest.dependencies {
            if force_all {
                let entry = self.resolve_dependency(dep)?;
                updated.push(format!("{} → v{}", dep.name, entry.version));
                new_lock.dependencies.push(entry);
            } else {
                if let Some(lock) = &self.lock {
                    if let Some(entry) = lock.find(&dep.name) {
                        new_lock.dependencies.push(entry.clone());
                    } else {
                        let entry = self.resolve_dependency(dep)?;
                        updated.push(format!("{} (new) v{}", dep.name, entry.version));
                        new_lock.dependencies.push(entry);
                    }
                } else {
                    let entry = self.resolve_dependency(dep)?;
                    updated.push(format!("{} (new) v{}", dep.name, entry.version));
                    new_lock.dependencies.push(entry);
                }
            }
        }

        let lock_path = project_dir.join(LockFile::FILENAME);
        new_lock.write_to_file(&lock_path)?;
        self.lock = Some(new_lock);

        Ok(updated)
    }

    /// 发布包（11.6）
    pub fn publish(&self, package_dir: &Path) -> Result<String, PackageError> {
        let manifest_path = package_dir.join("aura.toml");
        let manifest = PackageManifest::from_toml_file(&manifest_path)?;

        // 检查仓库配置
        let repo = manifest.repository.ok_or_else(|| {
            PackageError::ParseError("aura.toml missing repository field".to_string())
        })?;

        // 创建标签
        let version_str = manifest.version.clone();
        let result = git_command(
            &[
                "tag",
                &format!("v{}", version_str),
            ],
            Some(package_dir),
        );
        if !result.success {
            return Err(PackageError::GitError(format!(
                "failed to create tag: {}",
                result.error.unwrap_or_default()
            )));
        }

        // 推送标签
        let push_result = git_command(
            &[
                "push",
                "origin",
                &format!("v{}", version_str),
            ],
            Some(package_dir),
        );
        if !push_result.success {
            return Err(PackageError::GitError(format!(
                "failed to push tag: {}",
                push_result.error.unwrap_or_default()
            )));
        }

        Ok(format!(
            "published {} v{} to {}",
            manifest.name, version_str, repo
        ))
    }

    /// 显示依赖树（11.7）
    pub fn show_deps(&self, project_dir: &Path) -> Result<String, PackageError> {
        // 解析源码中的 @depends
        let mut deps = Vec::new();
        let mut graph = DependencyGraph::new(Some("project"));

        // 扫描所有 .aura 文件
        let entries = std::fs::read_dir(project_dir);
        if let Ok(entries) = entries {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "aura").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        deps.extend(parse_depends(&content));
                    }
                }
            }
        }

        // 也检查 aura.toml
        if let Some(manifest) = &self.manifest {
            deps.extend(manifest.dependencies.clone());
        }

        if deps.is_empty() {
            return Ok("no dependencies".to_string());
        }

        // 构建依赖图
        for dep in &deps {
            let version = match self.lock.as_ref() {
                Some(lock) => lock.find(&dep.name).map(|e| e.version.clone()),
                None => None,
            };
            match version {
                Some(v) => {
                    graph.add_node(&dep.name, v, vec![]);
                    if let Some(lock) = &self.lock {
                        if lock.find(&dep.name).is_some() {
                            graph.mark_installed(&dep.name);
                        }
                    }
                }
                None => {
                    let resolved = Version::new(0, 0, 0);
                    graph.add_node(&dep.name, resolved, vec![]);
                }
            }
        }

        Ok(graph.to_tree_string(0))
    }

    /// 离线模式验证（11.9）
    pub fn verify_offline(&self, project_dir: &Path) -> Result<Vec<String>, PackageError> {
        let lock_path = project_dir.join(LockFile::FILENAME);
        let lock = LockFile::from_file(&lock_path)?;

        let mut problems = Vec::new();
        for entry in &lock.dependencies {
            if !self.config.cache.is_cached(&entry.name, &entry.version) {
                problems.push(format!("{} v{} not cached", entry.name, entry.version));
            }
        }

        Ok(problems)
    }

    /// 创建新包项目（aura new）
    pub fn create_new_package(name: &str, dir: &Path) -> Result<(), PackageError> {
        let project_path = dir.join(name);

        // 创建目录
        std::fs::create_dir_all(&project_path)
            .map_err(|e| PackageError::IoError(format!("cannot create directory: {}", e)))?;

        // 创建 aura.toml
        let manifest = PackageManifest {
            schema_version: "1.0".to_string(),
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: Some(format!("{} package", name)),
            authors: vec![],
            license: Some("MIT".to_string()),
            repository: Some(format!("https://github.com/aura-lang/{}.git", name)),
            entry: "main.aura".to_string(),
            dependencies: Vec::new(),
            dev_dependencies: Vec::new(),
            exports: vec!["main".to_string()],
            platforms: vec![],
            library: false,
            kind: PackageKind::Bytecode,
            compiler_min_version: Some("0.3.0".to_string()),
            compiler_max_version: None,
            package: PackageOptions::default(),
            resources: ResourceConfig::default(),
        };
        manifest.write_to_file(&project_path.join("aura.toml"))?;

        // 创建主入口文件
        let main_content = format!(
            r#"// {} - Aura 包
// 版本: 0.1.0

public fun main() {{
    println("Hello from {}!")
}}

// 测试
// aura run main.aura
"#,
            name, name
        );
        std::fs::write(project_path.join("main.aura"), main_content)
            .map_err(|e| PackageError::IoError(format!("cannot write main.aura: {}", e)))?;

        // 创建 .gitignore
        let gitignore = "# Build artifacts\n*.auc\n*.exe\n*.o\n*.obj\n*.ll\n\n# Cache\n.aura-cache/\n\n# Dependencies\nvendor/\n";
        std::fs::write(project_path.join(".gitignore"), gitignore)
            .map_err(|e| PackageError::IoError(format!("cannot write .gitignore: {}", e)))?;

        // 初始化 Git 仓库
        let _ = git_command(&["init"], Some(&project_path));

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // 11.1 依赖声明解析
    #[test]
    fn test_parse_depends_basic() {
        let source = r#"
// @depends aura-json >= 1.0
// @depends aura-http == 2.1
import json
"#;
        let deps = parse_depends(source);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "aura-json");
        assert_eq!(deps[1].name, "aura-http");
    }

    #[test]
    fn test_parse_depends_with_source() {
        let source = r#"
// @depends aura-http from git@github.com:user/http.git
// @depends aura-raylib == 5.0
"#;
        let deps = parse_depends(source);
        assert_eq!(deps.len(), 2);
        assert!(matches!(deps[0].source, DependencySource::Git(_)));
        assert!(matches!(deps[1].source, DependencySource::Git(_)));
    }

    #[test]
    fn test_parse_depends_with_local_path() {
        let source = r#"
// @depends local-lib from ../local-lib
"#;
        let deps = parse_depends(source);
        assert_eq!(deps.len(), 1);
        assert!(matches!(deps[0].source, DependencySource::Path(_)));
    }

    #[test]
    fn test_parse_depends_ignores_comments() {
        let source = r#"
// 这是一个普通注释
// @notdepends aura-json >= 1.0
import json
"#;
        let deps = parse_depends(source);
        assert_eq!(deps.len(), 0);
    }

    // 11.2 版本约束求解
    #[test]
    fn test_version_parse() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_version_parse_prerelease() {
        let v = Version::parse("1.2.3-rc.1").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.prerelease, vec!["rc", "1"]);
    }

    #[test]
    fn test_version_parse_build() {
        let v = Version::parse("1.2.3+build.123").unwrap();
        assert_eq!(
            v.build,
            vec![
                "build", "123"
            ]
        );
    }

    #[test]
    fn test_version_parse_invalid() {
        assert!(Version::parse("invalid").is_err());
        assert!(Version::parse("1").is_err());
    }

    #[test]
    fn test_version_ordering() {
        let v1 = Version::new(1, 0, 0);
        let v2 = Version::new(1, 1, 0);
        let v3 = Version::new(2, 0, 0);
        assert!(v1 < v2);
        assert!(v2 < v3);
    }

    #[test]
    fn test_constraint_exact() {
        let c = VersionConstraint::Exact(Version::new(1, 2, 3));
        let v = Version::new(1, 2, 3);
        assert!(v.matches_constraint(&c));
        assert!(!v.matches_constraint(&VersionConstraint::Exact(Version::new(1, 2, 4))));
    }

    #[test]
    fn test_constraint_gte() {
        let c = VersionConstraint::GreaterThanEqual(Version::new(1, 0, 0));
        assert!(Version::new(1, 0, 0).matches_constraint(&c));
        assert!(Version::new(1, 1, 0).matches_constraint(&c));
        assert!(!Version::new(0, 9, 0).matches_constraint(&c));
    }

    #[test]
    fn test_constraint_tilde() {
        let c = VersionConstraint::Tilde(Version::new(1, 0, 0));
        // ~1.0 表示 >=1.0, <2.0
        assert!(Version::new(1, 0, 0).matches_constraint(&c));
        assert!(Version::new(1, 5, 0).matches_constraint(&c));
        assert!(!Version::new(2, 0, 0).matches_constraint(&c));
    }

    #[test]
    fn test_constraint_compatible() {
        let c = VersionConstraint::Compatible(Version::new(1, 0, 0));
        assert!(Version::new(1, 5, 0).matches_constraint(&c));
        assert!(!Version::new(2, 0, 0).matches_constraint(&c));

        let c0 = VersionConstraint::Compatible(Version::new(0, 5, 0));
        assert!(Version::new(0, 5, 0).matches_constraint(&c0));
        assert!(!Version::new(0, 6, 0).matches_constraint(&c0));
    }

    #[test]
    fn test_constraint_parse() {
        let c = VersionConstraint::parse(">= 1.0").unwrap();
        assert!(matches!(c, VersionConstraint::GreaterThanEqual(_)));

        let c = VersionConstraint::parse("== 1.2").unwrap();
        assert!(matches!(c, VersionConstraint::Exact(_)));

        let c = VersionConstraint::parse("~> 1.0").unwrap();
        assert!(matches!(c, VersionConstraint::Tilde(_)));

        let c = VersionConstraint::parse("^1.0").unwrap();
        assert!(matches!(c, VersionConstraint::Compatible(_)));

        let c = VersionConstraint::parse("*").unwrap();
        assert!(matches!(c, VersionConstraint::Any));
    }

    // 11.3 Git 仓库克隆与缓存
    #[test]
    fn test_cache_default_path() {
        let cache = PackageCache::default_cache();
        assert!(cache.root().display().to_string().contains(".aura"));
    }

    #[test]
    fn test_cache_package_dir() {
        let cache = PackageCache::from_env();
        let dir = cache.package_dir("test-pkg");
        assert!(dir.display().to_string().ends_with("test-pkg"));
    }

    #[test]
    fn test_cache_version_dir() {
        let cache = PackageCache::from_env();
        let v = Version::new(1, 2, 3);
        let dir = cache.version_dir("test-pkg", &v);
        assert!(dir.display().to_string().ends_with("1.2.3"));
    }

    // 11.4 aura install
    #[test]
    fn test_install_creates_lock() {
        let tmp = std::env::temp_dir().join(format!("aura_test_install_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // 创建一个模拟的 aura.toml（Phase 1 起使用真正的 TOML 格式）
        std::fs::write(
            tmp.join("aura.toml"),
            r#"
schema-version = "1.0"
name = "test-lib"
version = "0.1.0"
entry = "main.aura"
dependencies = []
dev_dependencies = []
exports = []
platforms = []
library = false
kind = "bytecode"
"#,
        )
        .unwrap();

        let dep = Dependency::new(
            "test-lib",
            VersionConstraint::Any,
            DependencySource::Path(tmp.clone()),
        );

        let mut pm = PackageManager::new();
        let result = pm.install(&tmp, &[dep]);
        assert!(result.is_ok());

        let lock_path = tmp.join(LockFile::FILENAME);
        assert!(lock_path.exists());

        // 清理
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.5 aura update
    #[test]
    fn test_update_requires_manifest() {
        let tmp = std::env::temp_dir().join(format!("aura_test_update_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let mut pm = PackageManager::new();
        // 没有 aura.toml 应该失败
        assert!(pm.update(&tmp, false).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.6 aura publish
    #[test]
    fn test_publish_requires_manifest() {
        let tmp = std::env::temp_dir().join(format!("aura_test_publish_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let pm = PackageManager::new();
        assert!(pm.publish(&tmp).is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.7 aura deps
    #[test]
    fn test_show_deps_empty() {
        let tmp = std::env::temp_dir().join(format!("aura_test_deps_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let pm = PackageManager::new();
        let result = pm.show_deps(&tmp).unwrap();
        assert!(result.contains("no dependencies"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.8 锁文件
    #[test]
    fn test_lock_file_write_read() {
        let tmp = std::env::temp_dir().join(format!("aura_test_lock_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let mut lock = LockFile::default();
        lock.dependencies.push(LockEntry {
            name: "test-lib".to_string(),
            version: Version::new(1, 2, 3),
            source: DependencySource::Git("https://github.com/test/lib.git".to_string()),
            rev: Some("abc123".to_string()),
            checksum: None,
        });

        let path = tmp.join("aura.lock");
        lock.write_to_file(&path).unwrap();

        let loaded = LockFile::from_file(&path).unwrap();
        assert_eq!(loaded.dependencies.len(), 1);
        assert_eq!(loaded.dependencies[0].name, "test-lib");
        assert_eq!(loaded.dependencies[0].version, Version::new(1, 2, 3));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.9 离线模式
    #[test]
    fn test_offline_verify() {
        let tmp = std::env::temp_dir().join(format!("aura_test_offline_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        // 创建空锁文件
        let lock = LockFile::default();
        lock.write_to_file(&tmp.join("aura.lock")).unwrap();

        let pm = PackageManager::new();
        let problems = pm.verify_offline(&tmp).unwrap();
        assert!(problems.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 11.10 测试
    #[test]
    fn test_create_new_package() {
        let tmp = std::env::temp_dir().join(format!("aura_test_new_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);

        let result = PackageManager::create_new_package("my-lib", &tmp);
        assert!(result.is_ok());

        let pkg_dir = tmp.join("my-lib");
        assert!(pkg_dir.join("aura.toml").exists());
        assert!(pkg_dir.join("main.aura").exists());
        assert!(pkg_dir.join(".gitignore").exists());

        // 读取 manifest
        let manifest = PackageManifest::from_toml_file(&pkg_dir.join("aura.toml")).unwrap();
        assert_eq!(manifest.name, "my-lib");
        assert_eq!(manifest.version, "0.1.0");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 依赖图测试
    #[test]
    fn test_dependency_graph_conflict() {
        let mut graph = DependencyGraph::new(Some("root"));
        graph.add_node("lib-a", Version::new(1, 0, 0), vec!["lib-c".to_string()]);
        graph.add_node("lib-b", Version::new(2, 0, 0), vec!["lib-c".to_string()]);
        graph.add_node("lib-c", Version::new(1, 0, 0), vec![]);

        let conflicts = graph.has_conflict();
        assert!(conflicts.is_empty());

        // 添加版本冲突
        graph.add_node("lib-c", Version::new(2, 0, 0), vec![]);
        let conflicts = graph.has_conflict();
        assert!(!conflicts.is_empty());
    }

    #[test]
    fn test_dependency_graph_tree() {
        let mut graph = DependencyGraph::new(Some("app"));
        graph.add_node(
            "app",
            Version::new(1, 0, 0),
            vec![
                "lib-a".to_string(),
                "lib-b".to_string(),
            ],
        );
        graph.add_node("lib-a", Version::new(2, 0, 0), vec!["lib-c".to_string()]);
        graph.add_node("lib-b", Version::new(3, 0, 0), vec![]);
        graph.add_node("lib-c", Version::new(1, 0, 0), vec![]);

        let tree = graph.to_tree_string(0);
        assert!(tree.contains("app"));
        assert!(tree.contains("lib-a"));
    }

    // 清单解析测试
    #[test]
    fn test_manifest_from_toml() {
        let toml = r#"
schema-version = "1.0"
name = "test-package"
version = "1.0.0"
description = "A test package"
authors = ["Author"]
license = "MIT"
repository = "https://github.com/test/pkg"
entry = "main.aura"
library = false
kind = "bytecode"

[[dependencies]]
name = "dep-a"
version = { GreaterThanEqual = { major = 1, minor = 0, patch = 0, prerelease = [], build = [] } }
source = { Git = "https://github.com/test/dep-a" }
"#;

        let manifest = PackageManifest::from_toml(toml).unwrap();
        assert_eq!(manifest.name, "test-package");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.description.as_deref(), Some("A test package"));
    }

    /// Phase 1: 测试新字段（library/kind/compiler-min-version 等）解析
    #[test]
    fn test_manifest_phase1_fields() {
        let toml = r#"
schema-version = "1.0"
name = "aura-json"
version = "1.2.3"
library = true
kind = "hybrid"
compiler-min-version = "0.3.0"
compiler-max-version = "1.0"

[package]
format = "auz"
include-sources = false
include-native = true
native-targets = ["x86_64-pc-windows-msvc"]
aot-opt-level = 2

[resources]
include = ["**/*.json"]
exclude = ["**/*.test.*"]
"#;

        let manifest = PackageManifest::from_toml(toml).unwrap();
        assert!(manifest.library);
        assert_eq!(manifest.kind, PackageKind::Hybrid);
        assert_eq!(manifest.compiler_min_version.as_deref(), Some("0.3.0"));
        assert_eq!(manifest.compiler_max_version.as_deref(), Some("1.0"));
        assert_eq!(manifest.package.include_sources, false);
        assert_eq!(manifest.package.include_native, true);
        assert_eq!(manifest.package.native_targets.len(), 1);
        assert_eq!(manifest.package.aot_opt_level, 2);
        assert_eq!(manifest.resources.include.len(), 1);
    }

    #[test]
    fn test_manifest_default_entry() {
        let manifest = PackageManifest {
            schema_version: "1.0".to_string(),
            name: "test".to_string(),
            version: "0.1.0".to_string(),
            description: None,
            authors: vec![],
            license: None,
            repository: None,
            entry: default_entry(),
            dependencies: vec![],
            dev_dependencies: vec![],
            exports: vec![],
            platforms: vec![],
            library: false,
            kind: PackageKind::Bytecode,
            compiler_min_version: None,
            compiler_max_version: None,
            package: PackageOptions::default(),
            resources: ResourceConfig::default(),
        };
        assert_eq!(manifest.entry, "main.aura");
        assert!(!manifest.library);
        assert_eq!(manifest.kind, PackageKind::Bytecode);
    }

    // 版本约束格式化测试
    #[test]
    fn test_constraint_to_string() {
        let c = VersionConstraint::GreaterThanEqual(Version::new(1, 0, 0));
        assert_eq!(c.to_string(), ">= 1.0.0");

        let c = VersionConstraint::Exact(Version::new(2, 1, 0));
        assert_eq!(c.to_string(), "== 2.1.0");

        let c = VersionConstraint::Tilde(Version::new(3, 0, 0));
        assert_eq!(c.to_string(), "~> 3.0.0");

        let c = VersionConstraint::Compatible(Version::new(4, 5, 0));
        assert_eq!(c.to_string(), "^4.5.0");

        let c = VersionConstraint::Any;
        assert_eq!(c.to_string(), "*");
    }
}
