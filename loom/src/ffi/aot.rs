//! [Phase 3] FFI AOT direct call configuration for the loom build system.
//!
//! Mirrors the compiler-side FFI AOT configuration
//! (see `compiler/src/codegen/ffi_aot.rs`) but expressed in terms of the loom
//! build pipeline: it decides which target mode (VM / JIT / AOT) the FFI
//! declarations are configured for, and how the inline cache and PLT
//! strategies are wired in at build time.
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

/// FFI AOT target execution mode.
///
/// Three-state mode selection aligned with the bootstrap layer
/// (`loom/src/stdlib::ExecutionMode`) and the compiler
/// (`compiler::codegen::ffi_aot::ExecutionMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AotFfiTarget {
    /// VM interpreted execution (default).
    #[default]
    Vm,
    /// JIT ahead-of-time compilation.
    Jit,
    /// AOT compilation to native machine code.
    Aot,
}

impl std::fmt::Display for AotFfiTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AotFfiTarget::Vm => write!(f, "vm"),
            AotFfiTarget::Jit => write!(f, "jit"),
            AotFfiTarget::Aot => write!(f, "aot"),
        }
    }
}

/// FFI AOT direct call configuration.
///
/// Controls how FFI declarations are configured at build time.
/// This is the loom-side counterpart to the compiler's `FfiAotConfig`.
#[derive(Debug, Clone)]
pub struct FfiAotConfig {
    /// Inline the FFI call site when possible.
    pub inline: bool,
    /// LLVM optimization level (0–3).
    pub optimize: u8,
    /// Static-link the FFI library into the binary.
    pub static_link: bool,
    /// Enable inline cache (VM mode only).
    pub enable_inline_cache: bool,
    /// Enable PLT lazy resolution (JIT mode only).
    pub enable_plt: bool,
}

impl Default for FfiAotConfig {
    fn default() -> Self {
        Self {
            inline: false,
            optimize: 2,
            static_link: false,
            enable_inline_cache: true,
            enable_plt: true,
        }
    }
}

/// Result of configuring AOT FFI direct calls.
///
/// Summarises what the loom build pipeline will generate for FFI
/// declarations, given a target mode and a configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiConfigResult {
    /// The execution mode the configuration was resolved for.
    pub mode: AotFfiTarget,
    /// Number of FFI declarations that will be configured.
    ///
    /// This is derived from the target mode:
    /// - VM: 1 (preloaded library-addresses map)
    /// - JIT: 1 (PLT stub entry)
    /// - AOT: 2 (import + export symbol tables)
    pub declarations: usize,
    /// Whether the inline cache will be enabled.
    ///
    /// Only meaningful for VM mode; forced to `false` for JIT/AOT.
    pub inline_cache: bool,
    /// Whether PLT lazy resolution will be enabled.
    ///
    /// Only meaningful for JIT mode; forced to `false` for VM/AOT.
    pub plt: bool,
}

/// Configure AOT FFI direct calls for the given target mode.
///
/// Resolves the effective configuration for the loom build pipeline.
/// This function does not inspect actual FFI declarations — that is the
/// compiler's responsibility. It only validates the configuration and
/// computes the effective mode-dependent flags.
///
/// # Errors
///
/// Returns an error if `config.optimize` is out of the valid range (0–3).
pub fn configure_aot_direct(
    mode: AotFfiTarget,
    config: &FfiAotConfig,
) -> Result<FfiConfigResult, String> {
    if config.optimize > 3 {
        return Err(format!(
            "FFI AOT configuration error: optimize level must be 0-3, got {}",
            config.optimize
        ));
    }

    // Declaration count is derived from the target mode:
    //   VM  → 1 (preloaded library-addresses map)
    //   JIT → 1 (PLT stub entry)
    //   AOT → 2 (import + export symbol tables)
    let declarations = match mode {
        AotFfiTarget::Vm => 1,
        AotFfiTarget::Jit => 1,
        AotFfiTarget::Aot => 2,
    };

    // Inline cache is only meaningful for VM mode.
    let inline_cache = config.enable_inline_cache && mode == AotFfiTarget::Vm;

    // PLT lazy resolution is only meaningful for JIT mode.
    let plt = config.enable_plt && mode == AotFfiTarget::Jit;

    Ok(FfiConfigResult {
        mode,
        declarations,
        inline_cache,
        plt,
    })
}

/// Parse an `AotFfiTarget` from a string (case-insensitive).
///
/// Accepts `"vm"`, `"default"`, `"jit"`, `"aot"`.
pub fn parse_aot_target(s: &str) -> Result<AotFfiTarget, String> {
    match s.to_ascii_lowercase().as_str() {
        "vm" | "default" => Ok(AotFfiTarget::Vm),
        "jit" => Ok(AotFfiTarget::Jit),
        "aot" => Ok(AotFfiTarget::Aot),
        _ => Err(format!("unknown FFI AOT target: {}", s)),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_aot_config_default() {
        let config = FfiAotConfig::default();
        assert!(!config.inline);
        assert_eq!(config.optimize, 2);
        assert!(!config.static_link);
        assert!(config.enable_inline_cache);
        assert!(config.enable_plt);
    }

    #[test]
    fn test_aot_ffi_target_display() {
        assert_eq!(AotFfiTarget::Vm.to_string(), "vm");
        assert_eq!(AotFfiTarget::Jit.to_string(), "jit");
        assert_eq!(AotFfiTarget::Aot.to_string(), "aot");
    }

    #[test]
    fn test_aot_ffi_target_default() {
        assert_eq!(AotFfiTarget::default(), AotFfiTarget::Vm);
    }

    #[test]
    fn test_parse_aot_target_valid() {
        assert_eq!(parse_aot_target("vm").unwrap(), AotFfiTarget::Vm);
        assert_eq!(parse_aot_target("default").unwrap(), AotFfiTarget::Vm);
        assert_eq!(parse_aot_target("jit").unwrap(), AotFfiTarget::Jit);
        assert_eq!(parse_aot_target("JIT").unwrap(), AotFfiTarget::Jit);
        assert_eq!(parse_aot_target("aot").unwrap(), AotFfiTarget::Aot);
        assert_eq!(parse_aot_target("AOT").unwrap(), AotFfiTarget::Aot);
    }

    #[test]
    fn test_parse_aot_target_invalid() {
        let result = parse_aot_target("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown FFI AOT target"));
    }

    #[test]
    fn test_configure_vm_mode() {
        let result = configure_aot_direct(AotFfiTarget::Vm, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.mode, AotFfiTarget::Vm);
        assert_eq!(result.declarations, 1);
        assert!(result.inline_cache, "VM mode should enable inline cache");
        assert!(!result.plt, "VM mode should not enable PLT");
    }

    #[test]
    fn test_configure_jit_mode() {
        let result = configure_aot_direct(AotFfiTarget::Jit, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.mode, AotFfiTarget::Jit);
        assert_eq!(result.declarations, 1);
        assert!(
            !result.inline_cache,
            "JIT mode should not enable inline cache"
        );
        assert!(result.plt, "JIT mode should enable PLT");
    }

    #[test]
    fn test_configure_aot_mode() {
        let result = configure_aot_direct(AotFfiTarget::Aot, &FfiAotConfig::default()).unwrap();
        assert_eq!(result.mode, AotFfiTarget::Aot);
        assert_eq!(result.declarations, 2);
        assert!(
            !result.inline_cache,
            "AOT mode should not enable inline cache"
        );
        assert!(!result.plt, "AOT mode should not enable PLT");
    }

    #[test]
    fn test_configure_vm_with_inline_cache_disabled() {
        let config = FfiAotConfig {
            enable_inline_cache: false,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Vm, &config).unwrap();
        assert!(!result.inline_cache);
    }

    #[test]
    fn test_configure_jit_with_plt_disabled() {
        let config = FfiAotConfig {
            enable_plt: false,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Jit, &config).unwrap();
        assert!(!result.plt);
    }

    #[test]
    fn test_configure_invalid_optimize_level() {
        let config = FfiAotConfig {
            optimize: 4,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Vm, &config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("optimize level"));
        assert!(err.contains("4"));
    }

    #[test]
    fn test_configure_optimize_level_max_valid() {
        let config = FfiAotConfig {
            optimize: 3,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Aot, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_configure_optimize_level_zero_valid() {
        let config = FfiAotConfig {
            optimize: 0,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Vm, &config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_config_result_cloning() {
        let config = FfiAotConfig {
            inline: true,
            static_link: true,
            optimize: 1,
            ..Default::default()
        };
        let result = configure_aot_direct(AotFfiTarget::Aot, &config).unwrap();
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_config_result_equality() {
        let result_a = configure_aot_direct(AotFfiTarget::Vm, &FfiAotConfig::default()).unwrap();
        let result_b = configure_aot_direct(AotFfiTarget::Vm, &FfiAotConfig::default()).unwrap();
        assert_eq!(result_a, result_b);
    }

    #[test]
    fn test_all_modes_declaration_counts() {
        let config = FfiAotConfig::default();
        let vm = configure_aot_direct(AotFfiTarget::Vm, &config).unwrap();
        let jit = configure_aot_direct(AotFfiTarget::Jit, &config).unwrap();
        let aot = configure_aot_direct(AotFfiTarget::Aot, &config).unwrap();
        assert_eq!(vm.declarations, 1);
        assert_eq!(jit.declarations, 1);
        assert_eq!(aot.declarations, 2);
    }
}
