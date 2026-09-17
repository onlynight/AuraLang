//! Phase 1: `.auz` 制品格式（tar + zstd 容器）
//!
//! 对应 设计方案 §5（容器格式选型）+ §5.1（目录布局）+ §5.2（魔数与校验）+ §13.1（校验和）。
//!
//! 容器：`.auz` = zstd 压缩的 POSIX ustar 归档。
//! - 魔数：28 B5 2F FD（zstd 标准，前 4 字节）
//! - zstd 内置 xxhash64 校验，解压时自动验证完整性
//!
//! 逻辑布局（tar 内路径）：
//!   foo-1.2.3.auz/
//!     META-INF/aura.toml           主清单（必需）
//!     META-INF/checksum.sha256     全包校验和（必需，Phase 1）
//!     META-INF/signature.sig       HMAC 签名（可选，Phase 4）
//!     lib/name-version/*.auc       字节码（bytecode/hybrid 必需）
//!     ref/index.json               类型签名索引（Phase 3，Phase 1 占位）
//!     native/triple/...            AOT 原生库（Phase 4，hybrid/native 必需）
//!     src/                         源码附件（可选）
//!     docs/                        文档（可选）
//!     resources/                   非代码资源（可选）
//!     test/                        测试用例（可选）
//!
//! 错误处理：所有 I/O / 格式 / 校验错误统一通过 ApkgError 返回。

use std::fmt;

pub mod builder;
pub mod checksum;
pub mod reader;

pub use builder::{BuildResult, PackageBuildOptions, PackageBuilder};
pub use reader::{PackageContent, PackageFileInfo, PackageReader};

// ─────────────────────────────────────────────────────────────────────────────
// 魔数与常量
// ─────────────────────────────────────────────────────────────────────────────

/// zstd 压缩流的魔数（4 字节，小端表示为 0xFD2FB528）
pub const ZSTD_MAGIC: [u8; 4] = [
    0x28, 0xB5, 0x2F, 0xFD,
];

/// .auz 格式版本（Phase 1 = 1）
pub const APKG_FORMAT_VERSION: u32 = 1;

/// zstd 默认压缩级别（设计方案 §5.0.7：level 3，压缩比与速度平衡）
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

// ─────────────────────────────────────────────────────────────────────────────
// 容器内路径约定
// ─────────────────────────────────────────────────────────────────────────────

/// META-INF 目录（元数据）
pub const META_INF_DIR: &str = "META-INF";
/// 主清单文件
pub const MANIFEST_FILENAME: &str = "META-INF/aura.toml";
/// 校验和文件
pub const CHECKSUM_FILENAME: &str = "META-INF/checksum.sha256";
/// 签名文件（Phase 4）
pub const SIGNATURE_FILENAME: &str = "META-INF/signature.sig";

/// 字节码目录
pub const LIB_DIR: &str = "lib";
/// 类型签名目录
pub const REF_DIR: &str = "ref";
/// 类型签名索引
pub const REF_INDEX_FILENAME: &str = "ref/index.json";
/// AOT 原生库目录
pub const NATIVE_DIR: &str = "native";
/// 源码附件目录
pub const SRC_DIR: &str = "src";
/// 文档目录
pub const DOCS_DIR: &str = "docs";
/// 资源目录
pub const RESOURCES_DIR: &str = "resources";
/// 测试目录
pub const TEST_DIR: &str = "test";

/// 检查路径是否在给定目录下
pub fn path_under(dir: &str, path: &str) -> bool {
    path.starts_with(dir) && (path.len() == dir.len() || path.as_bytes()[dir.len()] == b'/')
}

// ─────────────────────────────────────────────────────────────────────────────
// 错误类型
// ─────────────────────────────────────────────────────────────────────────────

/// .auz 格式错误
#[derive(Debug)]
pub enum ApkgError {
    /// I/O 错误（读写失败）
    Io(String),
    /// 格式错误（魔数不匹配、tar 损坏等）
    Format(String),
    /// 校验和错误
    Checksum(String),
    /// 清单错误
    Manifest(String),
    /// 压缩/解压错误
    Compression(String),
    /// 打包错误（构建时）
    Build(String),
}

impl std::fmt::Display for ApkgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApkgError::Io(m) => write!(f, "IO error: {}", m),
            ApkgError::Format(m) => write!(f, "format error: {}", m),
            ApkgError::Checksum(m) => write!(f, "checksum error: {}", m),
            ApkgError::Manifest(m) => write!(f, "manifest error: {}", m),
            ApkgError::Compression(m) => write!(f, "compression error: {}", m),
            ApkgError::Build(m) => write!(f, "packaging error: {}", m),
        }
    }
}

impl std::error::Error for ApkgError {}

impl From<std::io::Error> for ApkgError {
    fn from(e: std::io::Error) -> Self {
        ApkgError::Io(e.to_string())
    }
}

impl From<toml::ser::Error> for ApkgError {
    fn from(e: toml::ser::Error) -> Self {
        ApkgError::Manifest(e.to_string())
    }
}

impl From<toml::de::Error> for ApkgError {
    fn from(e: toml::de::Error) -> Self {
        ApkgError::Manifest(e.to_string())
    }
}
