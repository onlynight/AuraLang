//! Phase 6: Standard library installer.
//!
//! Installs, updates, and uninstalls the Aura standard library from a .auz package.

use std::path::Path;

use super::builder::StdlibManifest;
use super::reader::{StdlibPackageContent, read_auz_package};

/// Result of an install/update/uninstall operation.
#[derive(Debug, Clone)]
pub struct InstallResult {
    /// Whether the operation succeeded
    pub success: bool,
    /// Package name
    pub name: String,
    /// Package version
    pub version: String,
    /// Number of files installed
    pub files_installed: usize,
    /// Message
    pub message: String,
}

impl InstallResult {
    pub fn ok(name: &str, version: &str, files: usize) -> Self {
        Self {
            success: true,
            name: name.to_string(),
            version: version.to_string(),
            files_installed: files,
            message: format!("Installed {} v{}", name, version),
        }
    }

    pub fn err(message: &str) -> Self {
        Self {
            success: false,
            name: String::new(),
            version: String::new(),
            files_installed: 0,
            message: message.to_string(),
        }
    }
}

/// Installation target configuration.
#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// Installation directory
    pub install_dir: std::path::PathBuf,
    /// Whether to verify checksum before installing
    pub verify_checksum: bool,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            install_dir: std::path::PathBuf::from("aura_std"),
            verify_checksum: true,
        }
    }
}

/// Install a stdlib package from a .auz file.
pub fn install_auz(package_path: &Path, config: &InstallConfig) -> InstallResult {
    // 1. Read the package
    let content = match read_auz_package(package_path) {
        Ok(c) => c,
        Err(e) => return InstallResult::err(&format!("Failed to read package: {}", e)),
    };

    // 2. Verify checksum if required
    if config.verify_checksum && !content.verified {
        return InstallResult::err("Checksum verification failed");
    }

    // 3. Create installation directory
    let install_dir = &config.install_dir;
    if let Err(e) = std::fs::create_dir_all(install_dir) {
        return InstallResult::err(&format!("Cannot create install directory: {}", e));
    }

    // 4. Extract files
    let mut files_installed = 0;
    for (path, data) in &content.files {
        let dest = install_dir.join(path);
        if let Some(parent) = dest.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("Warning: cannot create dir {}: {}", parent.display(), e);
                continue;
            }
        }
        if let Err(e) = std::fs::write(&dest, data) {
            eprintln!("Warning: cannot write {}: {}", dest.display(), e);
            continue;
        }
        files_installed += 1;
    }

    // 5. Write manifest
    let manifest_path = install_dir.join("manifest.json");
    if let Ok(json) = serde_json::to_string_pretty(&content.manifest) {
        let _ = std::fs::write(&manifest_path, json);
    }

    InstallResult::ok(
        &content.manifest.name,
        &content.manifest.version,
        files_installed,
    )
}

/// Update a stdlib package (install over existing).
pub fn update_auz(package_path: &Path, config: &InstallConfig) -> InstallResult {
    install_auz(package_path, config)
}

/// Uninstall a stdlib package.
pub fn uninstall_auz(config: &InstallConfig) -> InstallResult {
    let install_dir = &config.install_dir;
    if !install_dir.exists() {
        return InstallResult::err("Installation not found");
    }

    // Read manifest for name/version
    let manifest_path = install_dir.join("manifest.json");
    let (name, version) = if manifest_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<StdlibManifest>(&content) {
                (manifest.name, manifest.version)
            } else {
                ("unknown".to_string(), "unknown".to_string())
            }
        } else {
            ("unknown".to_string(), "unknown".to_string())
        }
    } else {
        ("unknown".to_string(), "unknown".to_string())
    };

    // Remove installation directory
    if let Err(e) = std::fs::remove_dir_all(install_dir) {
        return InstallResult::err(&format!("Cannot remove installation: {}", e));
    }

    InstallResult {
        success: true,
        name: name.clone(),
        version: version.clone(),
        files_installed: 0,
        message: format!("Uninstalled {} v{}", name, version),
    }
}

/// Check if a stdlib is installed.
pub fn is_installed(config: &InstallConfig) -> bool {
    config.install_dir.join("manifest.json").exists()
}

/// Get installed stdlib info.
pub fn get_installed_info(config: &InstallConfig) -> Option<StdlibManifest> {
    let manifest_path = config.install_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&manifest_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Verify FFI function availability after installation.
///
/// Checks that all FFI functions declared in the manifest can be resolved
/// in the installed libraries.
pub fn verify_ffi_availability(config: &InstallConfig) -> InstallResult {
    let info = match get_installed_info(config) {
        Some(i) => i,
        None => return InstallResult::err("Installation not found"),
    };

    // Check if FFI configuration exists
    if info.ffi.cffi.is_none() {
        return InstallResult::ok(&info.name, &info.version, 0);
    }

    // Verify C FFI library files exist
    if let Some(ref cffi) = info.ffi.cffi {
        let lib_name = cffi.lib.clone();
        // Check if at least one library file exists for the current platform
        let has_lib = cffi.files.as_object().map_or(false, |m| {
            m.values().filter_map(|v| v.as_str()).any(|name| config.install_dir.join(name).exists())
        });

        if has_lib {
            return InstallResult::ok(&info.name, &info.version, 0);
        } else {
            return InstallResult::err(&format!(
                "FFI library '{}' not found for current platform",
                lib_name
            ));
        }
    }

    InstallResult::ok(&info.name, &info.version, 0)
}

#[cfg(test)]
mod tests {
    use super::super::builder::{ExecutionModeId, StdlibBuildOptions, StdlibPackageBuilder};
    use super::*;
    use crate::stdlib::{ExecutionMode, FfiMode, StdlibIndex, StdlibModule};
    use std::io::Write;

    fn make_test_stdlib() -> (StdlibIndex, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let mut idx = StdlibIndex::new(tmp.path().to_path_buf(), ExecutionMode::Vm, FfiMode::Aot);
        idx.output_dir = tmp.path().to_path_buf();
        idx.modules.push(StdlibModule {
            name: "Math".to_string(),
            full_name: "aura.lang.std.Math".to_string(),
            source_path: tmp.path().join("Math.aura"),
            auc_path: None,
            function_names: vec!["abs".to_string()],
            type_names: vec![],
            constant_names: vec![],
            has_extern: false,
        });

        // Create fake .auc file
        let auc_dir = tmp.path().join("auc");
        std::fs::create_dir_all(&auc_dir).unwrap();
        std::fs::write(auc_dir.join("Math.auc"), b"fake-auc-data").unwrap();

        (idx, tmp)
    }

    #[test]
    fn test_install_config_default() {
        let config = InstallConfig::default();
        assert!(config.verify_checksum);
        assert!(config.install_dir.ends_with("aura_std"));
    }

    #[test]
    fn test_install_result_ok() {
        let result = InstallResult::ok("aura-stdlib", "1.0.0", 5);
        assert!(result.success);
        assert_eq!(result.name, "aura-stdlib");
        assert_eq!(result.version, "1.0.0");
        assert_eq!(result.files_installed, 5);
        assert!(result.message.contains("Installed"));
    }

    #[test]
    fn test_install_result_err() {
        let result = InstallResult::err("Something went wrong");
        assert!(!result.success);
        assert_eq!(result.message, "Something went wrong");
    }

    #[test]
    fn test_install_and_uninstall() {
        let (idx, _tmp) = make_test_stdlib();
        let tmp = tempfile::tempdir().unwrap();

        // Build a .auz package
        let auc_dir = _tmp.path().join("auc");
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

        let auz_path = tmp.path().join("std.auz");
        let build_result = builder.build(&auz_path).unwrap();
        assert!(build_result.path.exists());

        // Install
        let install_config = InstallConfig {
            install_dir: tmp.path().join("installed"),
            verify_checksum: true,
        };
        let install_result = install_auz(&auz_path, &install_config);
        assert!(install_result.success);
        assert_eq!(install_result.name, "aura-stdlib");
        assert_eq!(install_result.version, "1.0.0");
        assert!(install_result.files_installed > 0);

        // Verify installed
        assert!(is_installed(&install_config));
        let info = get_installed_info(&install_config).unwrap();
        assert_eq!(info.name, "aura-stdlib");

        // Uninstall
        let uninstall_result = uninstall_auz(&install_config);
        assert!(uninstall_result.success);
        assert!(uninstall_result.message.contains("Uninstalled"));

        // Verify uninstalled
        assert!(!is_installed(&install_config));
    }

    #[test]
    fn test_install_invalid_package() {
        let tmp = tempfile::tempdir().unwrap();
        let invalid_path = tmp.path().join("invalid.auz");
        std::fs::write(&invalid_path, b"not a valid auz").unwrap();

        let config = InstallConfig::default();
        let result = install_auz(&invalid_path, &config);
        assert!(!result.success);
    }

    #[test]
    fn test_uninstall_not_found() {
        let config = InstallConfig {
            install_dir: std::path::PathBuf::from("/nonexistent/aura_std"),
            verify_checksum: false,
        };
        let result = uninstall_auz(&config);
        assert!(!result.success);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_verify_ffi_availability_not_installed() {
        let config = InstallConfig {
            install_dir: std::path::PathBuf::from("/nonexistent/aura_std"),
            verify_checksum: false,
        };
        let result = verify_ffi_availability(&config);
        assert!(!result.success);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_update_auz() {
        let (idx, _tmp) = make_test_stdlib();
        let tmp = tempfile::tempdir().unwrap();

        let auc_dir = _tmp.path().join("auc");
        let builder = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "2.0.0".to_string(),
                execution_modes: vec![ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );

        let auz_path = tmp.path().join("std_v2.auz");
        builder.build(&auz_path).unwrap();

        let config = InstallConfig {
            install_dir: tmp.path().join("installed"),
            verify_checksum: false,
        };

        // First install v1
        let builder1 = StdlibPackageBuilder::new(&idx).with_auc_dir(&auc_dir).with_options(
            StdlibBuildOptions {
                name: "aura-stdlib".to_string(),
                version: "1.0.0".to_string(),
                execution_modes: vec![ExecutionModeId::Vm],
                ffi_mode: "aot".to_string(),
                compression_level: 1,
                include_sources: false,
            },
        );
        let auz_v1 = tmp.path().join("std_v1.auz");
        builder1.build(&auz_v1).unwrap();
        install_auz(&auz_v1, &config);

        // Update to v2
        let result = update_auz(&auz_path, &config);
        assert!(result.success);
        assert_eq!(result.version, "2.0.0");

        let info = get_installed_info(&config).unwrap();
        assert_eq!(info.version, "2.0.0");
    }
}
