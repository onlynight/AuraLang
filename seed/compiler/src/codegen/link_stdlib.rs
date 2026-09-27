//! Phase 3: Standard library symbol linking.
//!
//! Loads .auc bytecode files from the stdlib output directory and links
//! their symbols (functions, types, constants) into the application module.
//!
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

use std::path::Path;

use crate::codegen::opcode::{BytecodeModule, ExportSymbol, SymbolKind};

/// Result of stdlib linking.
#[derive(Debug, Clone)]
pub struct StdlibLinkResult {
    /// Number of modules linked
    pub modules_linked: usize,
    /// Number of symbols resolved
    pub symbols_resolved: usize,
    /// Names of linked modules
    pub module_names: Vec<String>,
}

/// Load stdlib .auc modules and link their exports into the target module.
///
/// This function:
/// 1. Reads all .auc files from `auc_dir`
/// 2. Collects their export symbols
/// 3. Resolves the target module's import symbols against the stdlib exports
/// 4. Inlines stdlib function bodies if needed
///
/// # Arguments
/// * `module` - The application module to link stdlib into
/// * `auc_dir` - Directory containing stdlib .auc files
pub fn link_stdlib_symbols(
    module: &mut BytecodeModule,
    auc_dir: &Path,
) -> Result<StdlibLinkResult, String> {
    use crate::codegen::read_auc;

    let mut stdlib_exports = Vec::new();
    let mut module_names = Vec::new();
    let mut modules_linked = 0;

    // 1. Load all stdlib .auc files
    if !auc_dir.exists() {
        return Ok(StdlibLinkResult {
            modules_linked: 0,
            symbols_resolved: 0,
            module_names: Vec::new(),
        });
    }

    let entries = std::fs::read_dir(auc_dir)
        .map_err(|e| format!("cannot read stdlib dir {}: {}", auc_dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "auc").unwrap_or(false) {
            let module_name =
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();

            match read_auc(&path.to_string_lossy()) {
                Ok(std_module) => {
                    stdlib_exports.extend(std_module.exports.clone());
                    module_names.push(module_name);
                    modules_linked += 1;
                }
                Err(e) => {
                    eprintln!(
                        "  stdlib warn: failed to load stdlib module {}: {}",
                        module_name, e
                    );
                }
            }
        }
    }

    // 2. Resolve import symbols against stdlib exports
    let mut symbols_resolved = 0;
    for import in &mut module.imports {
        if let Some(export) = find_export(&stdlib_exports, import) {
            // Link the import to the export
            if let Some(func_idx) = export.func_idx {
                import.func_idx = Some(func_idx);
            }
            symbols_resolved += 1;
        }
    }

    // 3. Add stdlib dependencies
    for module_name in &module_names {
        if !module.dependencies.iter().any(|d| d.module == *module_name) {
            module.dependencies.push(crate::codegen::opcode::Dependency {
                module: module_name.clone(),
                uuid: [0u8; 16],
                version: "0.1.0".to_string(),
            });
        }
    }

    eprintln!(
        "  stdlib linking: {} modules linked, {} symbols resolved",
        modules_linked, symbols_resolved
    );

    Ok(StdlibLinkResult {
        modules_linked,
        symbols_resolved,
        module_names,
    })
}

/// Find a matching export for an import symbol.
fn find_export<'a>(
    exports: &'a [ExportSymbol],
    import: &crate::codegen::opcode::ImportSymbol,
) -> Option<&'a ExportSymbol> {
    // Match by name and kind
    exports.iter().find(|e| e.name == import.symbol && e.kind == import.kind).or_else(|| {
        // Match by full name
        exports.iter().find(|e| e.name == import.name)
    })
}

/// Load a single stdlib module by name from the .auc directory.
pub fn load_stdlib_module(auc_dir: &Path, module_name: &str) -> Result<BytecodeModule, String> {
    use crate::codegen::read_auc;

    let path = auc_dir.join(format!("{}.auc", module_name));
    if !path.exists() {
        return Err(format!("stdlib module not found: {}", path.display()));
    }

    read_auc(&path.to_string_lossy())
        .map_err(|e| format!("failed to load {}: {}", path.display(), e))
}

/// List all available stdlib modules in the .auc directory.
pub fn list_stdlib_modules(auc_dir: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    if !auc_dir.exists() {
        return modules;
    }

    let entries = match std::fs::read_dir(auc_dir) {
        Ok(e) => e,
        Err(_) => return modules,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "auc").unwrap_or(false) {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
            modules.push(name);
        }
    }

    modules.sort();
    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_link_stdlib_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut module = BytecodeModule::default();

        let result = link_stdlib_symbols(&mut module, tmp.path()).unwrap();
        assert_eq!(result.modules_linked, 0);
        assert_eq!(result.symbols_resolved, 0);
    }

    #[test]
    fn test_link_stdlib_nonexistent_dir() {
        let mut module = BytecodeModule::default();
        let result = link_stdlib_symbols(&mut module, Path::new("/nonexistent/path")).unwrap();
        assert_eq!(result.modules_linked, 0);
    }

    #[test]
    fn test_list_stdlib_modules_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let modules = list_stdlib_modules(tmp.path());
        assert!(modules.is_empty());
    }

    #[test]
    fn test_list_stdlib_modules_with_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Create fake .auc files
        std::fs::write(tmp.path().join("Math.auc"), b"fake").unwrap();
        std::fs::write(tmp.path().join("String.auc"), b"fake").unwrap();
        std::fs::write(tmp.path().join("readme.txt"), b"not auc").unwrap();

        let modules = list_stdlib_modules(tmp.path());
        assert_eq!(modules.len(), 2);
        assert!(modules.contains(&"Math".to_string()));
        assert!(modules.contains(&"String".to_string()));
    }

    #[test]
    fn test_load_stdlib_module_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let result = load_stdlib_module(tmp.path(), "Nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }
}
