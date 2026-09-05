//! Phase 1 §5 + §13.1: `.apkg` 打包器
//!
//! 职责：将 [`PackageManifest`] + [`BytecodeModule`]（+ 可选源码）打包为
//! `.apkg`（tar + zstd 容器），包含校验和文件。
//!
//! 输出布局（设计方案 §5.1）：
//!   foo-1.2.3.apkg/
//!     META-INF/aura.toml
//!     META-INF/checksum.sha256
//!     lib/name-version/entry.auc
//!     ref/index.json
//!     src/                        可选

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::codegen::{self, BytecodeModule};
use crate::package::PackageManifest;

use super::checksum::{self, ChecksumEntry};
use super::{
    ApkgError, DEFAULT_COMPRESSION_LEVEL, LIB_DIR, MANIFEST_FILENAME, REF_INDEX_FILENAME,
    SRC_DIR, CHECKSUM_FILENAME,
};

// ─────────────────────────────────────────────────────────────────────────────
// 打包选项
// ─────────────────────────────────────────────────────────────────────────────

/// 打包选项（设计方案 §9.1 `[package]` 的运行时开关）
#[derive(Debug, Clone, Default)]
pub struct PackageBuildOptions {
    /// 是否包含源码附件到 `src/`
    pub include_sources: bool,
    /// 是否包含资源文件到 `resources/`
    pub include_resources: bool,
    /// 资源包含模式（`**/*.json` 等）
    pub resource_include_patterns: Vec<String>,
    /// 是否写入 `ref/index.json`（Phase 1 占位）
    pub include_ref_index: bool,
    /// zstd 压缩级别（默认 3）
    pub compression_level: i32,
}

impl PackageBuildOptions {
    /// 默认选项（仅含必需的字节码 + 清单 + 校验和）
    pub fn minimal() -> Self {
        PackageBuildOptions {
            include_sources: false,
            include_resources: false,
            resource_include_patterns: vec![],
            include_ref_index: true,
            compression_level: DEFAULT_COMPRESSION_LEVEL,
        }
    }

    /// 完整选项（含源码 + ref 索引）
    pub fn full() -> Self {
        PackageBuildOptions {
            include_sources: true,
            include_resources: false,
            resource_include_patterns: vec![],
            include_ref_index: true,
            compression_level: DEFAULT_COMPRESSION_LEVEL,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 打包结果
// ─────────────────────────────────────────────────────────────────────────────

/// 打包结果
#[derive(Debug)]
pub struct BuildResult {
    /// 输出 `.apkg` 文件路径
    pub path: PathBuf,
    /// 最终 `.apkg` 文件大小（字节）
    pub size_bytes: u64,
    /// 包内文件数量（含清单 + 校验和）
    pub file_count: usize,
    /// 校验和条目
    pub checksum_entries: Vec<ChecksumEntry>,
}

impl BuildResult {
    /// 输出摘要
    pub fn summary(&self) -> String {
        format!(
            "已打包 {} 个文件到 {}（{} 字节）",
            self.file_count,
            self.path.display(),
            self.size_bytes
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 包构建器
// ─────────────────────────────────────────────────────────────────────────────

/// `.apkg` 包构建器
pub struct PackageBuilder<'a> {
    manifest: &'a PackageManifest,
    module: &'a BytecodeModule,
    source_dir: Option<&'a Path>,
    resource_dir: Option<&'a Path>,
    options: PackageBuildOptions,
}

impl<'a> PackageBuilder<'a> {
    /// 创建新的构建器
    pub fn new(manifest: &'a PackageManifest, module: &'a BytecodeModule) -> Self {
        Self {
            manifest,
            module,
            source_dir: None,
            resource_dir: None,
            options: PackageBuildOptions::minimal(),
        }
    }

    /// 指定源码目录（将 `.aura` 文件复制到 `src/`）
    pub fn with_source_dir(mut self, dir: &'a Path) -> Self {
        self.source_dir = Some(dir);
        self
    }

    /// 指定资源目录
    pub fn with_resource_dir(mut self, dir: &'a Path) -> Self {
        self.resource_dir = Some(dir);
        self
    }

    /// 设置打包选项
    pub fn with_options(mut self, options: PackageBuildOptions) -> Self {
        self.options = options;
        self
    }

    /// 执行打包
    pub fn build(&self, output_path: &Path) -> Result<BuildResult, ApkgError> {
        // 1. 收集所有待打包的文件（路径 -> 字节内容）
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        // 1.1 META-INF/aura.toml（主清单）
        let manifest_toml = self.manifest.to_toml().map_err(|e| {
            ApkgError::Manifest(format!("序列化清单失败: {}", e))
        })?;
        files.insert(MANIFEST_FILENAME.to_string(), manifest_toml.as_bytes().to_vec());

        // 1.2 lib/name-version/entry.auc（字节码）
        let entry_base = self.manifest.entry.trim_end_matches(".aura");
        let lib_subdir = format!("{}-{}", self.manifest.name, self.manifest.version);
        let auc_path = format!("{}/{}/{}.auc", LIB_DIR, lib_subdir, entry_base);
        let auc_bytes = codegen::to_bytes(self.module);
        files.insert(auc_path.clone(), auc_bytes);

        // 1.3 ref/index.json（Phase 1 占位，Phase 3 实现 .sig）
        if self.options.include_ref_index {
            let ref_index = Self::build_ref_index(self.manifest);
            files.insert(REF_INDEX_FILENAME.to_string(), ref_index.as_bytes().to_vec());
        }

        // 1.4 src/ 源码附件（可选）
        if self.options.include_sources {
            if let Some(dir) = self.source_dir {
                let mut src_files = Self::collect_source_files(dir)?;
                for (rel_path, content) in src_files.drain(..) {
                    let tar_path = format!("{}/{}", SRC_DIR, rel_path);
                    files.insert(tar_path, content);
                }
            }
        }

        // 1.5 resources/（可选）
        if self.options.include_resources {
            if let Some(dir) = self.resource_dir {
                let patterns = &self.options.resource_include_patterns;
                let mut res_files = Self::collect_matched_files(dir, patterns)?;
                for (rel_path, content) in res_files.drain(..) {
                    let tar_path = format!("{}/{}", super::RESOURCES_DIR, rel_path);
                    files.insert(tar_path, content);
                }
            }
        }

        // 2. 计算所有文件的 SHA-256 校验和
        let mut checksum_entries: Vec<ChecksumEntry> = Vec::new();
        for (path, content) in &files {
            checksum_entries.push(ChecksumEntry {
                hash: checksum::compute_sha256(content),
                path: path.clone(),
            });
        }
        checksum_entries.sort_by(|a, b| a.path.cmp(&b.path));

        // 3. 生成 checksum.sha256 文件内容并加入文件集
        let checksum_content = checksum::generate_checksum_file(&checksum_entries);
        let checksum_bytes = checksum_content.as_bytes().to_vec();
        files.insert(CHECKSUM_FILENAME.to_string(), checksum_bytes);

        let file_count = files.len();

        // 4. 写入 tar 归档到内存缓冲区
        let mut tar_buf: Vec<u8> = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_buf);

            for (path, content) in &files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);

                builder.append_data(
                    &mut header,
                    path.as_str(),
                    std::io::Cursor::new(content.as_slice()),
                )
                .map_err(|e| ApkgError::Build(format!("tar 写入 {} 失败: {}", path, e)))?;
            }

            builder
                .finish()
                .map_err(|e| ApkgError::Build(format!("tar 收尾失败: {}", e)))?;
        }

        // 5. zstd 压缩
        let mut compressed_buf: Vec<u8> = Vec::new();
        let mut encoder = zstd::Encoder::new(&mut compressed_buf, self.options.compression_level)
            .map_err(|e| ApkgError::Compression(format!("创建 zstd 编码器失败: {}", e)))?;
        encoder
            .write_all(&tar_buf)
            .map_err(|e| ApkgError::Compression(format!("zstd 写入失败: {}", e)))?;
        let _encoder = encoder
            .finish()
            .map_err(|e| ApkgError::Compression(format!("zstd 收尾失败: {}", e)))?;
        let output_bytes = compressed_buf;

        // 6. 确保输出目录存在
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ApkgError::Io(format!("无法创建目录 {}: {}", parent.display(), e))
                })?;
            }
        }

        // 7. 写入输出文件
        std::fs::write(output_path, &output_bytes).map_err(|e| {
            ApkgError::Io(format!("无法写入 {}: {}", output_path.display(), e))
        })?;

        Ok(BuildResult {
            path: output_path.to_path_buf(),
            size_bytes: output_bytes.len() as u64,
            file_count,
            checksum_entries,
        })
    }

    /// 构建 ref/index.json 占位内容（Phase 3 将实现 .sig 文件）
    fn build_ref_index(manifest: &PackageManifest) -> String {
        serde_json::json!({
            "version": 1,
            "modules": [{
                "name": manifest.name,
                "version": manifest.version,
                "exports": manifest.exports.clone(),
                "kind": manifest.kind.as_str(),
            }]
        })
        .to_string()
    }

    /// 收集源码目录下的 `.aura` 文件
    fn collect_source_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, ApkgError> {
        let mut result = Vec::new();
        if !dir.exists() {
            return Ok(result);
        }
        let base = dir.to_path_buf();
        Self::walk_dir(
            &base,
            &base,
            &mut result,
            &|path| path.extension().and_then(|e| e.to_str()).map(|e| e == "aura").unwrap_or(false),
        )?;
        Ok(result)
    }

    /// 收集匹配模式的资源文件
    fn collect_matched_files(
        dir: &Path,
        patterns: &[String],
    ) -> Result<Vec<(String, Vec<u8>)>, ApkgError> {
        let mut result = Vec::new();
        if !dir.exists() || patterns.is_empty() {
            return Ok(result);
        }
        let base = dir.to_path_buf();
        let compiled: Vec<regex::Regex> = patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect();

        Self::walk_dir(&base, &base, &mut result, &|path| {
            let rel = path.strip_prefix(&base).unwrap_or(path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            compiled.iter().any(|re| re.is_match(&rel_str))
        })?;
        Ok(result)
    }

    /// 递归遍历目录，收集匹配的文件
    fn walk_dir(
        base: &Path,
        current: &Path,
        result: &mut Vec<(String, Vec<u8>)>,
        filter: &dyn Fn(&Path) -> bool,
    ) -> Result<(), ApkgError> {
        let entries = std::fs::read_dir(current).map_err(|e| {
            ApkgError::Io(format!("无法读取目录 {}: {}", current.display(), e))
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::walk_dir(base, &path, result, filter)?;
            } else if filter(&path) {
                let content = std::fs::read(&path).map_err(|e| {
                    ApkgError::Io(format!("无法读取 {}: {}", path.display(), e))
                })?;
                let rel = path.strip_prefix(base).unwrap_or(&path);
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                result.push((rel_str, content));
            }
        }
        Ok(())
    }
}
