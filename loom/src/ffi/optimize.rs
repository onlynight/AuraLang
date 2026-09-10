//! [Phase 3] FFI optimization configuration for the loom build system.
//! Corresponds to Phase 3 §5.3 and Phase 5 in the full Aura-ification plan.
//!
//! Mirrors the compiler-side FFI optimization report
//! (see `compiler/src/codegen/ffi_optimize.rs`). This module
//! provides the loom build-pipeline counterpart: it resolves the
//! effective optimization settings and produces a summary of what
//! the compiler should apply.

use std::collections::HashMap;

/// FFI optimization configuration.
///
/// Controls which FFI optimizations the loom build pipeline will
/// request from the compiler. This is the build-side counterpart to
/// the compiler's `FfiOptimizerConfig`.
#[derive(Debug, Clone)]
pub struct FfiOptimizeConfig {
    /// Enable inline expansion of FFI call sites.
    ///
    /// When enabled, FFI call sites that exceed the inline threshold
    /// are expanded inline by the compiler.
    pub enable_inline: bool,
    /// Enable PLT lazy resolution for FFI call sites.
    ///
    /// PLT (Procedure Linkage Table) allows the runtime to resolve
    /// function addresses on first call rather than at link time.
    pub enable_plt: bool,
    /// Enable LLVM optimization passes for FFI code.
    ///
    /// When enabled, the LLVM backend applies its standard optimization
    /// pipeline to FFI call sites and their surrounding code.
    pub enable_llvm_optimize: bool,
    /// Number of calls before a call site is eligible for inlining.
    ///
    /// Call sites with `call_count >= inline_threshold` are candidates
    /// for inline expansion. A threshold of `0` means every call site
    /// is eligible.
    pub inline_threshold: u64,
}

impl Default for FfiOptimizeConfig {
    fn default() -> Self {
        Self {
            enable_inline: true,
            enable_plt: true,
            enable_llvm_optimize: true,
            inline_threshold: 10,
        }
    }
}

/// Result of applying FFI optimizations.
///
/// Summarises the optimization decisions made by the loom build
/// pipeline. The counts reflect how many call sites were selected
/// for each optimization; they are populated by the compiler during
/// the actual build pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FfiOptimizeResult {
    /// Number of FFI call sites selected for inline expansion.
    pub inlined_count: u64,
    /// Number of FFI call sites configured with PLT lazy resolution.
    pub plt_count: u64,
    /// Whether LLVM optimization passes were applied to FFI code.
    pub llvm_optimized: bool,
}

impl FfiOptimizeResult {
    /// Check whether any optimization was applied.
    pub fn has_optimizations(&self) -> bool {
        self.inlined_count > 0 || self.plt_count > 0 || self.llvm_optimized
    }

    /// Merge another `FfiOptimizeResult` into this one, summing counts
    /// and OR-ing the LLVM flag.
    pub fn merge(&mut self, other: &FfiOptimizeResult) {
        self.inlined_count += other.inlined_count;
        self.plt_count += other.plt_count;
        self.llvm_optimized = self.llvm_optimized || other.llvm_optimized;
    }
}

/// Apply FFI optimizations based on the given configuration.
///
/// This is the primary entry point for the loom build pipeline. It
/// resolves the effective optimization settings and returns a summary.
///
/// Since this function does not receive call-site data, the counts
/// are initialised to zero. They are populated by the compiler
/// during the actual build pass. The `llvm_optimized` flag directly
/// reflects the configuration.
///
/// For a data-driven version that takes call counts, see
/// [`apply_optimizations_with_counts`].
pub fn apply_optimizations(config: &FfiOptimizeConfig) -> FfiOptimizeResult {
    FfiOptimizeResult {
        inlined_count: 0,
        plt_count: 0,
        llvm_optimized: config.enable_llvm_optimize,
    }
}

/// Apply FFI optimizations using call-count data.
///
/// This is a data-driven variant of [`apply_optimizations`] that takes
/// per-function call counts and selects call sites for inlining based
/// on the threshold. PLT is applied to all call sites when enabled.
///
/// # Arguments
///
/// * `call_counts` — mapping from function name to call count
/// * `config` — optimization configuration
///
/// # Returns
///
/// A result summarising the number of inlined and PLT-configured
/// call sites, and whether LLVM optimization is enabled.
pub fn apply_optimizations_with_counts(
    call_counts: &HashMap<String, u64>,
    config: &FfiOptimizeConfig,
) -> FfiOptimizeResult {
    let mut inlined_count = 0u64;
    let mut plt_count = 0u64;

    for (_name, &count) in call_counts {
        // PLT applies to every call site when enabled.
        if config.enable_plt {
            plt_count += 1;
        }
        // Inline if the call count meets the threshold.
        if config.enable_inline && count >= config.inline_threshold {
            inlined_count += 1;
        }
    }

    FfiOptimizeResult {
        inlined_count,
        plt_count,
        llvm_optimized: config.enable_llvm_optimize,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_optimize_config_default() {
        let config = FfiOptimizeConfig::default();
        assert!(config.enable_inline);
        assert!(config.enable_plt);
        assert!(config.enable_llvm_optimize);
        assert_eq!(config.inline_threshold, 10);
    }

    #[test]
    fn test_ffi_optimize_result_default() {
        let result = FfiOptimizeResult::default();
        assert_eq!(result.inlined_count, 0);
        assert_eq!(result.plt_count, 0);
        assert!(!result.llvm_optimized);
        assert!(!result.has_optimizations());
    }

    #[test]
    fn test_apply_optimizations_default_config() {
        let config = FfiOptimizeConfig::default();
        let result = apply_optimizations(&config);
        assert_eq!(result.inlined_count, 0);
        assert_eq!(result.plt_count, 0);
        assert!(result.llvm_optimized);
        assert!(result.has_optimizations());
    }

    #[test]
    fn test_apply_optimizations_llvm_disabled() {
        let config = FfiOptimizeConfig {
            enable_llvm_optimize: false,
            ..Default::default()
        };
        let result = apply_optimizations(&config);
        assert!(!result.llvm_optimized);
        assert!(!result.has_optimizations());
    }

    #[test]
    fn test_apply_optimizations_with_counts_empty() {
        let counts: HashMap<String, u64> = HashMap::new();
        let config = FfiOptimizeConfig::default();
        let result = apply_optimizations_with_counts(&counts, &config);
        assert_eq!(result.inlined_count, 0);
        assert_eq!(result.plt_count, 0);
        assert!(result.llvm_optimized);
    }

    #[test]
    fn test_apply_optimizations_with_counts_below_threshold() {
        let mut counts = HashMap::new();
        counts.insert("slow_func".to_string(), 5u64);
        counts.insert("rare_func".to_string(), 1u64);
        let config = FfiOptimizeConfig {
            inline_threshold: 10,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        // Neither call site exceeds the threshold of 10.
        assert_eq!(result.inlined_count, 0);
        // PLT applies to both call sites.
        assert_eq!(result.plt_count, 2);
        assert!(result.llvm_optimized);
    }

    #[test]
    fn test_apply_optimizations_with_counts_above_threshold() {
        let mut counts = HashMap::new();
        counts.insert("hot_func".to_string(), 100u64);
        counts.insert("warm_func".to_string(), 50u64);
        counts.insert("cold_func".to_string(), 2u64);
        let config = FfiOptimizeConfig {
            inline_threshold: 10,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        // 2 call sites exceed the threshold.
        assert_eq!(result.inlined_count, 2);
        // PLT applies to all 3 call sites.
        assert_eq!(result.plt_count, 3);
        assert!(result.llvm_optimized);
    }

    #[test]
    fn test_apply_optimizations_with_counts_exact_threshold() {
        let mut counts = HashMap::new();
        counts.insert("borderline".to_string(), 10u64);
        let config = FfiOptimizeConfig {
            inline_threshold: 10,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        // Exactly at the threshold: should be inlined.
        assert_eq!(result.inlined_count, 1);
        assert_eq!(result.plt_count, 1);
    }

    #[test]
    fn test_apply_optimizations_with_counts_zero_threshold() {
        let mut counts = HashMap::new();
        counts.insert("once".to_string(), 1u64);
        counts.insert("never_used".to_string(), 0u64);
        let config = FfiOptimizeConfig {
            inline_threshold: 0,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        // Threshold 0: every call site is eligible.
        assert_eq!(result.inlined_count, 2);
        assert_eq!(result.plt_count, 2);
    }

    #[test]
    fn test_apply_optimizations_inline_disabled() {
        let mut counts = HashMap::new();
        counts.insert("hot_func".to_string(), 1000u64);
        let config = FfiOptimizeConfig {
            enable_inline: false,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        assert_eq!(result.inlined_count, 0);
        // PLT is still enabled.
        assert_eq!(result.plt_count, 1);
    }

    #[test]
    fn test_apply_optimizations_plt_disabled() {
        let mut counts = HashMap::new();
        counts.insert("hot_func".to_string(), 1000u64);
        counts.insert("warm_func".to_string(), 20u64);
        let config = FfiOptimizeConfig {
            enable_plt: false,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        assert_eq!(result.inlined_count, 2);
        assert_eq!(result.plt_count, 0);
    }

    #[test]
    fn test_apply_optimizations_all_disabled() {
        let mut counts = HashMap::new();
        counts.insert("func".to_string(), 1000u64);
        let config = FfiOptimizeConfig {
            enable_inline: false,
            enable_plt: false,
            enable_llvm_optimize: false,
            ..Default::default()
        };
        let result = apply_optimizations_with_counts(&counts, &config);
        assert_eq!(result.inlined_count, 0);
        assert_eq!(result.plt_count, 0);
        assert!(!result.llvm_optimized);
        assert!(!result.has_optimizations());
    }

    #[test]
    fn test_result_has_optimizations_inlined() {
        let result = FfiOptimizeResult {
            inlined_count: 1,
            plt_count: 0,
            llvm_optimized: false,
        };
        assert!(result.has_optimizations());
    }

    #[test]
    fn test_result_has_optimizations_plt() {
        let result = FfiOptimizeResult {
            inlined_count: 0,
            plt_count: 5,
            llvm_optimized: false,
        };
        assert!(result.has_optimizations());
    }

    #[test]
    fn test_result_has_optimizations_llvm() {
        let result = FfiOptimizeResult {
            inlined_count: 0,
            plt_count: 0,
            llvm_optimized: true,
        };
        assert!(result.has_optimizations());
    }

    #[test]
    fn test_result_merge() {
        let mut a = FfiOptimizeResult {
            inlined_count: 3,
            plt_count: 5,
            llvm_optimized: true,
        };
        let b = FfiOptimizeResult {
            inlined_count: 2,
            plt_count: 3,
            llvm_optimized: false,
        };
        a.merge(&b);
        assert_eq!(a.inlined_count, 5);
        assert_eq!(a.plt_count, 8);
        // OR: true || false = true
        assert!(a.llvm_optimized);
    }

    #[test]
    fn test_result_merge_both_llvm_false() {
        let mut a = FfiOptimizeResult {
            inlined_count: 0,
            plt_count: 0,
            llvm_optimized: false,
        };
        let b = FfiOptimizeResult {
            inlined_count: 0,
            plt_count: 0,
            llvm_optimized: false,
        };
        a.merge(&b);
        assert!(!a.llvm_optimized);
    }

    #[test]
    fn test_result_clone_and_equality() {
        let result = FfiOptimizeResult {
            inlined_count: 42,
            plt_count: 7,
            llvm_optimized: true,
        };
        let cloned = result.clone();
        assert_eq!(result, cloned);
    }

    #[test]
    fn test_config_clone() {
        let config = FfiOptimizeConfig {
            enable_inline: false,
            enable_plt: true,
            enable_llvm_optimize: false,
            inline_threshold: 99,
        };
        let cloned = config.clone();
        assert_eq!(cloned.inline_threshold, 99);
        assert!(!cloned.enable_inline);
        assert!(cloned.enable_plt);
        assert!(!cloned.enable_llvm_optimize);
    }
}
