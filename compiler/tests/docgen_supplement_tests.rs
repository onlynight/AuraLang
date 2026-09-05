#![cfg(feature = "llvm")]

use compiler::docgen::DocRegistry;

#[test]
fn test_doc_entries_count() {
    let registry = DocRegistry::new().load_all();
    let count = registry.all().len();
    assert!(count >= 88, "Fix 14 后文档条目应 >= 88，实际 {}", count);
}

#[test]
fn test_doc_concurrent_module() {
    let registry = DocRegistry::new().load_all();
    let docs = registry.by_module("concurrent");
    assert!(!docs.is_empty(), "concurrent 模块应有文档");
    assert!(docs.iter().any(|d| d.name == "aura.concurrent.spawn"));
    assert!(docs.iter().any(|d| d.name == "aura.concurrent.newChannel"));
}

#[test]
fn test_doc_ffi_module() {
    let registry = DocRegistry::new().load_all();
    let docs = registry.by_module("ffi");
    assert!(!docs.is_empty(), "ffi 模块应有文档");
    assert!(docs.iter().any(|d| d.name == "aura.ffi.CString"));
    assert!(docs.iter().any(|d| d.name == "aura.ffi.makeCallback"));
}
