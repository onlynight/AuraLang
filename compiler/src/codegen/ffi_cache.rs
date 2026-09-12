//! Phase 5: FFI call cache and inline cache optimization.
//!
//! Caches FFI function addresses and call sites to eliminate lookup overhead.
//! Supports hot-path detection and inline caching for repeated FFI calls.
//!
//! Corresponds to Phase 5 §5.5 in the full Aura-ification plan.

use std::collections::HashMap;

/// Inline cache entry for a single FFI call site.
#[derive(Debug, Clone, Default)]
pub struct InlineCacheEntry {
    /// The function name being called
    pub name: String,
    /// The cached function address
    pub address: Option<usize>,
    /// Number of times this cache entry has been hit
    pub hit_count: u64,
    /// Number of times this cache entry has been missed
    pub miss_count: u64,
    /// Whether the cache is valid (address has been resolved)
    pub valid: bool,
}

/// A single FFI call site with caching.
#[derive(Debug, Clone)]
pub struct FfiCallSite {
    /// Function name
    pub name: String,
    /// Target library (e.g. "libc")
    pub library: Option<String>,
    /// Cached function address
    pub address: Option<usize>,
    /// Number of times called
    pub call_count: u64,
    /// Number of times the address was looked up (cache miss)
    pub lookup_count: u64,
    /// Whether this call site is a hotspot (> threshold calls)
    pub is_hotspot: bool,
    /// Inline cache for fast repeated calls
    pub inline_cache: InlineCacheEntry,
}

impl Default for FfiCallSite {
    fn default() -> Self {
        Self {
            name: String::new(),
            library: None,
            address: None,
            call_count: 0,
            lookup_count: 0,
            is_hotspot: false,
            inline_cache: InlineCacheEntry::default(),
        }
    }
}

/// FFI call cache with inline cache optimization.
pub struct FfiCallCache {
    /// Call sites keyed by function name
    call_sites: HashMap<String, FfiCallSite>,
    /// Pre-loaded function addresses
    addresses: HashMap<String, usize>,
    /// Hotspot threshold
    hotspot_threshold: u64,
    /// Total calls recorded
    total_calls: u64,
    /// Total cache hits
    total_hits: u64,
    /// Total cache misses
    total_misses: u64,
}

impl FfiCallCache {
    /// Create a new FFI call cache with default settings.
    pub fn new() -> Self {
        Self {
            call_sites: HashMap::new(),
            addresses: HashMap::new(),
            hotspot_threshold: 1000,
            total_calls: 0,
            total_hits: 0,
            total_misses: 0,
        }
    }

    /// Create a new FFI call cache with a custom hotspot threshold.
    pub fn with_threshold(threshold: u64) -> Self {
        let mut cache = Self::new();
        cache.hotspot_threshold = threshold;
        cache
    }

    /// Pre-load a function address into the cache.
    pub fn preload(&mut self, name: &str, address: usize) {
        self.addresses.insert(name.to_string(), address);
        let site = self.call_sites.entry(name.to_string()).or_insert_with(|| FfiCallSite {
            name: name.to_string(),
            ..Default::default()
        });
        site.address = Some(address);
        site.inline_cache.address = Some(address);
        site.inline_cache.valid = true;
        site.inline_cache.name = name.to_string();
    }

    /// Look up a function address (with inline cache).
    pub fn lookup(&mut self, name: &str) -> Option<usize> {
        // Try inline cache first
        if let Some(site) = self.call_sites.get_mut(name) {
            if site.inline_cache.valid && site.inline_cache.name == name {
                site.inline_cache.hit_count += 1;
                self.total_hits += 1;
                return site.inline_cache.address;
            }
            // Cache miss
            site.inline_cache.miss_count += 1;
            self.total_misses += 1;
            site.lookup_count += 1;

            // Try to load from addresses map
            if let Some(addr) = self.addresses.get(name).copied() {
                site.address = Some(addr);
                site.inline_cache.address = Some(addr);
                site.inline_cache.valid = true;
                site.inline_cache.name = name.to_string();
                return Some(addr);
            }
        } else {
            // No call site exists - count as miss
            self.total_misses += 1;
        }

        // Not found
        None
    }

    /// Record a call to a function.
    pub fn record_call(&mut self, name: &str) {
        let site = self.call_sites.entry(name.to_string()).or_insert_with(|| FfiCallSite {
            name: name.to_string(),
            ..Default::default()
        });
        site.call_count += 1;
        self.total_calls += 1;

        // Check for hotspot
        if site.call_count >= self.hotspot_threshold && !site.is_hotspot {
            site.is_hotspot = true;
        }
    }

    /// Get call statistics for a function.
    pub fn get_call_count(&self, name: &str) -> u64 {
        self.call_sites.get(name).map(|s| s.call_count).unwrap_or(0)
    }

    /// Check if a function is a hotspot.
    pub fn is_hotspot(&self, name: &str) -> bool {
        self.call_sites.get(name).map(|s| s.is_hotspot).unwrap_or(false)
    }

    /// Get all hotspot functions.
    pub fn hotspots(&self) -> Vec<&str> {
        self.call_sites.values().filter(|s| s.is_hotspot).map(|s| s.name.as_str()).collect()
    }

    /// Get total call count.
    pub fn total_calls(&self) -> u64 {
        self.total_calls
    }

    /// Get cache hit rate.
    pub fn hit_rate(&self) -> f64 {
        if self.total_hits + self.total_misses == 0 {
            0.0
        } else {
            self.total_hits as f64 / (self.total_hits + self.total_misses) as f64
        }
    }

    /// Get the number of pre-loaded functions.
    pub fn preloaded_count(&self) -> usize {
        self.addresses.len()
    }

    /// Get the number of call sites.
    pub fn call_site_count(&self) -> usize {
        self.call_sites.len()
    }

    /// Get a reference to a call site.
    pub fn get_call_site(&self, name: &str) -> Option<&FfiCallSite> {
        self.call_sites.get(name)
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.call_sites.clear();
        self.addresses.clear();
        self.total_calls = 0;
        self.total_hits = 0;
        self.total_misses = 0;
    }
}

impl Default for FfiCallCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_call_cache_preload_and_lookup() {
        let mut cache = FfiCallCache::new();
        cache.preload("fopen", 0x400000);
        cache.preload("fclose", 0x401000);

        assert_eq!(cache.lookup("fopen"), Some(0x400000));
        assert_eq!(cache.lookup("fclose"), Some(0x401000));
        assert_eq!(cache.lookup("unknown"), None);
        assert_eq!(cache.preloaded_count(), 2);
    }

    #[test]
    fn test_ffi_call_cache_record_call() {
        let mut cache = FfiCallCache::new();
        cache.preload("fopen", 0x400000);

        for _ in 0..5 {
            cache.record_call("fopen");
        }

        assert_eq!(cache.get_call_count("fopen"), 5);
        assert_eq!(cache.total_calls(), 5);
    }

    #[test]
    fn test_ffi_call_cache_hotspot_detection() {
        let mut cache = FfiCallCache::with_threshold(10);

        for _ in 0..15 {
            cache.record_call("fopen");
        }

        assert!(cache.is_hotspot("fopen"));
        assert!(!cache.is_hotspot("unknown"));
        let hotspots = cache.hotspots();
        assert!(hotspots.contains(&"fopen"));
    }

    #[test]
    fn test_ffi_call_cache_hit_rate() {
        let mut cache = FfiCallCache::new();
        cache.preload("fopen", 0x400000);

        // Hit (inline cache valid)
        cache.lookup("fopen");
        cache.lookup("fopen");

        // Miss (not preloaded)
        cache.lookup("unknown");

        assert!(cache.hit_rate() > 0.0);
        assert!(cache.hit_rate() < 1.0);
    }

    #[test]
    fn test_ffi_call_cache_clear() {
        let mut cache = FfiCallCache::new();
        cache.preload("fopen", 0x400000);
        cache.record_call("fopen");

        cache.clear();

        assert_eq!(cache.preloaded_count(), 0);
        assert_eq!(cache.total_calls(), 0);
        assert_eq!(cache.get_call_count("fopen"), 0);
    }

    #[test]
    fn test_ffi_call_site_default() {
        let site = FfiCallSite::default();
        assert!(site.name.is_empty());
        assert!(site.address.is_none());
        assert_eq!(site.call_count, 0);
        assert!(!site.is_hotspot);
        assert!(!site.inline_cache.valid);
    }

    #[test]
    fn test_inline_cache_entry() {
        let mut entry = InlineCacheEntry::default();
        entry.name = "fopen".to_string();
        entry.address = Some(0x400000);
        entry.valid = true;
        entry.hit_count = 5;

        assert!(entry.valid);
        assert_eq!(entry.hit_count, 5);
        assert_eq!(entry.miss_count, 0);
    }
}
