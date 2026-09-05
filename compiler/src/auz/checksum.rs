//! Phase 1 §13.1: SHA-256 校验和生成与验证
//!
//! 格式与 `sha256sum` 一致（`<hex-hash>  <path>`），便于 CLI 工具处理。

use sha2::{Digest, Sha256};
use std::path::Path;

use super::ApkgError;

/// 计算字节的 SHA-256（小写 hex 字符串）
pub fn compute_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// 计算文件的 SHA-256
pub fn compute_file_sha256(path: &Path) -> Result<String, ApkgError> {
    let bytes = std::fs::read(path).map_err(|e| {
        ApkgError::Io(format!("无法读取 {}: {}", path.display(), e))
    })?;
    Ok(compute_sha256(&bytes))
}

/// 校验和文件条目（与 `sha256sum` 输出格式一致）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChecksumEntry {
    /// SHA-256（小写 hex）
    pub hash: String,
    /// 文件在 `.auz` 内的路径（POSIX 风格，如 `META-INF/aura.toml`）
    pub path: String,
}

impl ChecksumEntry {
    /// 格式化为 `sha256sum` 一行
    pub fn to_line(&self) -> String {
        format!("{}  {}", self.hash, self.path)
    }

    /// 从 `sha256sum` 一行解析
    pub fn parse_line(line: &str) -> Option<Self> {
        // sha256sum 格式：<64 hex>  <path>（两个空格分隔）
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        // 用双空格或单空格分隔
        let (hash, path) = line.split_once("  ").or_else(|| line.split_once(' '))?;
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(ChecksumEntry {
            hash: hash.to_string(),
            path: path.to_string(),
        })
    }
}

/// 生成 `checksum.sha256` 文件内容
pub fn generate_checksum_file(entries: &[ChecksumEntry]) -> String {
    entries.iter().map(|e| e.to_line()).collect::<Vec<_>>().join("\n") + "\n"
}

/// 解析 `checksum.sha256` 文件内容
pub fn parse_checksum_file(content: &str) -> Vec<ChecksumEntry> {
    content
        .lines()
        .filter_map(ChecksumEntry::parse_line)
        .collect()
}

/// 验证 `.auz` 内某文件内容的 SHA-256 是否与预期匹配
pub fn verify_bytes(expected_hash: &str, bytes: &[u8]) -> Result<(), ApkgError> {
    let actual = compute_sha256(bytes);
    if !expected_hash.eq_ignore_ascii_case(&actual) {
        return Err(ApkgError::Checksum(format!(
            "校验和不匹配: 期望 {}, 实际 {}",
            expected_hash, actual
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sha256_empty() {
        // SHA-256 of empty string
        let hash = compute_sha256(b"");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_compute_sha256_hello() {
        let hash = compute_sha256(b"hello");
        assert_eq!(hash, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
    }

    #[test]
    fn test_checksum_entry_roundtrip() {
        let entry = ChecksumEntry {
            hash: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string(),
            path: "META-INF/aura.toml".to_string(),
        };
        let line = entry.to_line();
        assert_eq!(line, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  META-INF/aura.toml");

        let parsed = ChecksumEntry::parse_line(&line);
        assert!(parsed.is_some());
        assert_eq!(parsed.unwrap(), entry);
    }

    #[test]
    fn test_parse_checksum_file() {
        let content = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  META-INF/aura.toml\nabcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789  lib/test.auc\n";
        let entries = parse_checksum_file(content);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "META-INF/aura.toml");
        assert_eq!(entries[1].path, "lib/test.auc");
    }

    #[test]
    fn test_verify_bytes_ok() {
        let data = b"test data";
        let hash = compute_sha256(data);
        assert!(verify_bytes(&hash, data).is_ok());
    }

    #[test]
    fn test_verify_bytes_fail() {
        let data = b"test data";
        assert!(verify_bytes("wrong_hash", data).is_err());
    }
}
