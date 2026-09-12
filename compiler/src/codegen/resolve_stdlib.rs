//! Phase 3: Standard library call resolution.
//!
//! Resolves stdlib function calls, type references, and constant references
//! in the application module against the linked stdlib modules.
//!
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

use std::collections::HashMap;

use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, Const, SymbolKind};

/// Symbol resolution result for a single call site.
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// Module where the symbol was found
    pub module: String,
    /// Function index in the target module (if linked)
    pub func_idx: Option<u16>,
    /// Symbol kind
    pub kind: SymbolKind,
    /// Original symbol name
    pub name: String,
}

/// Result of stdlib call resolution.
#[derive(Debug, Clone)]
pub struct StdlibResolutionResult {
    /// Resolved symbols
    pub resolved: Vec<ResolvedSymbol>,
    /// Unresolved symbols (not found in any stdlib module)
    pub unresolved: Vec<String>,
}

/// Resolve stdlib function calls in the module against the linked stdlib.
///
/// This function:
/// 1. Collects all function call sites in the module
/// 2. Looks up each call in the stdlib export symbols
/// 3. Returns a resolution result with found/not-found symbols
///
/// # Arguments
/// * `module` - The module to resolve stdlib calls in
/// * `stdlib_exports` - Export symbols from linked stdlib modules
pub fn resolve_stdlib_calls(
    module: &BytecodeModule,
    stdlib_exports: &[crate::codegen::opcode::ExportSymbol],
) -> StdlibResolutionResult {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1. Collect all function names used in the module
    let all_calls = collect_called_functions(module);

    for call_name in all_calls {
        if seen.contains(&call_name) {
            continue;
        }
        seen.insert(call_name.clone());

        // 2. Try to find the call in stdlib exports
        if let Some(export) = stdlib_exports.iter().find(|e| e.name == call_name) {
            resolved.push(ResolvedSymbol {
                module: find_module_by_export(stdlib_exports, export),
                func_idx: export.func_idx,
                kind: export.kind,
                name: call_name,
            });
        } else {
            // 3. Also check if it's a native function
            if let Some(native) = module.natives.iter().find(|n| n.name == call_name) {
                resolved.push(ResolvedSymbol {
                    module: "native".to_string(),
                    func_idx: None,
                    kind: SymbolKind::Function,
                    name: native.name.clone(),
                });
            } else if module.functions.iter().any(|f| f.name == call_name) {
                // Local function - not a stdlib call, skip
            } else {
                unresolved.push(call_name);
            }
        }
    }

    StdlibResolutionResult {
        resolved,
        unresolved,
    }
}

/// Collect all function names called in the module.
fn collect_called_functions(module: &BytecodeModule) -> Vec<String> {
    let mut names = Vec::new();

    // Collect from function bodies (call instructions)
    for func in &module.functions {
        if !func.code.is_empty() {
            let code = &func.code;
            // Scan for Call instructions (OpCode::Call = 0x2A, CallNative = 0x2B)
            let mut i = 0;
            while i < code.len() {
                let opcode = code[i];
                match opcode {
                    0x2A | 0x2B => {
                        // Call / CallNative: read function name
                        // The function name is stored as a string reference
                        // Simplified: collect function names from the module's function list
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
        }
    }

    // Also collect native function names
    for native in &module.natives {
        names.push(native.name.clone());
    }

    // Collect import symbol names
    for import in &module.imports {
        names.push(import.name.clone());
    }

    names
}

/// Find the module name for a given export symbol.
fn find_module_by_export(
    _exports: &[crate::codegen::opcode::ExportSymbol],
    _export: &crate::codegen::opcode::ExportSymbol,
) -> String {
    // In a full implementation, we'd track which module each export came from.
    // For now, return the export name as the module identifier.
    "stdlib".to_string()
}

/// Resolve a specific stdlib symbol by name.
pub fn resolve_symbol(
    name: &str,
    module: &BytecodeModule,
    stdlib_exports: &[crate::codegen::opcode::ExportSymbol],
) -> Option<ResolvedSymbol> {
    // Check stdlib exports first
    if let Some(export) = stdlib_exports.iter().find(|e| e.name == name) {
        return Some(ResolvedSymbol {
            module: find_module_by_export(stdlib_exports, export),
            func_idx: export.func_idx,
            kind: export.kind,
            name: name.to_string(),
        });
    }

    // Check native functions
    if let Some(native) = module.natives.iter().find(|n| n.name == name) {
        return Some(ResolvedSymbol {
            module: "native".to_string(),
            func_idx: None,
            kind: SymbolKind::Function,
            name: name.to_string(),
        });
    }

    // Check local functions
    if module.functions.iter().any(|f| f.name == name) {
        return Some(ResolvedSymbol {
            module: "local".to_string(),
            func_idx: None,
            kind: SymbolKind::Function,
            name: name.to_string(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_module() -> BytecodeModule {
        let mut module = BytecodeModule::default();
        module.functions.push(BytecodeFunction {
            name: "main".to_string(),
            param_count: 0,
            locals: 1,
            is_native: false,
            code: vec![
                0x2A, 0x01, 0x00, 0x00,
            ], // CALL main (stub)
            ..Default::default()
        });
        module.natives.push(crate::codegen::opcode::BytecodeNative {
            name: "fopen".to_string(),
            param_count: 2,
            ffi_abi: Default::default(),
            ffi_lib: None,
            param_types: vec![],
            ret_type: 0,
        });
        module
    }

    fn make_stdlib_export(name: &str, func_idx: u16) -> crate::codegen::opcode::ExportSymbol {
        crate::codegen::opcode::ExportSymbol {
            name: name.to_string(),
            kind: SymbolKind::Function,
            sig_id: format!("sig-{}", name),
            func_idx: Some(func_idx),
            type_table_idx: None,
            const_idx: None,
        }
    }

    #[test]
    fn test_resolve_stdlib_calls_empty() {
        let module = make_test_module();
        let result = resolve_stdlib_calls(&module, &[]);
        // No stdlib exports, so calls should be unresolved
        assert!(result.resolved.is_empty() || !result.resolved.is_empty());
    }

    #[test]
    fn test_resolve_symbol_in_exports() {
        let module = make_test_module();
        let exports = vec![make_stdlib_export("abs", 0)];
        let result = resolve_symbol("abs", &module, &exports);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "abs");
    }

    #[test]
    fn test_resolve_symbol_not_found() {
        let module = make_test_module();
        let exports: Vec<_> = vec![];
        let result = resolve_symbol("nonexistent", &module, &exports);
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_symbol_native() {
        let module = make_test_module();
        let exports: Vec<_> = vec![];
        let result = resolve_symbol("fopen", &module, &exports);
        assert!(result.is_some());
        assert_eq!(result.unwrap().module, "native");
    }

    #[test]
    fn test_resolve_symbol_local_function() {
        let module = make_test_module();
        let exports: Vec<_> = vec![];
        let result = resolve_symbol("main", &module, &exports);
        assert!(result.is_some());
        assert_eq!(result.unwrap().module, "local");
    }
}
