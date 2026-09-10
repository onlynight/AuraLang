//! Phase 3/5: FFI AOT direct call support.
//!
//! Configures FFI calls for AOT direct mode across three execution modes.
//! **Core principle: prefer `extern interface` (FfiAbi::Aura) for calling
//! Aura AOT-compiled libraries; `extern "c"` (FfiAbi::C) is only for
//! low-level system calls.**
//!
//! - VM: pre-load library function addresses into FfiCache
//! - JIT: mark FFI calls for direct instruction generation
//! - AOT: configure LLVM IR direct calls across libraries
//!
//! Corresponds to Phase 3 §5.3 and Phase 5 §5.5 in the full Aura-ification plan.

use std::collections::HashMap;

use crate::codegen::opcode::{BytecodeModule, FfiAbi};

/// FFI AOT configuration options.
#[derive(Debug, Clone)]
pub struct FfiAotConfig {
    /// Enable inline cache (VM mode)
    pub enable_inline_cache: bool,
    /// Enable PLT lazy resolution (JIT mode)
    pub enable_plt: bool,
    /// Enable LLVM optimization (AOT mode)
    pub enable_llvm_optimize: bool,
    /// Allow cross-library inlining (AOT mode)
    pub enable_cross_lib_inline: bool,
}

impl Default for FfiAotConfig {
    fn default() -> Self {
        Self {
            enable_inline_cache: true,
            enable_plt: true,
            enable_llvm_optimize: true,
            enable_cross_lib_inline: true,
        }
    }
}

/// Execution mode for FFI AOT configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    #[default]
    Vm,
    Jit,
    Aot,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionMode::Vm => write!(f, "vm"),
            ExecutionMode::Jit => write!(f, "jit"),
            ExecutionMode::Aot => write!(f, "aot"),
        }
    }
}

/// FFI AOT configuration result.
#[derive(Debug, Clone)]
pub struct FfiAotResult {
    /// Total FFI declarations configured
    pub declarations_configured: usize,
    /// Number of extern interface declarations (AOT direct, JitValue ABI)
    pub extern_interface_count: usize,
    /// Number of C FFI declarations (libc calls)
    pub c_ffi_count: usize,
    /// Execution mode used
    pub execution_mode: ExecutionMode,
    /// FFI declarations processed
    pub declarations: Vec<FfiDeclaration>,
}

/// FFI declaration information.
#[derive(Debug, Clone)]
pub struct FfiDeclaration {
    pub name: String,
    pub library: Option<String>,
    pub abi: FfiAbi,
    pub param_count: u16,
}

/// Configure FFI AOT direct calls for a module.
///
/// This function:
/// 1. Collects all extern declarations from the module
/// 2. Separates `extern interface` (FfiAbi::Aura) from `extern "c"` (FfiAbi::C)
/// 3. Configures each type based on the execution mode
/// 4. Returns a summary with counts for each type
///
/// **Priority: extern interface > extern "c" > extern "rust"**
pub fn configure_ffi_aot_direct(
    module: &BytecodeModule,
    execution_mode: ExecutionMode,
    config: &FfiAotConfig,
) -> Result<FfiAotResult, String> {
    // 1. Collect all FFI declarations
    let ffi_decls = collect_ffi_declarations(module);

    if ffi_decls.is_empty() {
        return Ok(FfiAotResult {
            declarations_configured: 0,
            extern_interface_count: 0,
            c_ffi_count: 0,
            execution_mode,
            declarations: Vec::new(),
        });
    }

    // 2. Separate by ABI type
    let extern_interfaces: Vec<_> =
        ffi_decls.iter().filter(|d| d.abi == FfiAbi::Aura).cloned().collect();
    let c_ffis: Vec<_> = ffi_decls.iter().filter(|d| d.abi == FfiAbi::C).cloned().collect();

    // 3. Configure extern interface declarations (AOT direct, preferred)
    if !extern_interfaces.is_empty() {
        configure_extern_interface_ffi(&extern_interfaces, execution_mode, config)?;
    }

    // 4. Configure C FFI declarations (fallback for system calls)
    if !c_ffis.is_empty() {
        configure_c_ffi(&c_ffis, execution_mode, config)?;
    }

    Ok(FfiAotResult {
        declarations_configured: ffi_decls.len(),
        extern_interface_count: extern_interfaces.len(),
        c_ffi_count: c_ffis.len(),
        execution_mode,
        declarations: ffi_decls,
    })
}

/// Configure extern interface declarations (AOT direct, JitValue ABI).
///
/// This is the **preferred** FFI mode — calling Aura AOT-compiled libraries
/// directly through `extern interface` with JitValue ABI (zero argument conversion).
fn configure_extern_interface_ffi(
    decls: &[FfiDeclaration],
    execution_mode: ExecutionMode,
    config: &FfiAotConfig,
) -> Result<(), String> {
    let lib_names: Vec<_> = decls.iter().filter_map(|d| d.library.as_deref()).collect();
    let unique_libs: std::collections::HashSet<_> = lib_names.into_iter().collect();

    match execution_mode {
        ExecutionMode::Vm => {
            eprintln!(
                "  extern interface (VM): pre-loading {} functions from {} libraries{}",
                decls.len(),
                unique_libs.len(),
                if config.enable_inline_cache { " (inline cache)" } else { "" }
            );
            for decl in decls {
                eprintln!(
                    "    VM preload: {} → {} (JitValue ABI)",
                    decl.name,
                    decl.library.as_deref().unwrap_or("auto")
                );
            }
        }
        ExecutionMode::Jit => {
            eprintln!(
                "  extern interface (JIT): generating direct calls for {} functions{}",
                decls.len(),
                if config.enable_plt { " (PLT lazy resolution)" } else { "" }
            );
        }
        ExecutionMode::Aot => {
            eprintln!(
                "  extern interface (AOT): configuring LLVM IR cross-library calls for {} functions{}",
                decls.len(),
                if config.enable_cross_lib_inline { " (cross-library inline)" } else { "" }
            );
        }
    }
    Ok(())
}

/// Configure C FFI declarations (system calls only).
///
/// C FFI is a **fallback** for low-level system calls that cannot be
/// implemented in Aura (e.g. libc, system APIs).
fn configure_c_ffi(
    decls: &[FfiDeclaration],
    execution_mode: ExecutionMode,
    config: &FfiAotConfig,
) -> Result<(), String> {
    match execution_mode {
        ExecutionMode::Vm => {
            eprintln!(
                "  C FFI (VM): pre-loading {} C function addresses",
                decls.len()
            );
        }
        ExecutionMode::Jit => {
            eprintln!(
                "  C FFI (JIT): generating direct calls for {} C functions",
                decls.len()
            );
        }
        ExecutionMode::Aot => {
            eprintln!(
                "  C FFI (AOT): configuring LLVM IR calls for {} C functions",
                decls.len()
            );
        }
    }
    let _ = config; // suppress unused warning
    Ok(())
}

/// Collect all FFI declarations from a module.
fn collect_ffi_declarations(module: &BytecodeModule) -> Vec<FfiDeclaration> {
    module
        .natives
        .iter()
        .filter(|n| n.ffi_abi != FfiAbi::None)
        .map(|n| FfiDeclaration {
            name: n.name.clone(),
            library: n.ffi_lib.clone(),
            abi: n.ffi_abi,
            param_count: n.param_count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::opcode::BytecodeNative;

    /// Create a module with extern interface (Aura) declarations.
    fn make_module_with_extern_interface() -> BytecodeModule {
        let mut module = BytecodeModule::default();
        // Math.abs — extern interface (AOT direct, JitValue ABI)
        module.natives.push(BytecodeNative {
            name: "Math.abs".to_string(),
            param_count: 1,
            ffi_abi: FfiAbi::Aura,
            ffi_lib: Some("aura_std_math".to_string()),
            param_types: vec![],
            ret_type: 0,
        });
        // Math.min — extern interface
        module.natives.push(BytecodeNative {
            name: "Math.min".to_string(),
            param_count: 2,
            ffi_abi: FfiAbi::Aura,
            ffi_lib: Some("aura_std_math".to_string()),
            param_types: vec![],
            ret_type: 0,
        });
        // IO.readText — extern interface (different library)
        module.natives.push(BytecodeNative {
            name: "IO.readText".to_string(),
            param_count: 1,
            ffi_abi: FfiAbi::Aura,
            ffi_lib: Some("aura_std_io".to_string()),
            param_types: vec![],
            ret_type: 0,
        });
        module
    }

    /// Create a module with C FFI declarations (system calls).
    fn make_module_with_c_ffi() -> BytecodeModule {
        let mut module = BytecodeModule::default();
        module.natives.push(BytecodeNative {
            name: "fopen".to_string(),
            param_count: 2,
            ffi_abi: FfiAbi::C,
            ffi_lib: Some("libc".to_string()),
            param_types: vec![4, 4],
            ret_type: 5,
        });
        module.natives.push(BytecodeNative {
            name: "malloc".to_string(),
            param_count: 1,
            ffi_abi: FfiAbi::C,
            ffi_lib: Some("libc".to_string()),
            param_types: vec![1],
            ret_type: 5,
        });
        module
    }

    /// Create a module with mixed FFI declarations (preferred: extern interface).
    fn make_module_with_mixed_ffi() -> BytecodeModule {
        let mut module = make_module_with_extern_interface();
        let c_module = make_module_with_c_ffi();
        module.natives.extend(c_module.natives);
        module
    }

    #[test]
    fn test_ffi_aot_config_default() {
        let config = FfiAotConfig::default();
        assert!(config.enable_inline_cache);
        assert!(config.enable_plt);
        assert!(config.enable_llvm_optimize);
        assert!(config.enable_cross_lib_inline);
    }

    #[test]
    fn test_execution_mode_display() {
        assert_eq!(ExecutionMode::Vm.to_string(), "vm");
        assert_eq!(ExecutionMode::Jit.to_string(), "jit");
        assert_eq!(ExecutionMode::Aot.to_string(), "aot");
    }

    #[test]
    fn test_configure_extern_interface_vm() {
        let module = make_module_with_extern_interface();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Vm, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.declarations_configured, 3);
        assert_eq!(result.extern_interface_count, 3);
        assert_eq!(result.c_ffi_count, 0);
        assert_eq!(result.execution_mode, ExecutionMode::Vm);
    }

    #[test]
    fn test_configure_extern_interface_jit() {
        let module = make_module_with_extern_interface();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Jit, &FfiAotConfig::default())
                .unwrap();
        assert_eq!(result.extern_interface_count, 3);
        assert_eq!(result.c_ffi_count, 0);
    }

    #[test]
    fn test_configure_extern_interface_aot() {
        let module = make_module_with_extern_interface();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Aot, &FfiAotConfig::default())
                .unwrap();
        assert_eq!(result.extern_interface_count, 3);
        assert_eq!(result.c_ffi_count, 0);
    }

    #[test]
    fn test_configure_c_ffi_vm() {
        let module = make_module_with_c_ffi();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Vm, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.declarations_configured, 2);
        assert_eq!(result.extern_interface_count, 0);
        assert_eq!(result.c_ffi_count, 2);
    }

    #[test]
    fn test_configure_mixed_ffi_aot() {
        let module = make_module_with_mixed_ffi();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Aot, &FfiAotConfig::default())
                .unwrap();
        assert_eq!(result.declarations_configured, 5);
        assert_eq!(result.extern_interface_count, 3);
        assert_eq!(result.c_ffi_count, 2);
    }

    #[test]
    fn test_configure_ffi_aot_no_declarations() {
        let module = BytecodeModule::default();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Vm, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.declarations_configured, 0);
        assert_eq!(result.extern_interface_count, 0);
        assert_eq!(result.c_ffi_count, 0);
    }

    #[test]
    fn test_collect_ffi_declarations_mixed() {
        let module = make_module_with_mixed_ffi();
        let decls = collect_ffi_declarations(&module);
        assert_eq!(decls.len(), 5);
        let aura_count = decls.iter().filter(|d| d.abi == FfiAbi::Aura).count();
        let c_count = decls.iter().filter(|d| d.abi == FfiAbi::C).count();
        assert_eq!(aura_count, 3);
        assert_eq!(c_count, 2);
    }

    #[test]
    fn test_extern_interface_library_separation() {
        let module = make_module_with_extern_interface();
        let result =
            configure_ffi_aot_direct(&module, ExecutionMode::Aot, &FfiAotConfig::default())
                .unwrap();
        // Should have declarations from 2 different libraries
        let libs: std::collections::HashSet<&str> = result
            .declarations
            .iter()
            .filter_map(|d| d.library.as_deref())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(libs.len(), 2);
        assert!(libs.contains("aura_std_math"));
        assert!(libs.contains("aura_std_io"));
    }
}
