//! [Phase 3] FFI call cache configuration for the loom build system.
//!
//! Mirrors the compiler-side FFI cache (see `compiler/src/codegen/ffi_cache.rs`)
//! and `compiler/src/vm/ffi_cache.rs`, but expressed as build-time
//! configuration: the loom pipeline decides *how* the cache should be
//! initialised (preload, inline cache, hotspot threshold) for a given
//! target mode, and exposes a `FfiCacheStats` struct that the runtime
//! populates during execution.
//! Corresponds to Phase 3 §5.3 and Phase 5 §5.5 in the full Aura-ification plan.

use crate::ffi::aot::AotFfiTarget;

/// FFI cache configuration.
///
/// Controls how the FFI call cache is initialised at build time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiCacheConfig {
    /// Pre-load known function addresses at start-up.
    ///
    /// Only meaningful for VM mode (the runtime resolves addresses eagerly
    /// and stores them in the cache). JIT/AOT resolve at link time and do
    /// not need a preload step.
    pub preload_on_start: bool,
    /// Enable the inline cache for repeated calls at the same call site.
    ///
    /// Inline caches eliminate the hash-map lookup for hot call sites by
    /// storing the resolved address in a side-table keyed by call site.
    pub inline_cache: bool,
    /// Number of calls before a call site is considered a "hotspot".
    ///
    /// Hotspots are candidates for inline-cache promotion and, in AOT
    /// mode, for cross-library inlining. A threshold of `0` disables
    /// hotspot detection entirely.
    pub hotspot_threshold: u64,
}

impl Default for FfiCacheConfig {
    fn default() -> Self {
        Self {
            preload_on_start: true,
            inline_cache: true,
            hotspot_threshold: 1000,
        }
    }
}

/// Generate a cache configuration for a given FFI AOT target mode.
///
/// Returns sensible defaults per mode:
///
/// | Mode | `preload_on_start` | `inline_cache` | `hotspot_threshold` |
/// |------|--------------------|----------------|---------------------|
/// | VM   | `true`             | `true`         | `1000`              |
/// | JIT  | `false`            | `true`         | `100`               |
/// | AOT  | `false`            | `false`        | `0`                 |
///
/// - **VM**: eagerly preloads library addresses and uses inline cache
///   aggressively (hotspot threshold is high because the VM is the slowest
///   mode and can afford to wait before promoting).
/// - **JIT**: PLT handles resolution lazily; inline cache is still
///   enabled for repeated calls, with a lower hotspot threshold because
///   the JIT is faster and can promote earlier.
/// - **AOT**: all resolution happens at link time — no preload, no
///   inline cache, no hotspot tracking needed.
pub fn generate_cache_config(mode: AotFfiTarget) -> FfiCacheConfig {
    match mode {
        AotFfiTarget::Vm => FfiCacheConfig {
            preload_on_start: true,
            inline_cache: true,
            hotspot_threshold: 1000,
        },
        AotFfiTarget::Jit => FfiCacheConfig {
            preload_on_start: false,
            inline_cache: true,
            hotspot_threshold: 100,
        },
        AotFfiTarget::Aot => FfiCacheConfig {
            preload_on_start: false,
            inline_cache: false,
            hotspot_threshold: 0,
        },
    }
}

/// FFI cache statistics.
///
/// Tracks aggregate cache metrics during execution. The runtime populates
/// these counters; the build system may read them to tune future cache
/// configurations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FfiCacheStats {
    /// Total number of FFI calls recorded.
    pub total_calls: u64,
    /// Total number of cache hits (address resolved from cache).
    pub total_hits: u64,
    /// Total number of cache misses (address not found in cache).
    pub total_misses: u64,
}

impl FfiCacheStats {
    /// Compute the cache hit rate as a fraction in `[0.0, 1.0]`.
    ///
    /// Returns `0.0` when no lookups have occurred.
    pub fn hit_rate(&self) -> f64 {
        let total = self.total_hits + self.total_misses;
        if total == 0 { 0.0 } else { self.total_hits as f64 / total as f64 }
    }

    /// Check whether the cache is empty (no lookups recorded).
    pub fn is_empty(&self) -> bool {
        self.total_hits == 0 && self.total_misses == 0
    }

    /// Record a cache hit, incrementing both the hit counter and the
    /// total call counter.
    pub fn record_hit(&mut self) {
        self.total_hits += 1;
        self.total_calls += 1;
    }

    /// Record a cache miss, incrementing both the miss counter and the
    /// total call counter.
    pub fn record_miss(&mut self) {
        self.total_misses += 1;
        self.total_calls += 1;
    }

    /// Record a call that did not involve a cache lookup
    /// (e.g. a direct call with a pre-resolved address).
    pub fn record_call(&mut self) {
        self.total_calls += 1;
    }

    /// Merge another `FfiCacheStats` into this one, summing all counters.
    pub fn merge(&mut self, other: &FfiCacheStats) {
        self.total_calls += other.total_calls;
        self.total_hits += other.total_hits;
        self.total_misses += other.total_misses;
    }

    /// Reset all counters to zero.
    pub fn reset(&mut self) {
        self.total_calls = 0;
        self.total_hits = 0;
        self.total_misses = 0;
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_cache_config_default() {
        let config = FfiCacheConfig::default();
        assert!(config.preload_on_start);
        assert!(config.inline_cache);
        assert_eq!(config.hotspot_threshold, 1000);
    }

    #[test]
    fn test_generate_cache_config_vm() {
        let config = generate_cache_config(AotFfiTarget::Vm);
        assert!(config.preload_on_start);
        assert!(config.inline_cache);
        assert_eq!(config.hotspot_threshold, 1000);
    }

    #[test]
    fn test_generate_cache_config_jit() {
        let config = generate_cache_config(AotFfiTarget::Jit);
        assert!(!config.preload_on_start);
        assert!(config.inline_cache);
        assert_eq!(config.hotspot_threshold, 100);
    }

    #[test]
    fn test_generate_cache_config_aot() {
        let config = generate_cache_config(AotFfiTarget::Aot);
        assert!(!config.preload_on_start);
        assert!(!config.inline_cache);
        assert_eq!(config.hotspot_threshold, 0);
    }

    #[test]
    fn test_generate_cache_config_differs_per_mode() {
        let vm = generate_cache_config(AotFfiTarget::Vm);
        let jit = generate_cache_config(AotFfiTarget::Jit);
        let aot = generate_cache_config(AotFfiTarget::Aot);

        // All three modes produce distinct configurations.
        assert_ne!(vm, jit);
        assert_ne!(vm, aot);
        assert_ne!(jit, aot);
    }

    #[test]
    fn test_ffi_cache_stats_default() {
        let stats = FfiCacheStats::default();
        assert_eq!(stats.total_calls, 0);
        assert_eq!(stats.total_hits, 0);
        assert_eq!(stats.total_misses, 0);
        assert!(stats.is_empty());
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_ffi_cache_stats_record_hit() {
        let mut stats = FfiCacheStats::default();
        stats.record_hit();
        stats.record_hit();
        assert_eq!(stats.total_hits, 2);
        assert_eq!(stats.total_calls, 2);
        assert_eq!(stats.total_misses, 0);
    }

    #[test]
    fn test_ffi_cache_stats_record_miss() {
        let mut stats = FfiCacheStats::default();
        stats.record_miss();
        stats.record_miss();
        stats.record_miss();
        assert_eq!(stats.total_misses, 3);
        assert_eq!(stats.total_calls, 3);
        assert_eq!(stats.total_hits, 0);
    }

    #[test]
    fn test_ffi_cache_stats_record_call() {
        let mut stats = FfiCacheStats::default();
        stats.record_call();
        stats.record_call();
        assert_eq!(stats.total_calls, 2);
        assert_eq!(stats.total_hits, 0);
        assert_eq!(stats.total_misses, 0);
    }

    #[test]
    fn test_ffi_cache_stats_hit_rate_all_hits() {
        let mut stats = FfiCacheStats::default();
        stats.record_hit();
        stats.record_hit();
        stats.record_hit();
        assert_eq!(stats.hit_rate(), 1.0);
    }

    #[test]
    fn test_ffi_cache_stats_hit_rate_all_misses() {
        let mut stats = FfiCacheStats::default();
        stats.record_miss();
        stats.record_miss();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_ffi_cache_stats_hit_rate_mixed() {
        let mut stats = FfiCacheStats::default();
        stats.record_hit();
        stats.record_hit();
        stats.record_miss();
        stats.record_miss();
        // 2 hits out of 4 lookups = 0.5
        assert!((stats.hit_rate() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ffi_cache_stats_is_empty() {
        let mut stats = FfiCacheStats::default();
        assert!(stats.is_empty());

        stats.record_hit();
        assert!(!stats.is_empty());

        stats.reset();
        assert!(stats.is_empty());
    }

    #[test]
    fn test_ffi_cache_stats_merge() {
        let mut a = FfiCacheStats {
            total_calls: 10,
            total_hits: 7,
            total_misses: 3,
        };
        let b = FfiCacheStats {
            total_calls: 5,
            total_hits: 2,
            total_misses: 3,
        };
        a.merge(&b);
        assert_eq!(a.total_calls, 15);
        assert_eq!(a.total_hits, 9);
        assert_eq!(a.total_misses, 6);
    }

    #[test]
    fn test_ffi_cache_stats_reset() {
        let mut stats = FfiCacheStats {
            total_calls: 100,
            total_hits: 50,
            total_misses: 50,
        };
        stats.reset();
        assert_eq!(stats.total_calls, 0);
        assert_eq!(stats.total_hits, 0);
        assert_eq!(stats.total_misses, 0);
    }

    #[test]
    fn test_ffi_cache_stats_clone() {
        let stats = FfiCacheStats {
            total_calls: 10,
            total_hits: 7,
            total_misses: 3,
        };
        let cloned = stats.clone();
        assert_eq!(stats, cloned);
    }

    #[test]
    fn test_ffi_cache_config_clone_and_equality() {
        let config = FfiCacheConfig {
            preload_on_start: false,
            inline_cache: true,
            hotspot_threshold: 42,
        };
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }
}
