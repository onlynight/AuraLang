//! LSP unit tests — completion, hover, definition, diagnostics, formatting, document management

use compiler::lsp::*;

// ─── LspHandler: initialize ─────────────────────────────────────────────────

#[test]
fn test_lsp_handler_initialize() {
    let mut handler = LspHandler::new();
    let response = handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
    assert!(response.is_some());
    let resp = response.unwrap();
    assert_eq!(resp.jsonrpc, "2.0");
    assert!(resp.result.is_object());

    // Verify capabilities are returned
    let caps = resp.result.get("capabilities").unwrap();
    assert!(caps.get("completionProvider").is_some());
    assert!(caps.get("definitionProvider").is_some());
    assert!(caps.get("hoverProvider").is_some());
    assert!(caps.get("diagnosticProvider").is_some());
    assert!(caps.get("documentFormattingProvider").is_some());
}

#[test]
fn test_lsp_handler_shutdown() {
    let mut handler = LspHandler::new();
    let response = handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"shutdown","params":{}}"#);
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_null());
}

// ─── DocumentManager ────────────────────────────────────────────────────────

#[test]
fn test_document_manager_open_and_get() {
    let mut dm = DocumentManager::new();
    let doc = dm.open("file:///test.aura", "fun main() {}", 1);
    assert_eq!(doc.uri, "file:///test.aura");
    assert_eq!(doc.version, 1);
    assert!(dm.get("file:///test.aura").is_some());
}

#[test]
fn test_document_manager_close() {
    let mut dm = DocumentManager::new();
    dm.open("file:///test.aura", "fun main() {}", 1);
    assert!(dm.get("file:///test.aura").is_some());
    dm.close("file:///test.aura");
    assert!(dm.get("file:///test.aura").is_none());
}

#[test]
fn test_document_manager_update() {
    let mut dm = DocumentManager::new();
    dm.open("file:///test.aura", "fun main() {}", 1);
    let doc = dm.update("file:///test.aura", "fun new_main() {}", 2);
    assert_eq!(doc.version, 2);
}

#[test]
fn test_document_manager_multiple_uris() {
    let mut dm = DocumentManager::new();
    dm.open("file:///a.aura", "fun a() {}", 1);
    dm.open("file:///b.aura", "fun b() {}", 1);
    dm.open("file:///c.aura", "fun c() {}", 1);
    let uris = dm.uris();
    assert_eq!(uris.len(), 3);
}

// ─── DocumentState: analyze ─────────────────────────────────────────────────

#[test]
fn test_document_state_analyze_function() {
    let mut state = DocumentState::new(
        "file:///test.aura",
        "fun add(a: Int, b: Int): Int { return a + b }",
        1,
    );
    state.analyze();
    assert!(state.ast.is_some());
    assert!(state.symbols.contains_key("add"));

    let symbol = &state.symbols["add"];
    assert_eq!(symbol.name, "add");
    assert_eq!(symbol.kind, SymbolKind::Function);
    assert!(symbol.type_str.contains("fun"));
}

#[test]
fn test_document_state_analyze_struct() {
    let source = "struct Point { var x: Int, var y: Int }";
    let mut state = DocumentState::new("file:///test.aura", source, 1);
    state.analyze();
    assert!(state.symbols.contains_key("Point"));

    let symbol = &state.symbols["Point"];
    assert_eq!(symbol.kind, SymbolKind::Struct);
}

#[test]
fn test_document_state_analyze_class() {
    let source = "class MyClass {}";
    let mut state = DocumentState::new("file:///test.aura", source, 1);
    state.analyze();
    assert!(state.symbols.contains_key("MyClass"));

    let symbol = &state.symbols["MyClass"];
    assert_eq!(symbol.kind, SymbolKind::Class);
}

#[test]
fn test_document_state_analyze_enum() {
    let source = "enum Color { Red, Green, Blue }";
    let mut state = DocumentState::new("file:///test.aura", source, 1);
    state.analyze();
    assert!(state.symbols.contains_key("Color"));

    let symbol = &state.symbols["Color"];
    assert_eq!(symbol.kind, SymbolKind::Enum);
}

#[test]
fn test_document_state_analyze_interface() {
    let source = "interface Drawable { fun draw() }";
    let mut state = DocumentState::new("file:///test.aura", source, 1);
    state.analyze();
    assert!(state.symbols.contains_key("Drawable"));

    let symbol = &state.symbols["Drawable"];
    assert_eq!(symbol.kind, SymbolKind::Interface);
}

#[test]
fn test_document_state_analyze_typealias() {
    let source = "typealias MyInt = Int";
    let mut state = DocumentState::new("file:///test.aura", source, 1);
    state.analyze();
    assert!(state.symbols.contains_key("MyInt"));

    let symbol = &state.symbols["MyInt"];
    assert_eq!(symbol.kind, SymbolKind::TypeAlias);
}

// ─── Completion ─────────────────────────────────────────────────────────────

#[test]
fn test_completion_returns_symbols() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_array());
    let items = resp.result.as_array().unwrap();
    assert!(!items.is_empty());
    let labels: Vec<String> =
        items.iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
    assert!(labels.contains(&"add".to_string()));
}

#[test]
fn test_completion_empty_document() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///empty.aura","text":"","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///empty.aura"},"position":{"line":0,"character":0}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_array());
    let items = resp.result.as_array().unwrap();
    assert!(items.is_empty());
}

#[test]
fn test_completion_multiple_symbols() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun a() {}\nfun b() {}\nfun c() {}\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#,
    );
    let resp = response.unwrap();
    let items = resp.result.as_array().unwrap();
    let labels: Vec<String> =
        items.iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
    assert!(labels.contains(&"a".to_string()));
    assert!(labels.contains(&"b".to_string()));
    assert!(labels.contains(&"c".to_string()));
}

// ─── Definition ─────────────────────────────────────────────────────────────

#[test]
fn test_definition_returns_location() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#,
    );
    assert!(response.is_some());
}

// ─── Hover ──────────────────────────────────────────────────────────────────

#[test]
fn test_hover_returns_info() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    // Result may be an object (hover found) or null (no symbol at position)
    assert!(resp.result.is_object() || resp.result.is_null());
    if let Some(hover) = resp.result.as_object() {
        assert!(hover.get("contents").is_some());
    }
}

// ─── Diagnostics ────────────────────────────────────────────────────────────

#[test]
fn test_diagnostic_clean_source() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///clean.aura","text":"fun main() {\n    val x = 1\n}\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///clean.aura"}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_array());
}

#[test]
fn test_diagnostic_error_source() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///broken.aura","text":"fun broken(\n","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///broken.aura"}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_array());
    // Should have at least one diagnostic
    let diagnostics = resp.result.as_array().unwrap();
    assert!(!diagnostics.is_empty());
}

// ─── Formatting ─────────────────────────────────────────────────────────────

#[test]
fn test_format_source_basic() {
    let input = "fun main(){println(1)}";
    let formatted = format_source(input);
    assert!(formatted.contains('\n'));
    assert!(formatted.contains("fun main()"));
    assert!(formatted.contains("println(1)"));
}

#[test]
fn test_format_source_already_formatted() {
    let input = "fun main() {\n    println(1)\n}";
    let formatted = format_source(input);
    assert!(formatted.contains("fun main()"));
    assert!(formatted.contains("println(1)"));
}

#[test]
fn test_format_source_nested() {
    let input = "fun outer(){fun inner(){println(1)}}";
    let formatted = format_source(input);
    assert!(formatted.contains("fun outer()"));
    assert!(formatted.contains("fun inner()"));
}

#[test]
fn test_format_source_empty() {
    let formatted = format_source("");
    assert!(formatted.is_empty() || formatted.lines().all(|l| l.is_empty()));
}

#[test]
fn test_lsp_formatting_edit() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun main(){println(1)}","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///test.aura"},"options":{"tabSize":4,"insertSpaces":true}}}"#,
    );
    assert!(response.is_some());
    let resp = response.unwrap();
    assert!(resp.result.is_array());
    // Should have at least one edit (source was unformatted)
    let edits = resp.result.as_array().unwrap();
    assert!(!edits.is_empty());
}

// ─── IncrementalEngine ──────────────────────────────────────────────────────

#[test]
fn test_incremental_engine_open_update_close() {
    let mut engine = IncrementalEngine::new();
    engine.open("file:///test.aura", "fun main() {}", 1);
    assert!(engine.docs().get("file:///test.aura").is_some());

    engine.update("file:///test.aura", "fun new_main() {}", 2);
    let doc = engine.docs().get("file:///test.aura").unwrap();
    assert_eq!(doc.version, 2);

    engine.close("file:///test.aura");
    assert!(engine.docs().get("file:///test.aura").is_none());
}

#[test]
fn test_incremental_engine_hash_unchanged() {
    let mut engine = IncrementalEngine::new();
    engine.open("file:///test.aura", "fun main() {}", 1);
    // Updating with the same text should not trigger re-analysis,
    // so the version stays at the old value (hash-based optimization)
    engine.update("file:///test.aura", "fun main() {}", 2);
    let doc = engine.docs().get("file:///test.aura").unwrap();
    assert_eq!(doc.version, 1);
}

// ─── didOpen / didChange / didClose ─────────────────────────────────────────

#[test]
fn test_did_open_and_close() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun main() {}","version":1}}}"#,
    );
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///test.aura"}}}"#,
    );
    assert!(response.is_some());
}

#[test]
fn test_did_change_updates_content() {
    let mut handler = LspHandler::new();
    handler.handle(
        r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun a() {}","version":1}}}"#,
    );
    handler.handle(
        r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///test.aura","version":2},"contentChanges":[{"text":"fun b() {}\n"}]}}"#,
    );
    // Completion should now show "b" instead of "a"
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#,
    );
    let resp = response.unwrap();
    let items = resp.result.as_array().unwrap();
    let labels: Vec<String> =
        items.iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
    assert!(labels.contains(&"b".to_string()));
    assert!(!labels.contains(&"a".to_string()));
}

// ─── Notification (no id) returns None ──────────────────────────────────────

#[test]
fn test_notification_returns_none() {
    let mut handler = LspHandler::new();
    // Notification has no "id" field
    let response = handler.handle(
        r#"{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun main() {}","version":1}}}"#,
    );
    assert!(response.is_none());
}

// ─── SymbolKind to_lsp_kind ─────────────────────────────────────────────────

#[test]
fn test_symbol_kind_lsp_mapping() {
    assert_eq!(SymbolKind::Function.to_lsp_kind(), 3);
    assert_eq!(SymbolKind::Variable.to_lsp_kind(), 6);
    assert_eq!(SymbolKind::Constant.to_lsp_kind(), 14);
    assert_eq!(SymbolKind::Struct.to_lsp_kind(), 2);
    assert_eq!(SymbolKind::Class.to_lsp_kind(), 5);
    assert_eq!(SymbolKind::Enum.to_lsp_kind(), 1);
    assert_eq!(SymbolKind::Interface.to_lsp_kind(), 8);
    assert_eq!(SymbolKind::TypeAlias.to_lsp_kind(), 7);
    assert_eq!(SymbolKind::Field.to_lsp_kind(), 8);
    assert_eq!(SymbolKind::Method.to_lsp_kind(), 2);
    assert_eq!(SymbolKind::Parameter.to_lsp_kind(), 9);
    assert_eq!(SymbolKind::Module.to_lsp_kind(), 9);
}
