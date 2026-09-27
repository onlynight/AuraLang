//! Phase 3: Multi-module VM support.
//!
//! Loads multiple .auc bytecode files and provides cross-module symbol
//! resolution and function calls.
//!
//! Corresponds to Phase 3 §5.3 in the full Aura-ification plan.

use std::collections::HashMap;
use std::path::Path;

use crate::codegen::opcode::{BytecodeFunction, BytecodeModule, ExportSymbol, SymbolKind};

/// A multi-module VM runtime.
pub struct MultiModuleVm {
    /// All loaded modules, keyed by module name
    modules: HashMap<String, BytecodeModule>,
    /// Entry module name
    entry_module: String,
    /// Cross-module symbol table: symbol name -> (module_name, export_index)
    symbol_table: HashMap<String, (String, usize)>,
}

/// Result of loading a multi-module program.
#[derive(Debug, Clone)]
pub struct MultiModuleResult {
    /// Number of modules loaded
    pub modules_loaded: usize,
    /// Number of symbols resolved across modules
    pub symbols_resolved: usize,
    /// Module names in load order
    pub module_names: Vec<String>,
}

impl MultiModuleVm {
    /// Create a new multi-module VM.
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            entry_module: String::new(),
            symbol_table: HashMap::new(),
        }
    }

    /// Load a module and register its exports.
    pub fn load_module(&mut self, name: &str, module: BytecodeModule) {
        // Register all exports in the symbol table
        for (i, export) in module.exports.iter().enumerate() {
            self.symbol_table.insert(export.name.clone(), (name.to_string(), i));
        }
        self.modules.insert(name.to_string(), module);
    }

    /// Set the entry module.
    pub fn set_entry(&mut self, module_name: &str) {
        self.entry_module = module_name.to_string();
    }

    /// Get the entry module.
    pub fn entry_module(&self) -> Option<&BytecodeModule> {
        self.modules.get(&self.entry_module)
    }

    /// Get a module by name.
    pub fn get_module(&self, name: &str) -> Option<&BytecodeModule> {
        self.modules.get(name)
    }

    /// Get a module by name (mutable).
    pub fn get_module_mut(&mut self, name: &str) -> Option<&mut BytecodeModule> {
        self.modules.get_mut(name)
    }

    /// List all loaded module names.
    pub fn module_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.modules.keys().cloned().collect();
        names.sort();
        names
    }

    /// Number of loaded modules.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Whether the VM is empty.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Resolve a symbol across all loaded modules.
    pub fn resolve_symbol(&self, name: &str) -> Option<(&BytecodeModule, &ExportSymbol)> {
        let (module_name, export_idx) = self.symbol_table.get(name)?;
        let module = self.modules.get(module_name)?;
        let export = module.exports.get(*export_idx)?;
        Some((module, export))
    }

    /// Check if a symbol exists in any loaded module.
    pub fn has_symbol(&self, name: &str) -> bool {
        self.symbol_table.contains_key(name)
    }

    /// Get the total number of symbols across all modules.
    pub fn symbol_count(&self) -> usize {
        self.symbol_table.len()
    }
}

impl Default for MultiModuleVm {
    fn default() -> Self {
        Self::new()
    }
}

/// Load multiple .auc files from a directory into a MultiModuleVm.
pub fn load_modules_from_dir(
    vm: &mut MultiModuleVm,
    dir: &Path,
    entry_name: &str,
) -> Result<MultiModuleResult, String> {
    use crate::codegen::read_auc;

    if !dir.exists() {
        return Err(format!("directory does not exist: {}", dir.display()));
    }

    let mut module_names = Vec::new();
    let mut modules_loaded = 0;

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read directory {}: {}", dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "auc").unwrap_or(false) {
            let module_name =
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();

            match read_auc(&path.to_string_lossy()) {
                Ok(module) => {
                    vm.load_module(&module_name, module);
                    module_names.push(module_name);
                    modules_loaded += 1;
                }
                Err(e) => {
                    eprintln!("failed to load module {}: {}", module_name, e);
                }
            }
        }
    }

    // Set entry module
    if vm.modules.contains_key(entry_name) {
        vm.set_entry(entry_name);
    }

    Ok(MultiModuleResult {
        modules_loaded,
        symbols_resolved: vm.symbol_count(),
        module_names,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_module_vm_new() {
        let vm = MultiModuleVm::new();
        assert!(vm.is_empty());
        assert_eq!(vm.len(), 0);
    }

    #[test]
    fn test_multi_module_vm_load_module() {
        let mut vm = MultiModuleVm::new();
        let mut module = BytecodeModule::default();
        module.exports.push(ExportSymbol {
            name: "main".to_string(),
            kind: SymbolKind::Function,
            sig_id: "sig-main".to_string(),
            func_idx: Some(0),
            type_table_idx: None,
            const_idx: None,
        });
        vm.load_module("app", module);
        assert_eq!(vm.len(), 1);
        assert!(vm.has_symbol("main"));
    }

    #[test]
    fn test_multi_module_vm_resolve_symbol() {
        let mut vm = MultiModuleVm::new();
        let mut module = BytecodeModule::default();
        module.exports.push(ExportSymbol {
            name: "add".to_string(),
            kind: SymbolKind::Function,
            sig_id: "sig-add".to_string(),
            func_idx: Some(0),
            type_table_idx: None,
            const_idx: None,
        });
        vm.load_module("math", module);

        let result = vm.resolve_symbol("add");
        assert!(result.is_some());
        let (m, export) = result.unwrap();
        assert_eq!(export.name, "add");
    }

    #[test]
    fn test_multi_module_vm_entry_module() {
        let mut vm = MultiModuleVm::new();
        let mut module = BytecodeModule::default();
        vm.load_module("app", module.clone());
        vm.set_entry("app");
        assert!(vm.entry_module().is_some());
    }

    #[test]
    fn test_multi_module_vm_cross_module() {
        let mut vm = MultiModuleVm::new();
        let mut math = BytecodeModule::default();
        math.exports.push(ExportSymbol {
            name: "abs".to_string(),
            kind: SymbolKind::Function,
            sig_id: "sig-abs".to_string(),
            func_idx: Some(0),
            type_table_idx: None,
            const_idx: None,
        });
        vm.load_module("math", math);

        let mut app = BytecodeModule::default();
        app.exports.push(ExportSymbol {
            name: "main".to_string(),
            kind: SymbolKind::Function,
            sig_id: "sig-main".to_string(),
            func_idx: Some(0),
            type_table_idx: None,
            const_idx: None,
        });
        vm.load_module("app", app);

        assert_eq!(vm.len(), 2);
        assert!(vm.has_symbol("abs"));
        assert!(vm.has_symbol("main"));
        assert_eq!(vm.symbol_count(), 2);
    }

    #[test]
    fn test_load_modules_from_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mut vm = MultiModuleVm::new();
        let result = load_modules_from_dir(&mut vm, tmp.path(), "app").unwrap();
        assert_eq!(result.modules_loaded, 0);
    }

    #[test]
    fn test_load_modules_from_nonexistent_dir() {
        let mut vm = MultiModuleVm::new();
        let result = load_modules_from_dir(&mut vm, Path::new("/nonexistent"), "app");
        assert!(result.is_err());
    }
}
