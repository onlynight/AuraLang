//! Phase 6: Standard library packaging and distribution.
//!
//! Provides .auz-based packaging of the Aura standard library with:
//! - Three execution mode support (VM/JIT/AOT)
//! - AOT native libraries and FFI mappings
//! - Checksum verification
//! - Install/update/uninstall operations

pub mod builder;
pub mod installer;
pub mod reader;

pub use builder::{
    AotFfiConfig, ArtifactEntry, ArtifactFormat, CffiConfig, ExecutionModeId, FfiConfig,
    StdlibBuildOptions, StdlibBuildResult, StdlibManifest, StdlibPackageBuilder, detect_platform,
};
pub use installer::{
    InstallConfig, InstallResult, get_installed_info, install_auz, is_installed, uninstall_auz,
    update_auz, verify_ffi_availability,
};
pub use reader::{StdlibPackageContent, read_auz_package};
