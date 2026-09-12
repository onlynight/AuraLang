//! Phase 6: Standard library package reader.
//!
//! Reads a .auz standard library artifact, verifying checksum and extracting
//! .auc files, native libraries, and FFI function mappings.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

use super::builder::StdlibManifest;

/// Result of reading a stdlib .auz package.
#[derive(Debug)]
pub struct StdlibPackageContent {
    /// Package manifest
    pub manifest: StdlibManifest,
    /// All extracted files (path -> content)
    pub files: BTreeMap<String, Vec<u8>>,
    /// Whether checksum verification passed
    pub verified: bool,
    /// Number of .auc files found
    pub auc_file_count: usize,
    /// Number of native library files found
    pub native_file_count: usize,
}

impl StdlibPackageContent {
    /// Get all .auc file paths.
    pub fn auc_files(&self) -> Vec<&str> {
        self.files.keys().filter(|p| p.ends_with(".auc")).map(|s| s.as_str()).collect()
    }

    /// Get FFI cache/symbol files.
    pub fn ffi_files(&self) -> Vec<&str> {
        self.files.keys().filter(|p| p.contains("ffi_")).map(|s| s.as_str()).collect()
    }

    /// Get native library files.
    pub fn native_files(&self) -> Vec<&str> {
        self.files.keys().filter(|p| p.starts_with("native/")).map(|s| s.as_str()).collect()
    }

    /// Get source files.
    pub fn source_files(&self) -> Vec<&str> {
        self.files.keys().filter(|p| p.starts_with("src/")).map(|s| s.as_str()).collect()
    }
}

/// Read a stdlib .auz package.
pub fn read_auz_package(path: &Path) -> Result<StdlibPackageContent, String> {
    // 1. Read file
    let compressed =
        std::fs::read(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;

    // 2. Verify zstd magic
    if compressed.len() < 4
        || compressed[..4]
            != [
                0x28, 0xB5, 0x2F, 0xFD,
            ]
    {
        return Err(format!(
            "Not a valid .auz file (missing zstd magic): {}",
            path.display()
        ));
    }

    // 3. Decompress zstd
    let tar_data = zstd::decode_all(&compressed[..])
        .map_err(|e| format!("zstd decompression failed: {}", e))?;

    // 4. Extract tar
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut archive = tar::Archive::new(&tar_data[..]);

    for entry in archive.entries().map_err(|e| format!("tar entries error: {}", e))? {
        let mut entry = entry.map_err(|e| format!("tar entry error: {}", e))?;
        let path_str = entry
            .path()
            .map_err(|e| format!("tar path error: {}", e))?
            .to_string_lossy()
            .to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).map_err(|e| format!("read {} failed: {}", path_str, e))?;
        files.insert(path_str, content);
    }

    // 5. Parse manifest
    let manifest_data = files
        .get("META-INF/manifest.json")
        .ok_or_else(|| "Package missing META-INF/manifest.json".to_string())?;
    let manifest: StdlibManifest =
        serde_json::from_slice(manifest_data).map_err(|e| format!("Invalid manifest: {}", e))?;

    // 6. Verify checksum
    let verified = verify_checksum(&files)?;

    // 7. Count files by type
    let auc_file_count = files.values().filter(|_| false).count(); // placeholder
    let real_auc_count = files.keys().filter(|p| p.ends_with(".auc")).count();
    let native_count = files.keys().filter(|p| p.starts_with("native/")).count();

    Ok(StdlibPackageContent {
        manifest,
        files,
        verified,
        auc_file_count: real_auc_count,
        native_file_count: native_count,
    })
}

/// Verify checksum against META-INF/checksum.sha256.
fn verify_checksum(files: &BTreeMap<String, Vec<u8>>) -> Result<bool, String> {
    let checksum_data = match files.get("META-INF/checksum.sha256") {
        Some(d) => d,
        None => return Ok(false),
    };

    let checksum_content = String::from_utf8_lossy(checksum_data);
    let mut all_ok = true;

    for line in checksum_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let expected_hash = parts[0];
        let file_path = parts[1..].join(" ");

        if let Some(content) = files.get(&file_path) {
            let actual_hash = sha256_hex(content);
            if actual_hash != expected_hash {
                eprintln!(
                    "Checksum mismatch: {} (expected {}, got {}",
                    file_path, expected_hash, actual_hash
                );
                all_ok = false;
            }
        }
    }

    Ok(all_ok)
}

/// Compute SHA-256 hash.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_verify_checksum_valid() {
        let mut files = BTreeMap::new();
        let content = b"hello world";
        files.insert("test.txt".to_string(), content.to_vec());
        let hash = sha256_hex(content);
        let checksum = format!("{}  test.txt", hash);
        files.insert(
            "META-INF/checksum.sha256".to_string(),
            checksum.into_bytes(),
        );

        let result = verify_checksum(&files).unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let mut files = BTreeMap::new();
        files.insert("test.txt".to_string(), b"hello".to_vec());
        files.insert(
            "META-INF/checksum.sha256".to_string(),
            b"0000000000000000000000000000000000000000000000000000000000000000  test.txt".to_vec(),
        );

        let result = verify_checksum(&files).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_checksum_no_file() {
        let files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let result = verify_checksum(&files).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_read_auz_package_not_found() {
        let result = read_auz_package(Path::new("/nonexistent/std.auz"));
        assert!(result.is_err());
    }

    #[test]
    fn test_read_auz_package_invalid_magic() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("invalid.auz");
        std::fs::write(&path, b"not a valid auz file").unwrap();

        let result = read_auz_package(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("zstd magic"));
    }

    #[test]
    fn test_stdlib_package_content_methods() {
        let mut files = BTreeMap::new();
        files.insert("lib/Math.auc".to_string(), b"a".to_vec());
        files.insert("lib/String.auc".to_string(), b"b".to_vec());
        files.insert("native/libstd.a".to_string(), b"c".to_vec());
        files.insert("src/math.aura".to_string(), b"d".to_vec());
        files.insert("lib/ffi_cache.vm.json".to_string(), b"e".to_vec());

        let content = StdlibPackageContent {
            manifest: super::super::builder::StdlibManifest {
                name: "test".to_string(),
                version: "1.0".to_string(),
                execution_modes: vec![],
                ffi_mode: "aot".to_string(),
                modules: vec![],
                artifacts: BTreeMap::new(),
                ffi: super::super::builder::FfiConfig {
                    cffi: None,
                    rustffi: None,
                    aot: None,
                },
                checksum: serde_json::json!({}),
            },
            files,
            verified: true,
            auc_file_count: 2,
            native_file_count: 1,
        };

        assert_eq!(content.auc_files().len(), 2);
        assert_eq!(content.native_files().len(), 1);
        assert_eq!(content.source_files().len(), 1);
        assert_eq!(content.ffi_files().len(), 1);
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash.len(), 64);
    }
}
