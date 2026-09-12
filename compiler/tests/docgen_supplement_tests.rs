#![cfg(feature = "llvm")]

use compiler::docgen::DocRegistry;

#[test]
fn test_doc_entries_count() {
    let registry = DocRegistry::new().load_all();
    let count = registry.all().len();
    assert!(
        count >= 88,
        "after Fix 14 doc entries should be >= 88, actual {}",
        count
    );
}

#[test]
fn test_doc_concurrent_module() {
    let registry = DocRegistry::new().load_all();
    let docs = registry.by_module("concurrent");
    assert!(!docs.is_empty(), "concurrent module should have docs");
    assert!(docs.iter().any(|d| d.name == "aura.lang.std.Coroutine.spawn"));
    assert!(docs.iter().any(|d| d.name == "aura.lang.std.Channel.newChannel"));
}

#[test]
fn test_doc_ffi_module() {
    let registry = DocRegistry::new().load_all();
    let docs = registry.by_module("ffi");
    assert!(!docs.is_empty(), "ffi module should have docs");
    assert!(docs.iter().any(|d| d.name == "aura.ffi.CString"));
    assert!(docs.iter().any(|d| d.name == "aura.ffi.makeCallback"));
}
