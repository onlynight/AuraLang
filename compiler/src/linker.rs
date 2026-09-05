////!Phase 3: 链接器（设计方案 §8）
//!!
////!编译期链接器负责：
////!1. 符号解析：将模块内符号引用解析为具体定义
//! 2. 冲突检测：检测跨模块符号名冲突
//!3. 依赖解析：递归解析依赖树，确保所有模块可加载
//!4. 重命名：对冲突符号生成唯一别名

use std::collections::HashMap;

use crate::signature::{ImportSig, ModuleSig, SymbolKind};

/// 链接错误
#[derive(Debug, Clone)]
pub enum LinkError {
    /// 符号未找到
    SymbolNotFound(String),
    /// 符号冲突
    SymbolConflict {
        symbol: String,
        modules: Vec<String>,
    },
    /// 模块未找到
    ModuleNotFound(String),
    /// 循环依赖
    CircularDependency(Vec<String>),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::SymbolNotFound(s) => write!(f, "符号未找到: {}", s),
            LinkError::SymbolConflict { symbol, modules } => write!(
                f,
                "符号冲突: {} 在多个模块中定义: {}",
                symbol,
                modules.join(", ")
            ),
            LinkError::ModuleNotFound(m) => write!(f, "模块未找到: {}", m),
            LinkError::CircularDependency(mods) => write!(
                f,
                "循环依赖: {}",
                mods.join(" -> ")
            ),
        }
    }
}

/// 解析后的符号
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// 原始符号名
    pub original_name: String,
    /// 解析后的唯一名称
    pub resolved_name: String,
    /// 所属模块
    pub module: String,
    /// 符号类型
    pub kind: SymbolKind,
    /// 是否被重命名
    pub renamed: bool,
}

/// 链接结果
#[derive(Debug, Default)]
pub struct LinkResult {
    /// 解析后的符号映射：原始名 -> ResolvedSymbol
    pub symbols: HashMap<String, ResolvedSymbol>,
    /// 冲突重命名映射：原始名 -> 唯一名
    pub renames: HashMap<String, String>,
    /// 检测到的冲突
    pub conflicts: Vec<LinkError>,
}

impl LinkResult {
    /// 是否链接成功（无冲突）
    pub fn is_ok(&self) -> bool {
        self.conflicts.is_empty()
    }

    /// 获取解析后的符号名
    pub fn resolve<'a>(&'a self, name: &'a str) -> &'a str {
        self.renames.get(name).map(|s| s.as_str()).unwrap_or(name)
    }
}

/// 链接器 — 跨模块符号解析与冲突检测
#[derive(Debug, Default)]
pub struct Linker {
    /// 已加载的模块签名
    modules: HashMap<String, ModuleSig>,
}

impl Linker {
    /// 创建空链接器
    pub fn new() -> Self {
        Self::default()
    }

    /// 加载模块签名
    pub fn load_module(&mut self, sig: &ModuleSig) -> Result<(), LinkError> {
        self.modules.insert(
            sig.module_name.clone(),
            sig.clone(),
        );
        Ok(())
    }

    /// 加载模块签名（从文件）
    pub fn load_module_from_file(&mut self, path: &str) -> Result<(), LinkError> {
        let sig = crate::signature::read_sig(path)
            .map_err(|e| LinkError::SymbolNotFound(e.to_string()))?;
        self.load_module(&sig)
    }

    /// 解析导入声明
    pub fn resolve_imports(
        &self,
        import: &ImportSig,
    ) -> Result<Vec<ResolvedSymbol>, LinkError> {
        let module_sig = self
            .modules
            .get(&import.module)
            .ok_or_else(|| LinkError::ModuleNotFound(import.module.clone()))?;

        let mut resolved = Vec::new();
        for sym in &import.symbols {
            let sig_sym = self.find_symbol(module_sig, &sym.name, sym.kind);
            if let Some(symbol) = sig_sym {
                resolved.push(ResolvedSymbol {
                    original_name: sym.name.clone(),
                    resolved_name: sym.name.clone(),
                    module: import.module.clone(),
                    kind: sym.kind,
                    renamed: false,
                });
            } else {
                return Err(LinkError::SymbolNotFound(format!(
                    "{}::{}",
                    import.module, sym.name
                )));
            }
        }

        Ok(resolved)
    }

    /// 链接所有已加载模块的导入
    pub fn link_all(&self) -> LinkResult {
        let mut result = LinkResult::default();
        let mut all_symbols: HashMap<String, Vec<String>> = HashMap::new();

        // 收集所有模块的公开符号
        for (mod_name, sig) in &self.modules {
            for func in &sig.functions {
                if func.is_public {
                    all_symbols
                        .entry(func.name.clone())
                        .or_default()
                        .push(mod_name.clone());
                }
            }
            for type_def in &sig.types {
                if type_def.is_public {
                    all_symbols
                        .entry(type_def.name.clone())
                        .or_default()
                        .push(mod_name.clone());
                }
            }
            for const_sig in &sig.constants {
                if const_sig.is_public {
                    all_symbols
                        .entry(const_sig.name.clone())
                        .or_default()
                        .push(mod_name.clone());
                }
            }
        }

        // 检测冲突
        for (symbol, modules) in &all_symbols {
            if modules.len() > 1 {
                // 符号冲突：生成唯一别名
                let conflict = LinkError::SymbolConflict {
                    symbol: symbol.clone(),
                    modules: modules.clone(),
                };
                result.conflicts.push(conflict);

                // 为每个模块的符号生成唯一名
                for mod_name in modules {
                    let unique_name = format!("{}_{}", mod_name, symbol);
                    result.renames.insert(
                        format!("{}_{}", mod_name, symbol),
                        unique_name.clone(),
                    );
                    result.symbols.insert(
                        format!("{}_{}", mod_name, symbol),
                        ResolvedSymbol {
                            original_name: symbol.clone(),
                            resolved_name: unique_name,
                            module: mod_name.clone(),
                            kind: SymbolKind::Function,
                            renamed: true,
                        },
                    );
                }
            } else {
                // 无冲突：直接使用原始名
                let mod_name = &modules[0];
                result.symbols.insert(
                    symbol.clone(),
                    ResolvedSymbol {
                        original_name: symbol.clone(),
                        resolved_name: symbol.clone(),
                        module: mod_name.clone(),
                        kind: SymbolKind::Function,
                        renamed: false,
                    },
                );
            }
        }

        // 解析每个模块的导入
        for (mod_name, sig) in &self.modules {
            for import in &sig.imports {
                if let Err(e) = self.resolve_imports(import) {
                    if !result.conflicts.iter().any(|c| matches!(
                        c,
                        LinkError::SymbolNotFound(s) if s.contains(mod_name)
                    )) {
                        result.conflicts.push(e);
                    }
                }
            }
        }

        result
    }

    /// 查找模块中的符号
    fn find_symbol(
        &self,
        sig: &ModuleSig,
        name: &str,
        kind: SymbolKind,
    ) -> Option<crate::signature::FuncSig> {
        match kind {
            SymbolKind::Function => sig.find_function(name).cloned(),
            _ => sig.find_function(name).cloned(),
        }
    }

    /// 检测循环依赖
    pub fn detect_circular_dependencies(&self) -> Vec<LinkError> {
        let mut errors = Vec::new();
        let mut visited = HashMap::new();
        let mut stack = Vec::new();

        for mod_name in self.modules.keys() {
            if !!visited.contains_key(mod_name) {
                self.visit_deps(mod_name, &mut visited, &mut stack, &mut errors);
            }
        }

        errors
    }

    /// 递归检查依赖
    fn visit_deps(
        &self,
        mod_name: &str,
        visited: &mut HashMap<String, usize>,
        stack: &mut Vec<String>,
        errors: &mut Vec<LinkError>,
    ) {
        if let Some(depth) = visited.get(mod_name) {
            if *depth > 0 {
                // 在栈中：循环依赖
                let start = stack.iter().position(|s| s == mod_name).unwrap_or(0);
                let mut cycle = stack[start..].to_vec();
                cycle.push(mod_name.to_string());
                errors.push(LinkError::CircularDependency(cycle));
                return;
            }
        }

        visited.insert(mod_name.to_string(), 1);
        stack.push(mod_name.to_string());

        if let Some(sig) = self.modules.get(mod_name) {
            for dep in &sig.dependencies {
                if self.modules.contains_key(dep) {
                    self.visit_deps(dep, visited, stack, errors);
                }
            }
        }

        stack.pop();
        visited.insert(mod_name.to_string(), 0);
    }

    /// 列出所有已加载模块
    pub fn list_modules(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{ConstSig, ConstValue, FuncSig, ModuleSig, TypeSig};

    fn make_module(name: &str, functions: Vec<&str>) -> ModuleSig {
        ModuleSig {
            module_name: name.to_string(),
            module_version: "1.0.0".to_string(),
            uuid: [0u8; 16],
            version: 1,
            functions: functions
                .iter()
                .map(|n| FuncSig {
                    name: n.to_string(),
                    params: vec![],
                    return_type: TypeSig::Int,
                    is_public: true,
                    type_params: vec![],
                })
                .collect(),
            types: vec![],
            constants: vec![],
            imports: vec![],
            dependencies: vec![],
        }
    }

    fn make_module_with_deps(name: &str, deps: Vec<&str>) -> ModuleSig {
        let mut sig = make_module(name, vec![]);
        sig.dependencies = deps.iter().map(|s| s.to_string()).collect();
        sig
    }

    #[test]
    fn test_linker_no_conflict() {
        let mut linker = Linker::new();
        linker
            .load_module(&make_module("math", vec!["add", "sub"]))
            .unwrap();
        linker
            .load_module(&make_module("string", vec!["concat", "trim"]))
            .unwrap();

        let result = linker.link_all();
        assert!(result.is_ok());
        assert_eq!(result.symbols.len(), 4);
    }

    fn test_linker_conflict() {
        let mut linker = Linker::new();
        linker.load_module(&make_module("math", vec!["parse"])).unwrap();
        linker.load_module(&make_module("string", vec!["parse"])).unwrap();

        let result = linker.link_all();
        assert!(!result.is_ok());
        assert_eq!(result.conflicts.len(), 1);
    }

    fn test_linker_missing_symbol() {
        let mut linker = Linker::new();
        linker.load_module(&make_module("app", vec![])).unwrap();

        let import = ImportSig {
            module: "math".to_string(),
            symbols: vec![crate::signature::ImportSymbolSig {
                name: "nonexistent".to_string(),
                kind: SymbolKind::Function,
            }],
            aliases: std::collections::BTreeMap::new(),
        };

        assert!(linker.resolve_imports(&import).is_err());
    }

    fn test_linker_missing_module() {
        let linker = Linker::new();
        let import = ImportSig {
            module: "nonexistent".to_string(),
            symbols: vec![],
            aliases: std::collections::BTreeMap::new(),
        };
        assert!(linker.resolve_imports(&import).is_err());
    }

    fn test_linker_circular_deps() {
        let mut linker = Linker::new();
        linker.load_module(&make_module_with_deps("a", vec!["b"])).unwrap();
        linker.load_module(&make_module_with_deps("b", vec!["c"])).unwrap();
        linker.load_module(&make_module_with_deps("c", vec!["a"])).unwrap();

        let errors = linker.detect_circular_dependencies();
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_linker_resolve_imports() {
        let mut linker = Linker::new();
        linker.load_module(&make_module("math", vec!["add"])).unwrap();

        let import = ImportSig {
            module: "math".to_string(),
            symbols: vec![crate::signature::ImportSymbolSig {
                name: "add".to_string(),
                kind: SymbolKind::Function,
            }],
            aliases: std::collections::BTreeMap::new(),
        };

        let resolved = linker.resolve_imports(&import).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].original_name, "add");
        assert_eq!(resolved[0].module, "math");
    }
}
