//! Phase 1 §5.2 + §11.4: `.apkg` 读取器
//!
//! 职责：
//! 1. 检测 zstd 魔数（28 B5 2F FD）
//! 2. zstd 解压
//! 3. tar 读取
//! 4. 提取清单 + 字节码 + 源码
//! 5. 校验和验证

use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::codegen::{self, BytecodeModule};
use crate::package::PackageManifest;

use super::checksum::{self, ChecksumEntry};
use super::{
    ApkgError, CHECKSUM_FILENAME, LIB_DIR, MANIFEST_FILENAME, REF_INDEX_FILENAME,
    SIGNATURE_FILENAME, SRC_DIR, ZSTD_MAGIC,
};

// ─────────────────────────────────────────────────────────────────────────────
// 文件信息
// ─────────────────────────────────────────────────────────────────────────────

/// `.apkg` 内单个文件的信息
#[derive(Debug, Clone)]
pub struct PackageFileInfo {
    /// 文件路径（tar 内路径，POSIX 风格）
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// 包内容
// ─────────────────────────────────────────────────────────────────────────────

/// 从 `.apkg` 读取的内容
#[derive(Debug)]
pub struct PackageContent {
    /// 包清单
    pub manifest: PackageManifest,
    /// 字节码模块（从 `lib/` 加载）
    pub module: Option<BytecodeModule>,
    /// 所有文件（路径 -> 字节内容）
    pub files: BTreeMap<String, Vec<u8>>,
    /// 校验和条目（从 `META-INF/checksum.sha256` 读取）
    pub checksum_entries: Vec<ChecksumEntry>,
    /// 是否已验证校验和
    pub verified: bool,
}

impl PackageContent {
    /// 获取字节码模块（若不存在则返回错误）
    pub fn module(&self) -> Result<&BytecodeModule, ApkgError> {
        self.module.as_ref().ok_or_else(|| {
            ApkgError::Format("包内未找到 lib/*.auc 字节码文件".to_string())
        })
    }

    /// 获取指定路径的文件内容
    pub fn file_content(&self, path: &str) -> Option<&Vec<u8>> {
        self.files.get(path)
    }

    /// 获取源码文件（`src/` 目录下的文件）
    pub fn source_files(&self) -> Vec<(&String, &Vec<u8>)> {
        self.files
            .iter()
            .filter(|(path, _)| super::path_under(SRC_DIR, path.as_str()))
            .collect()
    }

    /// 获取 ref/index.json 内容（若存在）
    pub fn ref_index(&self) -> Option<&Vec<u8>> {
        self.files.get(REF_INDEX_FILENAME)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 包读取器
// ─────────────────────────────────────────────────────────────────────────────

/// `.apkg` 包读取器
#[derive(Debug, Default)]
pub struct PackageReader;

impl PackageReader {
    /// 从文件路径读取 `.apkg`
    pub fn from_file(path: &Path) -> Result<PackageContent, ApkgError> {
        let bytes = std::fs::read(path).map_err(|e| {
            ApkgError::Io(format!("无法读取 {}: {}", path.display(), e))
        })?;
        Self::from_bytes(&bytes)
    }

    /// 从字节读取 `.apkg`
    pub fn from_bytes(bytes: &[u8]) -> Result<PackageContent, ApkgError> {
        // 1. zstd 魔数检测
        Self::check_zstd_magic(bytes)?;

        // 2. zstd 解压
        let tar_bytes = Self::decompress_zstd(bytes)?;

        // 3. tar 读取
        let files = Self::read_tar(&tar_bytes)?;

        // 4. 解析清单
        let manifest_content = files
            .get(MANIFEST_FILENAME)
            .ok_or_else(|| ApkgError::Manifest(format!("包内缺少 {}", MANIFEST_FILENAME)))?;
        let manifest_toml = std::str::from_utf8(manifest_content)
            .map_err(|e| ApkgError::Manifest(format!("清单编码错误: {}", e)))?;
        let manifest = PackageManifest::from_toml(manifest_toml).map_err(|e| {
            ApkgError::Manifest(format!("解析清单失败: {}", e))
        })?;

        // 5. 加载字节码模块
        let module = Self::load_bytecode_module(&files)?;

        // 6. 加载校验和
        let checksum_entries = Self::load_checksum_entries(&files)?;

        Ok(PackageContent {
            manifest,
            module,
            files,
            checksum_entries,
            verified: false,
        })
    }

    /// 验证 `.apkg` 文件完整性（zstd 魔数 + 解压 + 校验和）
    pub fn verify(path: &Path) -> Result<VerifyResult, ApkgError> {
        let content = Self::from_file(path)?;
        content.verify_checksums()
    }

    // ── 内部工具 ──

    /// 检测 zstd 魔数（设计方案 §5.2）
    fn check_zstd_magic(bytes: &[u8]) -> Result<(), ApkgError> {
        if bytes.len() < 4 {
            return Err(ApkgError::Format(format!(
                "文件过小（{} 字节），不是有效的 .apkg",
                bytes.len()
            )));
        }
        if &bytes[..4] != &ZSTD_MAGIC {
            return Err(ApkgError::Format(format!(
                "zstd 魔数不匹配: 期望 {:02X?}，实际 {:02X?}（不是 .apkg 文件）",
                ZSTD_MAGIC,
                &bytes[..4]
            )));
        }
        Ok(())
    }

    /// zstd 解压
    fn decompress_zstd(bytes: &[u8]) -> Result<Vec<u8>, ApkgError> {
        let mut decoder = zstd::Decoder::new(Cursor::new(bytes))
            .map_err(|e| ApkgError::Compression(format!("创建 zstd 解码器失败: {}", e)))?;
        let mut output = Vec::new();
        decoder
            .read_to_end(&mut output)
            .map_err(|e| ApkgError::Compression(format!("zstd 解压失败: {}", e)))?;
        Ok(output)
    }

    /// 从 tar 字节读取所有文件
    fn read_tar(tar_bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, ApkgError> {
        let mut archive = tar::Archive::new(Cursor::new(tar_bytes));
        let mut files = BTreeMap::new();

        for entry in archive
            .entries()
            .map_err(|e| ApkgError::Format(format!("tar 读取失败: {}", e)))?
        {
            let mut entry = entry.map_err(|e| ApkgError::Format(format!("tar 条目错误: {}", e)))?;

            // 只处理普通文件
            if !entry.header().entry_type().is_file() {
                continue;
            }

            let path = entry
                .path()
                .map_err(|e| ApkgError::Format(format!("tar 路径错误: {}", e)))?
                .to_string_lossy()
                .replace('\\', "/")
                .to_string();

            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| ApkgError::Io(format!("读取 {} 失败: {}", path, e)))?;

            files.insert(path, content);
        }

        Ok(files)
    }

    /// 从 `lib/` 目录加载字节码模块
    fn load_bytecode_module(
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Option<BytecodeModule>, ApkgError> {
        // 查找 lib/ 下的 .auc 文件
        let auc_files: Vec<&String> = files
            .keys()
            .filter(|p| super::path_under(LIB_DIR, p.as_str()) && p.ends_with(".auc"))
            .collect();

        if auc_files.is_empty() {
            // 库包可能不含 lib/（如 native-only，Phase 4）
            return Ok(None);
        }

        // 加载第一个 .auc 文件（Phase 1 单模块；Phase 2 实现多模块）
        let first_auc = auc_files[0];
        let module = codegen::from_bytes(&files[first_auc])
            .map_err(|e| ApkgError::Format(format!("反序列化 {} 失败: {}", first_auc, e)))?;
        Ok(Some(module))
    }

    /// 从 `META-INF/checksum.sha256` 加载校验和条目
    fn load_checksum_entries(
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Vec<ChecksumEntry>, ApkgError> {
        let checksum_content = match files.get(CHECKSUM_FILENAME) {
            Some(c) => c,
            None => return Ok(Vec::new()), // 校验和可选（旧包）
        };
        let text = std::str::from_utf8(checksum_content)
            .map_err(|e| ApkgError::Format(format!("校验和文件编码错误: {}", e)))?;
        Ok(checksum::parse_checksum_file(text))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 校验和验证
// ─────────────────────────────────────────────────────────────────────────────

/// 校验和验证结果
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// 验证通过的文件数
    pub verified_count: usize,
    /// 校验失败的文件列表（路径 -> 错误信息）
    pub failures: Vec<(String, String)>,
}

impl VerifyResult {
    /// 是否全部通过
    pub fn is_valid(&self) -> bool {
        self.failures.is_empty()
    }

    /// 生成验证报告
    pub fn report(&self) -> String {
        if self.is_valid() {
            format!("✓ 校验和验证通过（{} 个文件）", self.verified_count)
        } else {
            let mut lines = vec![format!(
                "✗ 校验和验证失败（{}/{} 通过）",
                self.verified_count,
                self.verified_count + self.failures.len()
            )];
            for (path, err) in &self.failures {
                lines.push(format!("  - {}: {}", path, err));
            }
            lines.join("\n")
        }
    }
}

impl PackageContent {
    /// 验证包内所有文件的 SHA-256 校验和
    pub fn verify_checksums(&self) -> Result<VerifyResult, ApkgError> {
        if self.checksum_entries.is_empty() {
            return Ok(VerifyResult {
                verified_count: 0,
                failures: vec![],
            });
        }

        let mut failures = Vec::new();
        let mut verified_count = 0;

        for entry in &self.checksum_entries {
            // 校验和文件本身不参与校验（循环引用）
            if entry.path == CHECKSUM_FILENAME {
                continue;
            }
            // 签名文件可选，不参与校验
            if entry.path == SIGNATURE_FILENAME {
                continue;
            }

            match self.files.get(&entry.path) {
                Some(content) => {
                    match checksum::verify_bytes(&entry.hash, content) {
                        Ok(()) => verified_count += 1,
                        Err(e) => failures.push((entry.path.clone(), e.to_string())),
                    }
                }
                None => {
                    failures.push((entry.path.clone(), "文件不存在".to_string()));
                }
            }
        }

        Ok(VerifyResult {
            verified_count,
            failures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    #[test]
    fn test_check_zstd_magic_ok() {
        let bytes = [0x28u8, 0xB5, 0x2F, 0xFD, 0x00, 0x00];
        assert!(PackageReader::check_zstd_magic(&bytes).is_ok());
    }

    #[test]
    fn test_check_zstd_magic_fail() {
        let bytes = [0xDEu8, 0xAD, 0xBE, 0xEF];
        assert!(PackageReader::check_zstd_magic(&bytes).is_err());
    }

    #[test]
    fn test_check_zstd_magic_short() {
        let bytes = [0x28u8, 0xB5];
        assert!(PackageReader::check_zstd_magic(&bytes).is_err());
    }

    #[test]
    fn test_roundtrip_build_and_read() {
        use crate::apkg::PackageBuilder;
        use crate::apkg::PackageBuildOptions;
        use crate::codegen::compile_source;
        use crate::package::{PackageKind, PackageManifest, PackageOptions, ResourceConfig};

        // 编译一个简单的 Aura 源码
        let source = r#"
public fun main() {
    println("Hello, Aura!")
}
"#;
        let module = compile_source(source).unwrap();

        // 创建清单
        let manifest = PackageManifest {
            schema_version: "1.0".to_string(),
            name: "test-pkg".to_string(),
            version: "0.1.0".to_string(),
            description: Some("测试包".to_string()),
            authors: vec![],
            license: Some("MIT".to_string()),
            repository: None,
            entry: "main.aura".to_string(),
            dependencies: vec![],
            dev_dependencies: vec![],
            exports: vec!["main".to_string()],
            platforms: vec![],
            library: true,
            kind: PackageKind::Bytecode,
            compiler_min_version: Some("0.3.0".to_string()),
            compiler_max_version: None,
            package: PackageOptions::default(),
            resources: ResourceConfig::default(),
        };

        // 打包
        let options = PackageBuildOptions {
            include_sources: false,
            include_ref_index: true,
            ..Default::default()
        };
        let builder = PackageBuilder::new(&manifest, &module).with_options(options);

        let out_path = std::env::temp_dir().join(format!("aura_test_pkg_{}.apkg", std::process::id()));
        let result = builder.build(&out_path).unwrap();
        assert!(result.size_bytes > 0);
        assert!(result.file_count >= 3); // manifest + checksum + auc

        // 读取
        let content = PackageReader::from_file(&out_path).unwrap();
        assert_eq!(content.manifest.name, "test-pkg");
        assert_eq!(content.manifest.version, "0.1.0");
        assert_eq!(content.manifest.kind, PackageKind::Bytecode);
        assert!(content.manifest.library);
        assert!(content.module.is_some());
        assert!(content.checksum_entries.len() >= 3);

        // 验证校验和
        let verify_result = PackageReader::verify(&out_path).unwrap();
        assert!(verify_result.is_valid());
        assert!(verify_result.verified_count > 0);

        // 清理
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn test_inspect_apkg_file_listing() {
        use crate::apkg::PackageBuilder;
        use crate::apkg::PackageBuildOptions;
        use crate::codegen::compile_source;
        use crate::package::{PackageKind, PackageManifest, PackageOptions, ResourceConfig};

        let source = "public fun main() { }";
        let module = compile_source(source).unwrap();

        let manifest = PackageManifest {
            schema_version: "1.0".to_string(),
            name: "inspect-test".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            authors: vec![],
            license: None,
            repository: None,
            entry: "main.aura".to_string(),
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

        let options = PackageBuildOptions {
            include_ref_index: true,
            ..Default::default()
        };
        let builder = PackageBuilder::new(&manifest, &module).with_options(options);
        let out_path = std::env::temp_dir().join(format!("aura_inspect_test_{}.apkg", std::process::id()));
        builder.build(&out_path).unwrap();

        let content = PackageReader::from_file(&out_path).unwrap();

        // 检查文件列表
        let paths: Vec<&str> = content.files.keys().map(|s| s.as_str()).collect();
        assert!(paths.iter().any(|p| *p == "META-INF/aura.toml"));
        assert!(paths.iter().any(|p| *p == "META-INF/checksum.sha256"));
        assert!(paths.iter().any(|p| p.starts_with("lib/")));
        assert!(paths.iter().any(|p| *p == "ref/index.json"));

        let _ = std::fs::remove_file(&out_path);
    }
}
