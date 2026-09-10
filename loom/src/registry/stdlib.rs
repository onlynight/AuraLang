//! Phase 6: Standard library registry integration.
//!
//! Bridges the package builder/reader/installer with the registry client and
//! local registry, providing high-level operations for publishing, installing,
//! updating, and uninstalling standard library packages from a registry.

use std::path::{Path, PathBuf};

use crate::package::builder::StdlibPackageBuilder;
use crate::package::installer::{InstallConfig, install_auz, uninstall_auz};
use crate::registry::client::{RegistryClient, RegistryConfig};
use crate::registry::local::LocalRegistry;

// ═══════════════════════════════════════════════════════════════════════════════
// Data types
// ═══════════════════════════════════════════════════════════════════════════════

/// A registry entry describing a published package version.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RegistryEntry {
    /// Package name
    pub name: String,
    /// Version string (semver)
    pub version: String,
    /// ISO-8601 timestamp of publication
    pub published_at: Option<String>,
    /// Number of downloads
    pub download_count: u64,
}

/// Result of publishing a package to the registry.
#[derive(Debug, Clone)]
pub struct PublishResult {
    /// URL where the package is hosted
    pub url: String,
    /// SHA-256 checksum of the artifact
    pub checksum: String,
    /// Artifact size in bytes
    pub size: u64,
}

/// Result of installing a package from the registry.
#[derive(Debug, Clone)]
pub struct InstallResultRegistry {
    /// Whether installation succeeded
    pub success: bool,
    /// Package name
    pub name: String,
    /// Installed version
    pub version: String,
    /// Number of files installed
    pub files_installed: usize,
    /// Installation directory
    pub install_dir: PathBuf,
    /// Human-readable message
    pub message: String,
}

/// Result of updating a package to a newer version.
#[derive(Debug, Clone)]
pub struct UpdateResult {
    /// Whether update succeeded
    pub success: bool,
    /// Package name
    pub name: String,
    /// Previous version
    pub old_version: Option<String>,
    /// New version installed
    pub new_version: String,
    /// Human-readable message
    pub message: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// StdlibRegistryClient
// ═══════════════════════════════════════════════════════════════════════════════

/// Standard library registry client.
///
/// Integrates with the package builder (`.auz` creation), package reader
/// (`.auz` parsing), package installer (install/uninstall), the REST registry
/// client (remote publish/query), and the local registry (local caching).
///
/// # Example
///
/// ```ignore
/// let config = RegistryConfig::new("https://registry.aura-lang.dev");
/// let client = StdlibRegistryClient::new(config, install_dir, local_registry)?;
///
/// // Publish
/// let result = client.publish(&builder)?;
///
/// // Install
/// let result = client.install("aura-stdlib", "1.0.0")?;
///
/// // Update to latest
/// let result = client.update("aura-stdlib")?;
///
/// // Uninstall
/// client.uninstall("aura-stdlib")?;
///
/// // List
/// let entries = client.list()?;
/// ```
pub struct StdlibRegistryClient {
    registry: RegistryClient,
    local: LocalRegistry,
    install_dir: PathBuf,
}

impl StdlibRegistryClient {
    /// Create a new standard library registry client.
    ///
    /// # Arguments
    /// * `config` - Registry REST API configuration
    /// * `install_dir` - Local installation directory for stdlib packages
    /// * `local` - Local registry manager for caching
    pub fn new(
        config: RegistryConfig,
        install_dir: PathBuf,
        local: LocalRegistry,
    ) -> Result<Self, String> {
        let registry = RegistryClient::new(config)
            .map_err(|e| format!("Failed to create registry client: {}", e))?;
        Ok(Self {
            registry,
            local,
            install_dir,
        })
    }

    /// Get the installation directory.
    pub fn install_dir(&self) -> &Path {
        &self.install_dir
    }

    /// Get the local registry manager.
    pub fn local_registry(&self) -> &LocalRegistry {
        &self.local
    }

    /// Publish a standard library package to the registry.
    ///
    /// Builds a `.auz` artifact from the builder, computes its checksum, and
    /// uploads it via the registry REST API.
    ///
    /// # Arguments
    /// * `builder` - A configured `StdlibPackageBuilder` with source directories
    ///   and options already set
    ///
    /// # Returns
    /// A `PublishResult` with the hosted URL, checksum, and size.
    pub fn publish(&self, builder: &StdlibPackageBuilder<'_>) -> Result<PublishResult, String> {
        // 1. Build the .auz artifact to a temporary location
        let temp_output_path = self
            .install_dir
            .parent()
            .map_or_else(|| PathBuf::from("."), |p| p.to_path_buf())
            .join("publish_temp.auz");

        let build_result = builder
            .build(&temp_output_path)
            .map_err(|e| format!("Failed to build stdlib package: {}", e))?;

        let name = build_result.manifest.name.clone();
        let version = build_result.manifest.version.clone();
        let size = build_result.size_bytes;
        let checksum = self.compute_checksum(&temp_output_path)?;

        // 2. Read the artifact bytes and base64-encode for upload
        let artifact_bytes = std::fs::read(&temp_output_path)
            .map_err(|e| format!("Failed to read artifact for upload: {}", e))?;
        let encoded_artifact = base64::encode(&artifact_bytes);

        // 3. Create publish request
        let request = crate::registry::client::PublishRequest {
            version: version.clone(),
            artifact: encoded_artifact,
            checksum: checksum.clone(),
            metadata: crate::registry::client::PackageMetadata {
                description: Some(format!("Aura standard library v{}", version)),
                license: Some("MIT".to_string()),
                authors: None,
                repository: None,
            },
        };

        // 4. Publish via REST API
        let publish_response =
            self.registry.publish(&name, &request).map_err(|e| format!("Publish failed: {}", e))?;

        if !publish_response.success {
            return Err(format!("Publish rejected: {}", publish_response.message));
        }

        // 5. Construct hosted URL
        let url = format!(
            "{}/v1/packages/{}/versions/{}/{}.auz",
            self.registry.config().base_url.trim_end_matches('/'),
            name,
            version,
            name
        );

        Ok(PublishResult {
            url,
            checksum,
            size,
        })
    }

    /// Install a standard library package from the registry.
    ///
    /// Downloads the `.auz` artifact, caches it locally, and installs it
    /// into the installation directory.
    ///
    /// # Arguments
    /// * `name` - Package name (e.g. `"aura-stdlib"`)
    /// * `version` - Version string (e.g. `"1.0.0"`)
    ///
    /// # Returns
    /// An `InstallResultRegistry` describing the outcome.
    pub fn install(&self, name: &str, version: &str) -> Result<InstallResultRegistry, String> {
        // 1. Get version info from registry
        let version_info = self
            .registry
            .get_version(name, version)
            .map_err(|e| format!("Failed to query registry: {}", e))?;

        let download_url = version_info
            .download
            .clone()
            .ok_or_else(|| format!("No download URL for {}@{}", name, version))?;

        // 2. Download the artifact
        let artifact_path = self.download_to_local(name, version, &download_url)?;

        // 3. Verify checksum if available
        if let Some(ref expected_checksum) = version_info.checksum {
            let actual_checksum = self.compute_checksum(&artifact_path)?;
            let expected_hash =
                expected_checksum.strip_prefix("sha256:").unwrap_or(expected_checksum.as_str());
            if actual_checksum != expected_hash {
                return Err(format!(
                    "Checksum mismatch for {}@{}: expected {}, got {}",
                    name, version, expected_hash, actual_checksum
                ));
            }
        }

        // 4. Cache locally
        self.local
            .install(
                name,
                version,
                &artifact_path,
                version_info.checksum.as_deref(),
            )
            .map_err(|e| format!("Local cache install failed: {}", e))?;

        // 5. Install to the target directory
        let install_config = InstallConfig {
            install_dir: self.install_dir.clone(),
            verify_checksum: true,
        };
        let install_result = install_auz(&artifact_path, &install_config);

        if !install_result.success {
            return Err(format!(
                "Install failed: {} (install_auz: {})",
                name, install_result.message
            ));
        }

        Ok(InstallResultRegistry {
            success: true,
            name: name.to_string(),
            version: version.to_string(),
            files_installed: install_result.files_installed,
            install_dir: self.install_dir.clone(),
            message: install_result.message,
        })
    }

    /// Update a standard library package to the latest available version.
    ///
    /// Queries the registry for the latest version, and if newer than what is
    /// installed, downloads and installs it.
    ///
    /// # Arguments
    /// * `name` - Package name (e.g. `"aura-stdlib"`)
    ///
    /// # Returns
    /// An `UpdateResult` describing the outcome.
    pub fn update(&self, name: &str) -> Result<UpdateResult, String> {
        // 1. Find latest version from registry
        let package_info = self
            .registry
            .get_package(name)
            .map_err(|e| format!("Failed to query registry: {}", e))?;

        let latest_version = package_info
            .latest
            .clone()
            .ok_or_else(|| format!("No latest version found for {}", name))?;

        // 2. Check current installed version
        let install_config = InstallConfig {
            install_dir: self.install_dir.clone(),
            verify_checksum: true,
        };

        let old_version = get_installed_version(&install_config);

        if old_version.as_deref() == Some(latest_version.as_str()) {
            return Ok(UpdateResult {
                success: true,
                name: name.to_string(),
                old_version: old_version.clone(),
                new_version: latest_version.clone(),
                message: format!("{} is already at latest version ({})", name, latest_version),
            });
        }

        // 3. Install the latest version (install_auz overwrites existing)
        let install_result = self.install(name, &latest_version);

        match install_result {
            Ok(result) => {
                let version_str = result.version.clone();
                Ok(UpdateResult {
                    success: true,
                    name: name.to_string(),
                    old_version,
                    new_version: version_str.clone(),
                    message: format!("Updated {} to v{}", name, version_str),
                })
            }
            Err(e) => Err(format!("Update failed: {}", e)),
        }
    }

    /// Uninstall a standard library package.
    ///
    /// Removes the installed files from the installation directory and
    /// cleans up the local cache.
    ///
    /// # Arguments
    /// * `name` - Package name (e.g. `"aura-stdlib"`)
    pub fn uninstall(&self, name: &str) -> Result<(), String> {
        let install_config = InstallConfig {
            install_dir: self.install_dir.clone(),
            verify_checksum: false,
        };

        let result = uninstall_auz(&install_config);
        if !result.success {
            return Err(format!("Uninstall failed for {}: {}", name, result.message));
        }

        // Clean up local cache entries for this package
        let versions = self
            .local
            .versions(name)
            .map_err(|e| format!("Failed to query local registry: {}", e))?;
        for entry in &versions {
            let _ = self.local.remove(name, &entry.version);
        }

        Ok(())
    }

    /// List all standard library packages available in the registry.
    ///
    /// Queries the registry REST API and returns entries with download
    /// counts and publication timestamps.
    ///
    /// # Returns
    /// A vector of `RegistryEntry` items.
    pub fn list(&self) -> Result<Vec<RegistryEntry>, String> {
        // Query the registry for the stdlib package
        let package_info = self
            .registry
            .get_package("aura-stdlib")
            .map_err(|e| format!("Failed to list packages: {}", e))?;

        let mut entries = Vec::new();
        for vi in &package_info.versions {
            let entry = RegistryEntry {
                name: package_info.name.clone(),
                version: vi.version.clone(),
                published_at: vi.released.clone(),
                download_count: 0, // download count not available in current API
            };
            entries.push(entry);
        }

        // Also include locally installed packages
        let local_packages = self
            .local
            .list_packages()
            .map_err(|e| format!("Failed to list local packages: {}", e))?;

        for name in &local_packages {
            if name == "aura-stdlib" {
                continue; // already included
            }
            let local_versions = self
                .local
                .versions(name)
                .map_err(|e| format!("Failed to list local versions for {}: {}", name, e))?;
            for entry in local_versions {
                entries.push(RegistryEntry {
                    name: name.clone(),
                    version: entry.version,
                    published_at: entry.installed_at,
                    download_count: 0,
                });
            }
        }

        Ok(entries)
    }

    // ── Private helpers ──────────────────────────────────────

    /// Download a `.auz` artifact to the local cache directory.
    fn download_to_local(&self, name: &str, version: &str, url: &str) -> Result<PathBuf, String> {
        let cache_dir = self.local.root_dir().join(name).join(version);
        std::fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Cannot create cache dir: {}", e))?;

        let dest = cache_dir.join(format!("{}.auz", name));
        self.registry.download(url, &dest).map_err(|e| format!("Download failed: {}", e))?;

        Ok(dest)
    }

    /// Compute SHA-256 checksum of a file.
    fn compute_checksum(&self, path: &Path) -> Result<String, String> {
        let content = std::fs::read(path)
            .map_err(|e| format!("Failed to read {} for checksum: {}", path.display(), e))?;
        let hash = sha256_hash(&content);
        Ok(format!("sha256:{}", hash))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Read the installed version from a manifest.json in the install directory.
fn get_installed_version(config: &InstallConfig) -> Option<String> {
    let manifest_path = config.install_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&manifest_path).ok()?;
    let manifest: crate::package::builder::StdlibManifest = serde_json::from_str(&content).ok()?;
    Some(manifest.version)
}

/// Compute SHA-256 hash of data.
fn sha256_hash(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unit tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::builder::{ExecutionModeId, StdlibBuildOptions};
    use crate::package::installer::update_auz;
    use crate::package::reader::read_auz_package;
    use crate::stdlib::{
        ExecutionMode, FfiDeclaration, FfiIndex, FfiMode, StdlibIndex, StdlibModule,
    };

    /// Helper: create a test stdlib index with a couple of modules.
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

    /// Helper: build a minimal .auz package and return the path.
    fn build_test_package(tmp: &Path) -> PathBuf {
        let idx = make_test_stdlib_index();

        let auc_dir = tmp.join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc-math").unwrap();
        std::fs::write(auc_dir.join("String.auc"), b"fake-auc-string").unwrap();

        let builder = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "1.0.0".to_string(),
                execution_modes: vec![crate::package::builder::ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );

        let output_path = tmp.join("aura-stdlib-1.0.0.auz");
        builder.build(&output_path).unwrap();
        output_path
    }

    #[test]
    fn test_registry_entry_serialize() {
        let entry = RegistryEntry {
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            published_at: Some("2024-01-15T10:00:00Z".to_string()),
            download_count: 42,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("aura-stdlib"));
        assert!(json.contains("1.0.0"));
        assert!(json.contains("2024-01-15"));

        let parsed: RegistryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "aura-stdlib");
        assert_eq!(parsed.version, "1.0.0");
        assert_eq!(parsed.download_count, 42);
    }

    #[test]
    fn test_publish_result_debug() {
        let result = PublishResult {
            url: "https://registry.example.com/v1/packages/aura-stdlib/versions/1.0.0/aura-stdlib.auz"
                .to_string(),
            checksum: "sha256:abc123".to_string(),
            size: 12345,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("aura-stdlib"));
        assert!(debug_str.contains("sha256:abc123"));
    }

    #[test]
    fn test_install_result_registry_fields() {
        let result = InstallResultRegistry {
            success: true,
            name: "aura-stdlib".to_string(),
            version: "1.0.0".to_string(),
            files_installed: 10,
            install_dir: PathBuf::from("/tmp/test-install"),
            message: "Installed aura-stdlib v1.0.0".to_string(),
        };
        assert!(result.success);
        assert_eq!(result.name, "aura-stdlib");
        assert_eq!(result.version, "1.0.0");
        assert_eq!(result.files_installed, 10);
        assert!(result.message.contains("Installed"));
    }

    #[test]
    fn test_update_result_fields() {
        let result = UpdateResult {
            success: true,
            name: "aura-stdlib".to_string(),
            old_version: Some("1.0.0".to_string()),
            new_version: "1.1.0".to_string(),
            message: "Updated aura-stdlib to v1.1.0".to_string(),
        };
        assert!(result.success);
        assert_eq!(result.old_version.as_deref(), Some("1.0.0"));
        assert_eq!(result.new_version, "1.1.0");
    }

    #[test]
    fn test_stdlib_registry_client_creation() {
        let config = RegistryConfig::new("https://registry.aura-lang.dev");
        let tmp = tempfile::tempdir().unwrap();
        let local = LocalRegistry::new(tmp.path().join("registry"));
        let client = StdlibRegistryClient::new(config, tmp.path().join("install"), local);
        assert!(client.is_ok());
        let client = client.unwrap();
        assert!(client.install_dir().ends_with("install"));
        assert!(client.local_registry().root_dir().ends_with("registry"));
    }

    #[test]
    fn test_build_and_read_package_integration() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a .auz package
        let auz_path = build_test_package(tmp.path());
        assert!(auz_path.exists());
        assert!(auz_path.metadata().unwrap().len() > 0);

        // Read it back
        let content = read_auz_package(&auz_path).unwrap();
        assert_eq!(content.manifest.name, "aura-stdlib");
        assert_eq!(content.manifest.version, "1.0.0");
        assert!(content.verified);
        assert!(content.auc_file_count > 0);
        assert!(content.files.contains_key("META-INF/manifest.json"));
    }

    #[test]
    fn test_install_and_uninstall_via_registry_flow() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a test package
        let auz_path = build_test_package(tmp.path());

        // Simulate what StdlibRegistryClient::install does internally
        let install_dir = tmp.path().join("installed");
        let install_config = InstallConfig {
            install_dir: install_dir.clone(),
            verify_checksum: true,
        };

        let result = install_auz(&auz_path, &install_config);
        assert!(result.success);
        assert_eq!(result.name, "aura-stdlib");
        assert_eq!(result.version, "1.0.0");
        assert!(result.files_installed > 0);

        // Verify installed
        let info = get_installed_version(&install_config);
        assert_eq!(info.as_deref(), Some("1.0.0"));

        // Uninstall
        let uninstall_result = uninstall_auz(&install_config);
        assert!(uninstall_result.success);

        // Verify uninstalled
        assert!(get_installed_version(&install_config).is_none());
    }

    #[test]
    fn test_update_flow_simulated() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = make_test_stdlib_index();

        let auc_dir = tmp.path().join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc-v1").unwrap();

        let install_dir = tmp.path().join("installed");

        // Install v1
        let builder_v1 = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "1.0.0".to_string(),
                execution_modes: vec![crate::package::builder::ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );
        let auz_v1 = tmp.path().join("std_v1.auz");
        builder_v1.build(&auz_v1).unwrap();

        let config = InstallConfig {
            install_dir: install_dir.clone(),
            verify_checksum: true,
        };
        let r1 = install_auz(&auz_v1, &config);
        assert!(r1.success);
        assert_eq!(get_installed_version(&config).as_deref(), Some("1.0.0"));

        // Update to v2
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc-v2").unwrap();
        let builder_v2 = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "2.0.0".to_string(),
                execution_modes: vec![crate::package::builder::ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );
        let auz_v2 = tmp.path().join("std_v2.auz");
        builder_v2.build(&auz_v2).unwrap();

        let r2 = update_auz(&auz_v2, &config);
        assert!(r2.success);
        assert_eq!(r2.version, "2.0.0");
        assert_eq!(get_installed_version(&config).as_deref(), Some("2.0.0"));
    }

    #[test]
    fn test_checksum_verification() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a package
        let auz_path = build_test_package(tmp.path());

        // Compute checksum
        let content = std::fs::read(&auz_path).unwrap();
        let hash = sha256_hash(&content);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // Verify the package can be read and checksum passes
        let package = read_auz_package(&auz_path).unwrap();
        assert!(package.verified);
    }

    #[test]
    fn test_three_execution_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let idx = make_test_stdlib_index();

        // Create .auc files
        let auc_dir = tmp.path().join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"auc-math").unwrap();
        std::fs::write(auc_dir.join("String.auc"), b"auc-string").unwrap();

        // Create native directory with fake .a files
        let native_dir = tmp.path().join("native");
        std::fs::create_dir_all(&native_dir).unwrap();
        std::fs::write(native_dir.join("libstd.a"), b"native-lib").unwrap();

        // FFI index
        let mut ffi_idx = FfiIndex::new(FfiMode::Aot);
        ffi_idx.declarations.push(FfiDeclaration {
            name: "fopen".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: None,
        });

        let builder = StdlibPackageBuilder::new(&idx)
            .with_auc_dir(&auc_dir)
            .with_native_dir(&native_dir)
            .with_ffi_index(&ffi_idx)
            .with_options(StdlibBuildOptions::default_all_modes());

        let output_path = tmp.path().join("std_3modes.auz");
        let result = builder.build(&output_path);
        assert!(result.is_ok(), "Build failed: {:?}", result.err());

        let r = result.unwrap();
        assert_eq!(r.manifest.execution_modes.len(), 3);
        assert!(r.manifest.artifacts.contains_key("vm"));
        assert!(r.manifest.artifacts.contains_key("jit"));
        assert!(r.manifest.artifacts.contains_key("aot"));
        assert!(r.file_count >= 5); // manifest + checksum + at least 3 files

        // Verify the package reads back correctly
        let content = read_auz_package(&output_path).unwrap();
        assert!(content.verified);
        assert_eq!(content.manifest.execution_modes.len(), 3);
        assert!(content.auc_files().len() >= 1);
    }

    #[test]
    fn test_local_registry_cache_integration() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = LocalRegistry::new(tmp.path().join("cache"));

        // Build a test package
        let auz_path = build_test_package(tmp.path());

        // Install to local registry
        let entry = registry.install("aura-stdlib", "1.0.0", &auz_path, None).unwrap();
        assert_eq!(entry.name, "aura-stdlib");
        assert_eq!(entry.version, "1.0.0");
        assert!(entry.checksum.is_some());

        // Find it
        let found = registry.find("aura-stdlib", "1.0.0");
        assert!(found.is_some());
        assert!(found.unwrap().exists());

        // Find latest
        let latest = registry.find_latest("aura-stdlib");
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().0, "1.0.0");

        // List packages
        let packages = registry.list_packages().unwrap();
        assert!(packages.contains(&"aura-stdlib".to_string()));

        // Remove
        registry.remove("aura-stdlib", "1.0.0").unwrap();
        assert!(registry.find("aura-stdlib", "1.0.0").is_none());
    }

    #[test]
    fn test_package_roundtrip_checksum_mismatch() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a package
        let auz_path = build_test_package(tmp.path());

        // Corrupt the file
        let content = std::fs::read(&auz_path).unwrap();
        let mut corrupted = content;
        // Flip some bits in the middle
        if corrupted.len() > 20 {
            corrupted[10] ^= 0xFF;
        }
        std::fs::write(&auz_path, &corrupted).unwrap();

        // Reading may still work but checksum should fail
        match read_auz_package(&auz_path) {
            Ok(c) => {
                // If it can be read, checksum should not pass
                assert!(!c.verified);
            }
            Err(_) => {
                // Corrupted enough to fail parsing — also acceptable
            }
        }
    }

    #[test]
    fn test_sha256_hash_known() {
        // Verify known SHA-256 hash for "hello"
        let hash = sha256_hash(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
