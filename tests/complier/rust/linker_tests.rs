//! Linker unit tests — symbol resolution, conflict detection, circular dependency detection

use compiler::linker::*;
use compiler::signature::{
    ConstSig, ConstValue, FuncSig, ImportSig, ImportSymbolSig, ModuleSig, SymbolKind, TypeSig,
};
use std::collections::BTreeMap;

// ─── Helper builders ────────────────────────────────────────────────────────

fn make_module(name: &str, functions: &[&str]) -> ModuleSig {
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

fn make_module_with_types(name: &str, functions: &[&str], types: &[&str]) -> ModuleSig {
    let mut sig = make_module(name, functions);
    sig.types = types
        .iter()
        .map(|n| compiler::signature::TypeDefSig {
            name: n.to_string(),
            kind: compiler::signature::TypeDefKind::Struct,
            type_params: vec![],
            fields: vec![],
            methods: vec![],
            variants: vec![],
            super_types: vec![],
            is_public: true,
        })
        .collect();
    sig
}

fn make_module_with_deps(name: &str, deps: &[&str]) -> ModuleSig {
    let mut sig = make_module(name, &[]);
    sig.dependencies = deps.iter().map(|s| s.to_string()).collect();
    sig
}

fn make_module_with_imports(name: &str, imports: Vec<ImportSig>) -> ModuleSig {
    let mut sig = make_module(name, &[]);
    sig.imports = imports;
    sig
}

// ─── Linker: loading modules ────────────────────────────────────────────────

#[test]
fn test_linker_new_empty() {
    let linker = Linker::new();
    assert_eq!(linker.list_modules().len(), 0);
}

#[test]
fn test_linker_load_module() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub",
            ],
        ))
        .unwrap();
    assert_eq!(linker.list_modules().len(), 1);
    assert!(linker.list_modules().contains(&"math"));
}

#[test]
fn test_linker_load_multiple_modules() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub",
            ],
        ))
        .unwrap();
    linker
        .load_module(&make_module(
            "string",
            &[
                "concat", "trim",
            ],
        ))
        .unwrap();
    linker
        .load_module(&make_module(
            "io",
            &[
                "print", "read",
            ],
        ))
        .unwrap();
    assert_eq!(linker.list_modules().len(), 3);
}

#[test]
fn test_linker_load_module_from_file_not_found() {
    let mut linker = Linker::new();
    let result = linker.load_module_from_file("/nonexistent/path.sig");
    assert!(result.is_err());
}

// ─── Linker: link_all (no conflicts) ────────────────────────────────────────

#[test]
fn test_link_all_no_conflict() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub",
            ],
        ))
        .unwrap();
    linker
        .load_module(&make_module(
            "string",
            &[
                "concat", "trim",
            ],
        ))
        .unwrap();

    let result = linker.link_all();
    assert!(result.is_ok());
    assert_eq!(result.symbols.len(), 4);
    assert!(result.conflicts.is_empty());
}

#[test]
fn test_link_all_resolves_symbols() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub",
            ],
        ))
        .unwrap();

    let result = linker.link_all();
    assert!(result.is_ok());

    let add = result.symbols.get("add").unwrap();
    assert_eq!(add.original_name, "add");
    assert_eq!(add.resolved_name, "add");
    assert_eq!(add.module, "math");
    assert!(!add.renamed);
}

// ─── Linker: conflict detection ─────────────────────────────────────────────

#[test]
fn test_link_all_conflict() {
    let mut linker = Linker::new();
    linker.load_module(&make_module("math", &["parse"])).unwrap();
    linker.load_module(&make_module("string", &["parse"])).unwrap();

    let result = linker.link_all();
    assert!(!result.is_ok());
    assert_eq!(result.conflicts.len(), 1);

    match &result.conflicts[0] {
        LinkError::SymbolConflict {
            symbol,
            modules,
        } => {
            assert_eq!(symbol, "parse");
            assert_eq!(modules.len(), 2);
        }
        _ => panic!("expected SymbolConflict"),
    }
}

#[test]
fn test_link_all_conflict_generates_renames() {
    let mut linker = Linker::new();
    linker.load_module(&make_module("math", &["parse"])).unwrap();
    linker.load_module(&make_module("string", &["parse"])).unwrap();

    let result = linker.link_all();
    // Both should be renamed
    let math_parse = result.symbols.get("math_parse").unwrap();
    assert!(math_parse.renamed);
    assert_eq!(math_parse.resolved_name, "math_parse");

    let string_parse = result.symbols.get("string_parse").unwrap();
    assert!(string_parse.renamed);
    assert_eq!(string_parse.resolved_name, "string_parse");
}

#[test]
fn test_link_all_type_conflict() {
    let mut linker = Linker::new();
    linker.load_module(&make_module_with_types("lib1", &[], &["Point"])).unwrap();
    linker.load_module(&make_module_with_types("lib2", &[], &["Point"])).unwrap();

    let result = linker.link_all();
    assert!(!result.is_ok());
    assert_eq!(result.conflicts.len(), 1);
}

// ─── Linker: import resolution ──────────────────────────────────────────────

#[test]
fn test_resolve_imports_basic() {
    let mut linker = Linker::new();
    linker.load_module(&make_module("math", &["add"])).unwrap();

    let import = ImportSig {
        module: "math".to_string(),
        symbols: vec![
            ImportSymbolSig {
                name: "add".to_string(),
                kind: SymbolKind::Function,
            },
        ],
        aliases: BTreeMap::new(),
    };

    let resolved = linker.resolve_imports(&import).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].original_name, "add");
    assert_eq!(resolved[0].module, "math");
    assert!(!resolved[0].renamed);
}

#[test]
fn test_resolve_imports_missing_symbol() {
    let mut linker = Linker::new();
    linker.load_module(&make_module("app", &[])).unwrap();

    let import = ImportSig {
        module: "math".to_string(),
        symbols: vec![
            ImportSymbolSig {
                name: "nonexistent".to_string(),
                kind: SymbolKind::Function,
            },
        ],
        aliases: BTreeMap::new(),
    };

    // Module "math" not loaded
    assert!(linker.resolve_imports(&import).is_err());
}

#[test]
fn test_resolve_imports_missing_module() {
    let linker = Linker::new();
    let import = ImportSig {
        module: "nonexistent".to_string(),
        symbols: vec![],
        aliases: BTreeMap::new(),
    };
    assert!(linker.resolve_imports(&import).is_err());
}

#[test]
fn test_resolve_imports_multiple_symbols() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub", "mul", "div",
            ],
        ))
        .unwrap();

    let import = ImportSig {
        module: "math".to_string(),
        symbols: vec![
            ImportSymbolSig {
                name: "add".to_string(),
                kind: SymbolKind::Function,
            },
            ImportSymbolSig {
                name: "sub".to_string(),
                kind: SymbolKind::Function,
            },
        ],
        aliases: BTreeMap::new(),
    };

    let resolved = linker.resolve_imports(&import).unwrap();
    assert_eq!(resolved.len(), 2);
}

// ─── Linker: circular dependency detection ──────────────────────────────────

#[test]
fn test_circular_dependency_a_b_c_a() {
    let mut linker = Linker::new();
    linker.load_module(&make_module_with_deps("a", &["b"])).unwrap();
    linker.load_module(&make_module_with_deps("b", &["c"])).unwrap();
    linker.load_module(&make_module_with_deps("c", &["a"])).unwrap();

    let errors = linker.detect_circular_dependencies();
    assert!(!errors.is_empty());

    let has_circular = errors.iter().any(|e| matches!(e, LinkError::CircularDependency(_)));
    assert!(has_circular);
}

#[test]
fn test_circular_dependency_self() {
    let mut linker = Linker::new();
    linker.load_module(&make_module_with_deps("a", &["a"])).unwrap();

    let errors = linker.detect_circular_dependencies();
    assert!(!errors.is_empty());
}

#[test]
fn test_no_circular_dependency_linear() {
    let mut linker = Linker::new();
    linker.load_module(&make_module_with_deps("a", &["b"])).unwrap();
    linker.load_module(&make_module_with_deps("b", &["c"])).unwrap();
    linker.load_module(&make_module_with_deps("c", &[])).unwrap();

    let errors = linker.detect_circular_dependencies();
    assert!(errors.is_empty());
}

#[test]
fn test_no_circular_dependency_diamond() {
    // a -> b, a -> c, b -> d, c -> d (diamond, not circular)
    let mut linker = Linker::new();
    let mut sig_a = make_module_with_deps("a", &["b", "c"]);
    // No actual cycle
    linker.load_module(&sig_a).unwrap();
    linker.load_module(&make_module_with_deps("b", &["d"])).unwrap();
    linker.load_module(&make_module_with_deps("c", &["d"])).unwrap();
    linker.load_module(&make_module_with_deps("d", &[])).unwrap();

    let errors = linker.detect_circular_dependencies();
    assert!(errors.is_empty());
}

// ─── Linker: LinkResult resolve ─────────────────────────────────────────────

#[test]
fn test_link_result_resolve_renamed() {
    let mut linker = Linker::new();
    linker.load_module(&make_module("math", &["parse"])).unwrap();
    linker.load_module(&make_module("string", &["parse"])).unwrap();

    let result = linker.link_all();
    let resolved = result.resolve("math_parse");
    assert_eq!(resolved, "math_parse");
}

#[test]
fn test_link_result_resolve_unrenamed() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module(
            "math",
            &[
                "add", "sub",
            ],
        ))
        .unwrap();

    let result = linker.link_all();
    let resolved = result.resolve("add");
    assert_eq!(resolved, "add");
}

// ─── Linker: import with types ──────────────────────────────────────────────

#[test]
fn test_resolve_imports_with_type() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module_with_types(
            "lib",
            &[],
            &[
                "Point", "Line",
            ],
        ))
        .unwrap();

    let import = ImportSig {
        module: "lib".to_string(),
        symbols: vec![
            ImportSymbolSig {
                name: "Point".to_string(),
                kind: SymbolKind::Type,
            },
        ],
        aliases: BTreeMap::new(),
    };

    let resolved = linker.resolve_imports(&import).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].original_name, "Point");
}

// ─── Linker: mixed modules ──────────────────────────────────────────────────

#[test]
fn test_link_all_mixed_modules() {
    let mut linker = Linker::new();
    linker
        .load_module(&make_module_with_types(
            "math",
            &[
                "add", "sub",
            ],
            &["Matrix"],
        ))
        .unwrap();
    linker
        .load_module(&make_module_with_types(
            "string",
            &["concat"],
            &["StringBuilder"],
        ))
        .unwrap();

    let result = linker.link_all();
    assert!(result.is_ok());
    // 3 functions (add, sub, concat) + 2 types (Matrix, StringBuilder) = 5 symbols
    assert_eq!(result.symbols.len(), 5);
}

// ─── LinkError Display ──────────────────────────────────────────────────────

#[test]
fn test_link_error_display_symbol_not_found() {
    let e = LinkError::SymbolNotFound("foo::bar".to_string());
    assert!(e.to_string().contains("foo::bar"));
}

#[test]
fn test_link_error_display_symbol_conflict() {
    let e = LinkError::SymbolConflict {
        symbol: "parse".to_string(),
        modules: vec![
            "math".to_string(),
            "string".to_string(),
        ],
    };
    let s = e.to_string();
    assert!(s.contains("parse"));
    assert!(s.contains("math"));
    assert!(s.contains("string"));
}

#[test]
fn test_link_error_display_module_not_found() {
    let e = LinkError::ModuleNotFound("nonexistent".to_string());
    assert!(e.to_string().contains("nonexistent"));
}

#[test]
fn test_link_error_display_circular_dependency() {
    let e = LinkError::CircularDependency(vec![
        "a".to_string(),
        "b".to_string(),
        "c".to_string(),
        "a".to_string(),
    ]);
    let s = e.to_string();
    assert!(s.contains("a"));
    assert!(s.contains("b"));
    assert!(s.contains("c"));
}
