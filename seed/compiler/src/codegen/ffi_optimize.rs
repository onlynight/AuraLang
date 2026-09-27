//! Phase 5: FFI call optimization.
//!
//! Provides optimization strategies for FFI calls.
//! Corresponds to Phase 5 in the full Aura-ification plan.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct FfiOptimizationReport {
    pub total_declarations: usize,
    pub hotspots: usize,
    pub inlineable: usize,
    pub devirtualizable: usize,
    pub eliminable: usize,
    pub suggestions: Vec<FfiOptimizationSuggestion>,
}

#[derive(Debug, Clone)]
pub struct FfiOptimizationSuggestion {
    pub name: String,
    pub kind: FfiOptimizationKind,
    pub estimated_improvement: f64,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FfiOptimizationKind {
    Inline,
    Devirtualize,
    Eliminate,
    Preload,
    InlineCache,
}

#[derive(Debug, Clone)]
pub struct FfiOptimizerConfig {
    pub enable_inline: bool,
    pub enable_devirtualize: bool,
    pub enable_dce: bool,
    pub hotspot_threshold: u64,
    pub inline_min_calls: u64,
}

impl Default for FfiOptimizerConfig {
    fn default() -> Self {
        Self {
            enable_inline: true,
            enable_devirtualize: true,
            enable_dce: true,
            hotspot_threshold: 100,
            inline_min_calls: 10,
        }
    }
}

pub fn optimize_ffi_calls(
    names: &[String],
    call_counts: &HashMap<String, u64>,
    config: &FfiOptimizerConfig,
) -> FfiOptimizationReport {
    let mut report = FfiOptimizationReport {
        total_declarations: names.len(),
        hotspots: 0,
        inlineable: 0,
        devirtualizable: 0,
        eliminable: 0,
        suggestions: Vec::new(),
    };
    for name in names {
        let calls = call_counts.get(name).copied().unwrap_or(0);
        if calls == 0 && config.enable_dce {
            report.eliminable += 1;
            report.suggestions.push(FfiOptimizationSuggestion {
                name: name.clone(),
                kind: FfiOptimizationKind::Eliminate,
                estimated_improvement: 0.1,
                description: "Unused".to_string(),
            });
            continue;
        }
        if calls >= config.hotspot_threshold {
            report.hotspots += 1;
            report.suggestions.push(FfiOptimizationSuggestion {
                name: name.clone(),
                kind: FfiOptimizationKind::Preload,
                estimated_improvement: 0.5,
                description: "Hotspot preload".to_string(),
            });
        }
        if calls >= config.inline_min_calls && config.enable_inline {
            report.inlineable += 1;
            report.suggestions.push(FfiOptimizationSuggestion {
                name: name.clone(),
                kind: FfiOptimizationKind::Inline,
                estimated_improvement: 0.4,
                description: format!("Called {} times", calls),
            });
        }
        if calls > 0 && config.enable_devirtualize {
            report.devirtualizable += 1;
            report.suggestions.push(FfiOptimizationSuggestion {
                name: name.clone(),
                kind: FfiOptimizationKind::Devirtualize,
                estimated_improvement: 0.2,
                description: "De-virtualize".to_string(),
            });
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_optimize_empty() {
        let report = optimize_ffi_calls(&[], &HashMap::new(), &FfiOptimizerConfig::default());
        assert_eq!(report.total_declarations, 0);
    }
    #[test]
    fn test_optimize_hotspot() {
        let names = vec!["fopen".to_string()];
        let mut counts = HashMap::new();
        counts.insert("fopen".to_string(), 500);
        let report = optimize_ffi_calls(&names, &counts, &FfiOptimizerConfig::default());
        assert_eq!(report.hotspots, 1);
    }
    #[test]
    fn test_optimize_elimination() {
        let names = vec!["unused".to_string()];
        let counts: HashMap<String, u64> = HashMap::new();
        let report = optimize_ffi_calls(&names, &counts, &FfiOptimizerConfig::default());
        assert_eq!(report.eliminable, 1);
    }
}
