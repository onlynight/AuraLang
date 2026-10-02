// ================================================================
// 【冻结基线】Rust VM 双实现常驻（VM-PA-00 v3.2 / D6 / P4.3）
// [FROZEN BASELINE] Rust VM dual-impl co-resident (D6/P4.3): bug-fix only, no new capabilities, never reference the Aura VM.
// 本文件属于 Rust VM（只读冻结的兼容基线、第二实现）：
//   - 禁止新增能力，仅允许修 bug 与安全修补；
//   - 不得引用 Aura VM 实现（aura/compiler/aura/lang/compiler/vm/）；
//   - 与 Aura VM 的唯一交集是 .auc 二进制格式（Rust 编译器产出，两侧各自执行）。
// 依据：docs/vm_pure_aura/00-VM纯Aura化技术方案与达成路径.md 三-阶段P4 / 决策D6。
// ================================================================
//! Phase 3: VM FFI call cache.
//!
//! Caches FFI function addresses to eliminate lookup overhead on repeated calls.
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

use std::collections::HashMap;

/// FFI call site statistics.
#[derive(Debug, Clone, Default)]
pub struct FfiCallSite {
    /// Function name
    pub name: String,
    /// Function address (pre-loaded)
    pub address: Option<usize>,
    /// Number of times called
    pub call_count: u64,
    /// Last call timestamp (nanoseconds)
    pub last_called_at: u64,
}

/// FFI call cache for the VM.
pub struct FfiCache {
    /// Pre-loaded function addresses
    addresses: HashMap<String, usize>,
    /// Call site statistics
    call_sites: HashMap<String, FfiCallSite>,
}

impl FfiCache {
    /// Create a new empty FFI cache.
    pub fn new() -> Self {
        Self {
            addresses: HashMap::new(),
            call_sites: HashMap::new(),
        }
    }

    /// Pre-load a function address into the cache.
    pub fn preload_function(&mut self, name: &str, address: usize) {
        self.addresses.insert(name.to_string(), address);
        let entry = self.call_sites.entry(name.to_string()).or_insert_with(|| FfiCallSite {
            name: name.to_string(),
            ..Default::default()
        });
        entry.address = Some(address);
    }

    /// Get a pre-loaded function address.
    pub fn get_address(&self, name: &str) -> Option<usize> {
        self.addresses.get(name).copied()
    }

    /// Check if a function has been pre-loaded.
    pub fn is_preloaded(&self, name: &str) -> bool {
        self.addresses.contains_key(name)
    }

    /// Record a call to a cached function.
    pub fn record_call(&mut self, name: &str) {
        if let Some(site) = self.call_sites.get_mut(name) {
            site.call_count += 1;
        }
    }

    /// Get call statistics for a function.
    pub fn get_call_count(&self, name: &str) -> u64 {
        self.call_sites.get(name).map(|s| s.call_count).unwrap_or(0)
    }

    /// Get all call site statistics.
    pub fn call_sites(&self) -> &HashMap<String, FfiCallSite> {
        &self.call_sites
    }

    /// Get the number of pre-loaded functions.
    pub fn preloaded_count(&self) -> usize {
        self.addresses.len()
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.addresses.clear();
        self.call_sites.clear();
    }
}

impl Default for FfiCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_cache_new() {
        let cache = FfiCache::new();
        assert_eq!(cache.preloaded_count(), 0);
    }

    #[test]
    fn test_ffi_cache_preload() {
        let mut cache = FfiCache::new();
        cache.preload_function("fopen", 0x400000);
        assert_eq!(cache.get_address("fopen"), Some(0x400000));
        assert!(cache.is_preloaded("fopen"));
        assert_eq!(cache.preloaded_count(), 1);
    }

    #[test]
    fn test_ffi_cache_record_call() {
        let mut cache = FfiCache::new();
        cache.preload_function("fopen", 0x400000);
        cache.record_call("fopen");
        cache.record_call("fopen");
        cache.record_call("fopen");
        assert_eq!(cache.get_call_count("fopen"), 3);
    }

    #[test]
    fn test_ffi_cache_clear() {
        let mut cache = FfiCache::new();
        cache.preload_function("fopen", 0x400000);
        cache.clear();
        assert_eq!(cache.preloaded_count(), 0);
        assert!(!cache.is_preloaded("fopen"));
    }

    #[test]
    fn test_ffi_cache_multiple_functions() {
        let mut cache = FfiCache::new();
        cache.preload_function("fopen", 0x400000);
        cache.preload_function("fclose", 0x401000);
        cache.preload_function("malloc", 0x402000);
        assert_eq!(cache.preloaded_count(), 3);
        assert_eq!(cache.get_address("fclose"), Some(0x401000));
    }

    #[test]
    fn test_ffi_cache_call_sites() {
        let mut cache = FfiCache::new();
        cache.preload_function("fopen", 0x400000);
        cache.record_call("fopen");
        let sites = cache.call_sites();
        assert!(sites.contains_key("fopen"));
        assert_eq!(sites.get("fopen").unwrap().call_count, 1);
    }
}
