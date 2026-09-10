//! Phase 6: Standard library package builder.
//!
//! Packages the Aura standard library into a .auz artifact containing:
//! - .auc bytecode files (VM/JIT mode)
//! - Native AOT libraries (.so/.dll/.a) (AOT mode)
//! - C FFI static libraries
//! - FFI function address/symbol mappings
//! - Manifest with three execution modes
//! - SHA-256 checksum
//!
//! Corresponds to Phase 6 in the full Aura-ification plan.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::stdlib::{FfiIndex, StdlibIndex};

// ─────────────────────────────────────────────────────────────────────────────
// Artifact metadata
// ─────────────────────────────────────────────────────────────────────────────

/// Execution mode identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExecutionModeId {
    Vm,
    Jit,
    Aot,
}

impl std::fmt::Display for ExecutionModeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionModeId::Vm => write!(f, "vm"),
            ExecutionModeId::Jit => write!(f, "jit"),
            ExecutionModeId::Aot => write!(f, "aot"),
        }
    }
}

/// Artifact format type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactFormat {
    Auc,
    Native,
    CStaticLib,
}

impl std::fmt::Display for ArtifactFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArtifactFormat::Auc => write!(f, "auc"),
            ArtifactFormat::Native => write!(f, "native"),
            ArtifactFormat::CStaticLib => write!(f, "c-static"),
        }
    }
}

/// An artifact entry in the manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactEntry {
    /// Format type
    pub format: String,
    /// File names (VM/JIT) or platform -> file mapping (AOT)
    pub files: serde_json::Value,
    /// FFI cache/symbol file (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffi_cache: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffi_symbols: Option<String>,
}

/// FFI configuration in the manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FfiConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cffi: Option<CffiConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rustffi: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aot: Option<AotFfiConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CffiConfig {
    pub lib: String,
    pub files: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AotFfiConfig {
    pub inline: bool,
    pub optimize: u8,
    pub static_link: bool,
}

/// Standard library package manifest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StdlibManifest {
    pub name: String,
    pub version: String,
    pub execution_modes: Vec<String>,
    pub ffi_mode: String,
    pub modules: Vec<String>,
    pub artifacts: BTreeMap<String, ArtifactEntry>,
    pub ffi: FfiConfig,
    pub checksum: serde_json::Value,
}

// ─────────────────────────────────────────────────────────────────────────────
// Build options and result
// ─────────────────────────────────────────────────────────────────────────────

/// Build options for stdlib packaging.
#[derive(Debug, Clone, Default)]
pub struct StdlibBuildOptions {
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// Execution modes to include
    pub execution_modes: Vec<ExecutionModeId>,
    /// FFI mode
    pub ffi_mode: String,
    /// Compression level (zstd)
    pub compression_level: i32,
    /// Include source files
    pub include_sources: bool,
}

impl StdlibBuildOptions {
    /// Default options (all three modes, AOT FFI).
    pub fn default_all_modes() -> Self {
        Self {
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            execution_modes: vec![
                ExecutionModeId::Vm,
                ExecutionModeId::Jit,
                ExecutionModeId::Aot,
            ],
            ffi_mode: "aot".to_string(),
            compression_level: 3,
            include_sources: false,
        }
    }
}

/// Result of a stdlib package build.
#[derive(Debug)]
pub struct StdlibBuildResult {
    /// Output .auz file path
    pub path: PathBuf,
    /// Output size in bytes
    pub size_bytes: u64,
    /// Number of files in the package
    pub file_count: usize,
    /// The generated manifest
    pub manifest: StdlibManifest,
}

impl StdlibBuildResult {
    pub fn summary(&self) -> String {
        format!(
            "Packed {} files into {} ({} bytes, version {})",
            self.file_count,
            self.path.display(),
            self.size_bytes,
            self.manifest.version
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Builder
// ─────────────────────────────────────────────────────────────────────────────

/// Standard library package builder.
pub struct StdlibPackageBuilder<'a> {
    stdlib_index: &'a StdlibIndex,
    ffi_index: Option<&'a FfiIndex>,
    auc_dir: Option<&'a Path>,
    native_dir: Option<&'a Path>,
    cffi_dir: Option<&'a Path>,
    source_dir: Option<&'a Path>,
    options: StdlibBuildOptions,
}

impl<'a> StdlibPackageBuilder<'a> {
    /// Create a new builder.
    pub fn new(stdlib_index: &'a StdlibIndex) -> Self {
        Self {
            stdlib_index,
            ffi_index: None,
            auc_dir: None,
            native_dir: None,
            cffi_dir: None,
            source_dir: None,
            options: StdlibBuildOptions::default_all_modes(),
        }
    }

    /// Set the FFI index.
    pub fn with_ffi_index(mut self, index: &'a FfiIndex) -> Self {
        self.ffi_index = Some(index);
        self
    }

    /// Set the directory containing .auc files.
    pub fn with_auc_dir(mut self, dir: &'a Path) -> Self {
        self.auc_dir = Some(dir);
        self
    }

    /// Set the directory containing AOT native libraries.
    pub fn with_native_dir(mut self, dir: &'a Path) -> Self {
        self.native_dir = Some(dir);
        self
    }

    /// Set the directory containing C FFI libraries.
    pub fn with_cffi_dir(mut self, dir: &'a Path) -> Self {
        self.cffi_dir = Some(dir);
        self
    }

    /// Set the source directory for inclusion.
    pub fn with_source_dir(mut self, dir: &'a Path) -> Self {
        self.source_dir = Some(dir);
        self
    }

    /// Set build options.
    pub fn with_options(mut self, options: StdlibBuildOptions) -> Self {
        self.options = options;
        self
    }

    /// Build the .auz package.
    pub fn build(&self, output_path: &Path) -> Result<StdlibBuildResult, String> {
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

        // 1. Build artifacts for each execution mode
        let mut artifacts = BTreeMap::new();

        for mode in &self.options.execution_modes {
            let mode_str = mode.to_string();
            let mut artifact = ArtifactEntry {
                format: match mode {
                    ExecutionModeId::Aot => "native".to_string(),
                    _ => "auc".to_string(),
                },
                files: serde_json::json!([]),
                ffi_cache: None,
                ffi_symbols: None,
            };

            if matches!(mode, ExecutionModeId::Aot) {
                // AOT: package native libraries per platform
                let native_files = self.collect_native_files();
                artifact.files = serde_json::json!(native_files);
                if let Some(ref ffi_idx) = self.ffi_index {
                    let symbols =
                        serde_json::to_string(ffi_idx).unwrap_or_else(|_| "{}".to_string());
                    let sym_path = format!("native/ffi_symbols.{}.json", mode_str);
                    files.insert(sym_path.clone(), symbols.into_bytes());
                    artifact.ffi_symbols = Some(sym_path);
                }
            } else {
                // VM/JIT: package .auc files
                let auc_files = self.collect_auc_files();
                artifact.files = serde_json::json!(auc_files);
                for auc_file in &auc_files {
                    if let Some(ref auc_dir) = self.auc_dir {
                        let auc_path = auc_dir.join(format!("{}.auc", auc_file));
                        if auc_path.exists() {
                            let content = std::fs::read(&auc_path).map_err(|e| {
                                format!("Failed to read {}: {}", auc_path.display(), e)
                            })?;
                            let tar_path = format!("lib/{}/{}.auc", mode_str, auc_file);
                            files.insert(tar_path, content);
                        }
                    }
                }
                // FFI cache for VM mode
                if matches!(mode, ExecutionModeId::Vm) {
                    if let Some(ref ffi_idx) = self.ffi_index {
                        let cache =
                            serde_json::to_string(ffi_idx).unwrap_or_else(|_| "{}".to_string());
                        let cache_path = format!("lib/ffi_cache.{}.json", mode_str);
                        files.insert(cache_path.clone(), cache.into_bytes());
                        artifact.ffi_cache = Some(cache_path);
                    }
                }
            }

            artifacts.insert(mode_str, artifact);
        }

        // 2. Package C FFI libraries
        let mut cffi_config = None;
        if let Some(ref cffi_dir) = self.cffi_dir {
            if cffi_dir.exists() {
                let mut cffi_files = serde_json::Map::new();
                let entries = match std::fs::read_dir(cffi_dir) {
                    Ok(d) => d.collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                };
                for entry in entries.iter().flatten() {
                    let path = entry.path();
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let key = match ext {
                        "a" | "lib" | "so" | "dll" | "dylib" => {
                            let platform = detect_platform();
                            format!("{}-{}", platform, fname)
                        }
                        _ => continue,
                    };
                    cffi_files.insert(key, serde_json::Value::String(fname.to_string()));
                }
                if !cffi_files.is_empty() {
                    cffi_config = Some(CffiConfig {
                        lib: "aura_std_cffi".to_string(),
                        files: serde_json::Value::Object(cffi_files),
                    });
                }
            }
        }

        // 3. Include source files if requested
        if self.options.include_sources {
            if let Some(ref src_dir) = self.source_dir {
                if src_dir.exists() {
                    self.collect_source_files(src_dir, &mut files);
                }
            }
        }

        // 4. Build manifest
        let modules: Vec<String> =
            self.stdlib_index.modules.iter().map(|m| format!("aura.lang.std.{}", m.name)).collect();

        let manifest = StdlibManifest {
            name: self.options.name.clone(),
            version: self.options.version.clone(),
            execution_modes: self.options.execution_modes.iter().map(|m| m.to_string()).collect(),
            ffi_mode: self.options.ffi_mode.clone(),
            modules,
            artifacts,
            ffi: FfiConfig {
                cffi: cffi_config,
                rustffi: None,
                aot: Some(AotFfiConfig {
                    inline: true,
                    optimize: 3,
                    static_link: false,
                }),
            },
            checksum: serde_json::json!({"sha256": "placeholder"}),
        };

        // 5. Write manifest to package
        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        files.insert(
            "META-INF/manifest.json".to_string(),
            manifest_json.into_bytes(),
        );

        // 6. Compute checksums
        let mut checksum_entries: Vec<(String, String)> = Vec::new();
        for (path, content) in &files {
            let hash = sha256_hex(content);
            checksum_entries.push((path.clone(), hash));
        }
        checksum_entries.sort();

        let checksum_content = checksum_entries
            .iter()
            .map(|(p, h)| format!("{}  {}", h, p))
            .collect::<Vec<_>>()
            .join("\n");
        files.insert(
            "META-INF/checksum.sha256".to_string(),
            checksum_content.into_bytes(),
        );

        // 7. Write .auz (tar + zstd)
        let file_count = files.len();
        let output_bytes = write_auz(&files, self.options.compression_level)?;

        // 8. Ensure output directory exists
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Cannot create directory {}: {}", parent.display(), e))?;
            }
        }

        // 9. Write output file
        std::fs::write(output_path, &output_bytes)
            .map_err(|e| format!("Cannot write {}: {}", output_path.display(), e))?;

        Ok(StdlibBuildResult {
            path: output_path.to_path_buf(),
            size_bytes: output_bytes.len() as u64,
            file_count,
            manifest,
        })
    }

    /// Collect .auc file names from the stdlib index.
    fn collect_auc_files(&self) -> Vec<String> {
        self.stdlib_index.modules.iter().map(|m| m.name.clone()).collect()
    }

    /// Collect native library files per platform.
    fn collect_native_files(&self) -> serde_json::Value {
        let mut result = serde_json::Map::new();
        if let Some(ref native_dir) = self.native_dir {
            if native_dir.exists() {
                let entries = match std::fs::read_dir(native_dir) {
                    Ok(d) => d.collect::<Vec<_>>(),
                    Err(_) => Vec::new(),
                };
                for entry in entries.iter().flatten() {
                    let path = entry.path();
                    let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if matches!(ext, "a" | "lib" | "so" | "dll" | "dylib") {
                        let platform = detect_platform();
                        result.insert(
                            format!("{}-{}", platform, fname),
                            serde_json::Value::String(fname.to_string()),
                        );
                    }
                }
            }
        }
        serde_json::Value::Object(result)
    }

    /// Collect source files recursively.
    fn collect_source_files(&self, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        if !dir.is_dir() {
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(d) => d.collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        for entry in entries.iter().flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.collect_source_files(&path, files);
            } else if path.extension().and_then(|e| e.to_str()) == Some("aura") {
                let rel = path.to_string_lossy().replace('\\', "/");
                let content = std::fs::read(&path).unwrap_or_default();
                files.insert(format!("src/{}", rel), content);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper functions
// ─────────────────────────────────────────────────────────────────────────────

/// Detect the current platform triple.
pub fn detect_platform() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(target_os = "macos") {
        "macos-x86_64"
    } else {
        "linux-x86_64"
    }
}

/// Compute SHA-256 hash of content.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex::encode(result)
}

/// Write files to a .auz container (tar + zstd).
fn write_auz(files: &BTreeMap<String, Vec<u8>>, compression_level: i32) -> Result<Vec<u8>, String> {
    // Build tar archive
    let mut tar_buf: Vec<u8> = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_buf);
        for (path, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            builder
                .append_data(
                    &mut header,
                    path.as_str(),
                    std::io::Cursor::new(content.as_slice()),
                )
                .map_err(|e| format!("tar write {} failed: {}", path, e))?;
        }
        builder.finish().map_err(|e| format!("tar finish failed: {}", e))?;
    }

    // Compress with zstd
    let mut compressed: Vec<u8> = Vec::new();
    let mut encoder = zstd::Encoder::new(&mut compressed, compression_level)
        .map_err(|e| format!("zstd encoder failed: {}", e))?;
    encoder.write_all(&tar_buf).map_err(|e| format!("zstd write failed: {}", e))?;
    encoder.finish().map_err(|e| format!("zstd finish failed: {}", e))?;

    Ok(compressed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::{
        ExecutionMode, FfiDeclaration, FfiIndex, FfiMode, StdlibIndex, StdlibModule,
    };
    use std::path::PathBuf;

    fn make_test_stdlib_index() -> StdlibIndex {
        let mut idx = StdlibIndex::new(PathBuf::from("/tmp/std"), ExecutionMode::Vm, FfiMode::Aot);
        idx.output_dir = PathBuf::from("/tmp/std");
        idx.modules.push(StdlibModule {
            name: "Math".to_string(),
            full_name: "aura.lang.std.Math".to_string(),
            source_path: PathBuf::from("/tmp/Math.aura"),
            auc_path: None,
            function_names: vec![
                "abs".to_string(),
                "min".to_string(),
                "max".to_string(),
            ],
            type_names: vec![],
            constant_names: vec![
                "PI".to_string(),
                "E".to_string(),
            ],
            has_extern: false,
        });
        idx.modules.push(StdlibModule {
            name: "String".to_string(),
            full_name: "aura.lang.std.String".to_string(),
            source_path: PathBuf::from("/tmp/String.aura"),
            auc_path: None,
            function_names: vec![
                "length".to_string(),
                "contains".to_string(),
            ],
            type_names: vec![],
            constant_names: vec![],
            has_extern: false,
        });
        idx
    }

    #[test]
    fn test_execution_mode_id_display() {
        assert_eq!(ExecutionModeId::Vm.to_string(), "vm");
        assert_eq!(ExecutionModeId::Jit.to_string(), "jit");
        assert_eq!(ExecutionModeId::Aot.to_string(), "aot");
    }

    #[test]
    fn test_artifact_format_display() {
        assert_eq!(ArtifactFormat::Auc.to_string(), "auc");
        assert_eq!(ArtifactFormat::Native.to_string(), "native");
        assert_eq!(ArtifactFormat::CStaticLib.to_string(), "c-static");
    }

    #[test]
    fn test_stdlib_build_options_default() {
        let opts = StdlibBuildOptions::default_all_modes();
        assert_eq!(opts.name, "aura-stdlib");
        assert_eq!(opts.version, "1.0.0");
        assert_eq!(opts.execution_modes.len(), 3);
        assert_eq!(opts.ffi_mode, "aot");
        assert_eq!(opts.compression_level, 3);
    }

    #[test]
    fn test_stdlib_build_options_minimal() {
        let opts = StdlibBuildOptions::default();
        assert!(opts.name.is_empty());
        assert!(opts.version.is_empty());
        assert!(opts.execution_modes.is_empty());
    }

    #[test]
    fn test_stdlib_manifest_serialize() {
        let idx = make_test_stdlib_index();
        let manifest = StdlibManifest {
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            execution_modes: vec![
                "vm".to_string(),
                "jit".to_string(),
                "aot".to_string(),
            ],
            ffi_mode: "aot".to_string(),
            modules: vec![
                "aura.lang.std.Math".to_string(),
                "aura.lang.std.String".to_string(),
            ],
            artifacts: BTreeMap::new(),
            ffi: FfiConfig {
                cffi: None,
                rustffi: None,
                aot: Some(AotFfiConfig {
                    inline: true,
                    optimize: 3,
                    static_link: false,
                }),
            },
            checksum: serde_json::json!({"sha256": "abc123"}),
        };

        let json = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(json.contains("aura-stdlib"));
        assert!(json.contains("1.0.0"));
        assert!(json.contains("Math"));
        assert!(json.contains("String"));
    }

    #[test]
    fn test_stdlib_package_builder_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = make_test_stdlib_index();

        // Create a fake .auc file
        let auc_dir = tmp.path().join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc-data").unwrap();
        std::fs::write(auc_dir.join("String.auc"), b"fake-auc-data2").unwrap();

        let builder = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "1.0.0".to_string(),
                execution_modes: vec![ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );

        let output_path = tmp.path().join("std.auz");
        let result = builder.build(&output_path);

        assert!(result.is_ok(), "Build failed: {:?}", result.err());
        let r = result.unwrap();
        assert!(r.path.exists());
        assert!(r.size_bytes > 0);
        assert!(r.file_count >= 4); // manifest + checksum + 2 auc files
        assert_eq!(r.manifest.name, "aura-stdlib");
        assert_eq!(r.manifest.version, "1.0.0");
        assert_eq!(r.manifest.modules.len(), 2);
    }

    #[test]
    fn test_stdlib_package_builder_all_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = make_test_stdlib_index();

        let auc_dir = tmp.path().join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc").unwrap();

        let builder = StdlibPackageBuilder::new(&idx)
            .with_auc_dir(&auc_dir)
            .with_options(StdlibBuildOptions::default_all_modes());

        let output_path = tmp.path().join("std.auz");
        let result = builder.build(&output_path);

        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.file_count >= 4);
        assert_eq!(r.manifest.execution_modes.len(), 3);
        assert!(r.manifest.artifacts.contains_key("vm"));
        assert!(r.manifest.artifacts.contains_key("jit"));
        assert!(r.manifest.artifacts.contains_key("aot"));
    }

    #[test]
    fn test_stdlib_package_builder_with_ffi_index() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = make_test_stdlib_index();

        let mut ffi_idx = crate::stdlib::FfiIndex::new(FfiMode::Aot);
        ffi_idx.declarations.push(FfiDeclaration {
            name: "fopen".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: None,
        });

        let builder = StdlibPackageBuilder::new(&idx).with_ffi_index(&ffi_idx).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "1.0.0".to_string(),
                execution_modes: vec![ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );

        let output_path = tmp.path().join("std.auz");
        let result = builder.build(&output_path);

        assert!(result.is_ok(), "Build failed: {:?}", result.err());
        let r = result.unwrap();
        assert!(r.file_count >= 3); // manifest + checksum + ffi_cache
    }

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_detect_platform() {
        let platform = detect_platform();
        assert!(!platform.is_empty());
        assert!(platform.contains("x86_64"));
    }
}
