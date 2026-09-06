//! P13: LSP 服务器 (aura-lsp)
//!
//! 实现 Language Server Protocol，通过 JSON-RPC 与编辑器通信。
//! 详见技术方案.md 第十章。

use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};

use crate::ast::*;
use crate::errors::CompileError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::span::Span;

// ─────────────────────────────────────────────────────────────────────────────
// LSP 消息类型
// ─────────────────────────────────────────────────────────────────────────────

/// JSON-RPC 响应
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: serde_json::Value,
}

/// LSP 错误
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspError {
    pub code: i32,
    pub message: String,
}

/// 位置
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Copy)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

/// 范围
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Copy)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// 位置引用
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// 补全项
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
}

/// 悬停结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HoverResult {
    pub contents: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// 诊断信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LspDiagnostic {
    pub range: Range,
    pub severity: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub source: String,
    pub message: String,
}

/// 文本编辑
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// 符号信息
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Variable,
    Constant,
    Struct,
    Class,
    Enum,
    Interface,
    TypeAlias,
    Field,
    Method,
    Parameter,
    Module,
}

impl SymbolKind {
    fn to_lsp_kind(&self) -> i32 {
        match self {
            SymbolKind::Function => 3,
            SymbolKind::Variable => 6,
            SymbolKind::Constant => 14,
            SymbolKind::Struct => 2,
            SymbolKind::Class => 5,
            SymbolKind::Enum => 1,
            SymbolKind::Interface => 8,
            SymbolKind::TypeAlias => 7,
            SymbolKind::Field => 8,
            SymbolKind::Method => 2,
            SymbolKind::Parameter => 9,
            SymbolKind::Module => 9,
        }
    }
}

/// 符号信息（用于补全、跳转定义、悬停）
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub type_str: String,
    pub doc: Option<String>,
    pub visibility: Visibility,
}

// ─────────────────────────────────────────────────────────────────────────────
// 文档状态管理（13.4）
// ─────────────────────────────────────────────────────────────────────────────

/// 文档状态
#[derive(Debug)]
pub struct DocumentState {
    pub uri: String,
    pub text: String,
    pub version: i32,
    pub ast: Option<Program>,
    pub symbols: HashMap<String, SymbolInfo>,
    pub errors: Vec<CompileError>,
}

impl DocumentState {
    pub fn new(uri: &str, text: &str, version: i32) -> Self {
        Self {
            uri: uri.to_string(),
            text: text.to_string(),
            version,
            ast: None,
            symbols: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// 重新分析文档（词法 + 语法）
    pub fn analyze(&mut self) -> &mut Self {
        let mut lexer = Lexer::new(&self.text);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program();

        self.symbols.clear();
        self.collect_symbols(&program);
        self.ast = Some(program);
        self.errors = parser.errors().to_vec();
        self
    }

    fn collect_symbols(&mut self, program: &Program) {
        for decl in &program.declarations {
            match decl {
                Decl::Function(f) => {
                    let ret_type = f
                        .return_type
                        .as_ref()
                        .map(|t| self.type_to_string(t))
                        .unwrap_or_else(|| "Unit".to_string());
                    self.symbols.insert(
                        f.name.clone(),
                        SymbolInfo {
                            name: f.name.clone(),
                            kind: SymbolKind::Function,
                            span: f.span,
                            type_str: format!("fun {}() -> {}", f.name, ret_type),
                            doc: None,
                            visibility: Visibility::Public,
                        },
                    );
                }
                Decl::Struct(s) => {
                    self.symbols.insert(
                        s.name.clone(),
                        SymbolInfo {
                            name: s.name.clone(),
                            kind: SymbolKind::Struct,
                            span: s.span,
                            type_str: format!("struct {}", s.name),
                            doc: None,
                            visibility: Visibility::Public,
                        },
                    );
                    for f in &s.fields {
                        let ty_str = f
                            .type_hint
                            .as_ref()
                            .map(|t| self.type_to_string(t))
                            .unwrap_or_else(|| "Any".to_string());
                        self.symbols.insert(
                            f.name.clone(),
                            SymbolInfo {
                                name: f.name.clone(),
                                kind: SymbolKind::Field,
                                span: f.span,
                                type_str: ty_str,
                                doc: None,
                                visibility: f.visibility,
                            },
                        );
                    }
                }
                Decl::Class(c) => {
                    self.symbols.insert(
                        c.name.clone(),
                        SymbolInfo {
                            name: c.name.clone(),
                            kind: SymbolKind::Class,
                            span: c.span,
                            type_str: format!("class {}", c.name),
                            doc: None,
                            visibility: Visibility::Public,
                        },
                    );
                }
                Decl::Enum(e) => {
                    self.symbols.insert(
                        e.name.clone(),
                        SymbolInfo {
                            name: e.name.clone(),
                            kind: SymbolKind::Enum,
                            span: e.span,
                            type_str: format!("enum {}", e.name),
                            doc: None,
                            visibility: Visibility::Public,
                        },
                    );
                }
                Decl::Interface(i) => {
                    self.symbols.insert(
                        i.name.clone(),
                        SymbolInfo {
                            name: i.name.clone(),
                            kind: SymbolKind::Interface,
                            span: i.span,
                            type_str: format!("interface {}", i.name),
                            doc: None,
                            visibility: Visibility::Public,
                        },
                    );
                }
                Decl::TypeAlias(t) => {
                    let ty_str = self.type_to_string(&t.aliased_type);
                    self.symbols.insert(
                        t.name.clone(),
                        SymbolInfo {
                            name: t.name.clone(),
                            kind: SymbolKind::TypeAlias,
                            span: t.span,
                            type_str: format!("typealias {} = {}", t.name, ty_str),
                            doc: None,
                            visibility: t.visibility,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    fn type_to_string(&self, ty: &Type) -> String {
        match ty {
            Type::Named { name, .. } => name.clone(),
            Type::Int => "Int".to_string(),
            Type::Long => "Long".to_string(),
            Type::Float => "Float".to_string(),
            Type::Double => "Double".to_string(),
            Type::Boolean => "Boolean".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "Char".to_string(),
            Type::Unit => "Unit".to_string(),
            Type::Any => "Any".to_string(),
            Type::Nothing => "Nothing".to_string(),
            Type::Short => "Short".to_string(),
            Type::Byte => "Byte".to_string(),
            Type::Nullable(t) => format!("{}?", self.type_to_string(t)),
            Type::Pointer(t) => format!("Pointer<{}>", self.type_to_string(t)),
            Type::Array(t) => format!("Array<{}>", self.type_to_string(t)),
            Type::Function {
                params,
                return_type,
                ..
            } => {
                let params_str: Vec<String> = params
                    .iter()
                    .map(|p| self.type_to_string(p.type_hint.as_deref().unwrap_or(&Type::Any)))
                    .collect();
                let ret_str = return_type
                    .as_ref()
                    .map(|t| self.type_to_string(t))
                    .unwrap_or_else(|| "Unit".to_string());
                format!("({}) -> {}", params_str.join(", "), ret_str)
            }
            Type::Generic {
                name, args, ..
            } => {
                let args_str: Vec<String> = args.iter().map(|a| self.type_to_string(a)).collect();
                format!("{}<{}>", name, args_str.join(", "))
            }
            Type::StarProjection { .. } => "*".to_string(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 文档管理器
// ─────────────────────────────────────────────────────────────────────────────

pub struct DocumentManager {
    documents: HashMap<String, DocumentState>,
}

impl DocumentManager {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: &str, text: &str, version: i32) -> &DocumentState {
        let mut state = DocumentState::new(uri, text, version);
        state.analyze();
        self.documents.insert(uri.to_string(), state);
        self.documents.get(uri).unwrap()
    }

    pub fn update(&mut self, uri: &str, text: &str, version: i32) -> &DocumentState {
        if let Some(state) = self.documents.get_mut(uri) {
            state.text = text.to_string();
            state.version = version;
            state.analyze();
        }
        self.documents.get_mut(uri).unwrap()
    }

    pub fn close(&mut self, uri: &str) {
        self.documents.remove(uri);
    }

    pub fn get(&self, uri: &str) -> Option<&DocumentState> {
        self.documents.get(uri)
    }

    pub fn get_mut(&mut self, uri: &str) -> Option<&mut DocumentState> {
        self.documents.get_mut(uri)
    }

    pub fn uris(&self) -> Vec<String> {
        self.documents.keys().map(|k| k.clone()).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 增量编译引擎（13.2）
// ─────────────────────────────────────────────────────────────────────────────

pub struct IncrementalEngine {
    doc_manager: DocumentManager,
    last_hashes: HashMap<String, u64>,
}

impl IncrementalEngine {
    pub fn new() -> Self {
        Self {
            doc_manager: DocumentManager::new(),
            last_hashes: HashMap::new(),
        }
    }

    pub fn open(&mut self, uri: &str, text: &str, version: i32) -> &DocumentState {
        let hash = Self::hash_text(text);
        self.last_hashes.insert(uri.to_string(), hash);
        self.doc_manager.open(uri, text, version)
    }

    pub fn update(&mut self, uri: &str, text: &str, version: i32) -> &DocumentState {
        let new_hash = Self::hash_text(text);
        let old_hash = self.last_hashes.get(uri).copied();

        if old_hash == Some(new_hash) {
            self.doc_manager.get_mut(uri).unwrap()
        } else {
            self.last_hashes.insert(uri.to_string(), new_hash);
            self.doc_manager.update(uri, text, version)
        }
    }

    pub fn close(&mut self, uri: &str) {
        self.last_hashes.remove(uri);
        self.doc_manager.close(uri);
    }

    pub fn docs(&self) -> &DocumentManager {
        &self.doc_manager
    }

    pub fn docs_mut(&mut self) -> &mut DocumentManager {
        &mut self.doc_manager
    }

    fn hash_text(text: &str) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in text.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LSP 处理器（13.3 - 13.9）
// ─────────────────────────────────────────────────────────────────────────────

pub struct LspHandler {
    engine: IncrementalEngine,
}

impl LspHandler {
    pub fn new() -> Self {
        Self {
            engine: IncrementalEngine::new(),
        }
    }

    /// 处理一条 JSON-RPC 消息。
    /// 仅对带 id 的请求返回响应；通知（notification，无 id）返回 None（协议规范）。
    pub fn handle(&mut self, message: &str) -> Option<LspResponse> {
        let value: serde_json::Value = serde_json::from_str(message).ok()?;
        let method = value.get("method").and_then(|m| m.as_str())?;
        let id = value.get("id").cloned();

        // 通知（无 id）不产生响应
        let id = match id {
            Some(id) => id,
            None => return None,
        };

        let result = match method {
            "initialize" => self.handle_initialize(),
            "shutdown" => serde_json::json!(null),
            "exit" => serde_json::json!(null),
            "textDocument/didOpen" => self.handle_did_open(&value["params"]),
            "textDocument/didChange" => self.handle_did_change(&value["params"]),
            "textDocument/didClose" => self.handle_did_close(&value["params"]),
            "textDocument/completion" => self.handle_completion(&value["params"]),
            "textDocument/definition" => self.handle_definition(&value["params"]),
            "textDocument/hover" => self.handle_hover(&value["params"]),
            "textDocument/diagnostic" => self.handle_diagnostic(&value["params"]),
            "textDocument/formatting" => self.handle_formatting(&value["params"]),
            _ => serde_json::Value::Null,
        };

        Some(LspResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result,
        })
    }

    fn handle_initialize(&mut self) -> serde_json::Value {
        serde_json::json!({
            "capabilities": {
                "textDocumentSync": { "openClose": true, "change": 2, "save": true },
                "completionProvider": { "triggerCharacters": [".", "(", ":"] },
                "definitionProvider": true,
                "hoverProvider": true,
                "diagnosticProvider": true,
                "documentFormattingProvider": true
            }
        })
    }

    fn handle_did_open(&mut self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let text = params["textDocument"]["text"].as_str().unwrap_or_default();
        let version = params["textDocument"]["version"].as_i64().unwrap_or(0) as i32;
        self.engine.open(uri, text, version);
        serde_json::json!({})
    }

    fn handle_did_change(&mut self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let version = params["textDocument"]["version"].as_i64().unwrap_or(0) as i32;
        if let Some(changes) = params["contentChanges"].as_array() {
            if let Some(last) = changes.last() {
                let text = last["text"].as_str().unwrap_or_default();
                self.engine.update(uri, text, version);
            }
        }
        serde_json::json!({})
    }

    fn handle_did_close(&mut self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        self.engine.close(uri);
        serde_json::json!({})
    }

    /// 13.5: 代码补全
    fn handle_completion(&self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let items: Vec<CompletionItem> = self
            .engine
            .docs()
            .get(uri)
            .map(|doc| {
                doc.symbols
                    .values()
                    .map(|s| CompletionItem {
                        label: s.name.clone(),
                        kind: s.kind.to_lsp_kind(),
                        insert_text: None,
                        detail: Some(s.type_str.clone()),
                        documentation: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        serde_json::to_value(&items).unwrap_or_default()
    }

    /// 13.6: 跳转定义
    fn handle_definition(&self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let line = params["position"]["line"].as_u64().unwrap_or(0);

        let location: Option<Location> = self.engine.docs().get(uri).and_then(|doc| {
            doc.symbols
                .values()
                .find(|s| {
                    line as u32 >= s.span.start_line as u32 && line as u32 <= s.span.end_line as u32
                })
                .map(|s| Location {
                    uri: uri.to_string(),
                    range: Range {
                        start: Position {
                            line: s.span.start_line as u32,
                            character: s.span.start_col as u32,
                        },
                        end: Position {
                            line: s.span.end_line as u32,
                            character: s.span.end_col as u32,
                        },
                    },
                })
        });

        serde_json::to_value(location).unwrap_or_default()
    }

    /// 13.7: 悬停提示
    fn handle_hover(&self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let line = params["position"]["line"].as_u64().unwrap_or(0);

        let hover: Option<HoverResult> = self.engine.docs().get(uri).and_then(|doc| {
            doc.symbols.values().find(|s| {
                line as u32 >= s.span.start_line as u32 && line as u32 <= s.span.end_line as u32
            }).map(|s| HoverResult {
                contents: serde_json::json!({
                    "kind": "markdown",
                    "value": format!("**{}**\n\n```\n{}\n```\n\n可见性: {}", s.name, s.type_str, match s.visibility {
                        Visibility::Public => "public",
                        Visibility::Private => "private",
                        Visibility::Internal => "internal",
                        Visibility::Protected => "protected",
                    })
                }),
                range: Some(Range {
                    start: Position { line: s.span.start_line as u32, character: s.span.start_col as u32 },
                    end: Position { line: s.span.end_line as u32, character: s.span.end_col as u32 },
                }),
            })
        });

        serde_json::to_value(hover).unwrap_or_default()
    }

    /// 13.8: 诊断推送
    fn handle_diagnostic(&self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();

        let diagnostics: Vec<LspDiagnostic> = self
            .engine
            .docs()
            .get(uri)
            .map(|doc| {
                doc.errors
                    .iter()
                    .map(|e| LspDiagnostic {
                        range: Range {
                            start: Position {
                                line: e.span.start_line as u32,
                                character: e.span.start_col as u32,
                            },
                            end: Position {
                                line: e.span.end_line as u32,
                                character: e.span.end_col as u32,
                            },
                        },
                        severity: match e.severity {
                            crate::errors::ErrorSeverity::Error => 1,
                            crate::errors::ErrorSeverity::Warning => 2,
                            crate::errors::ErrorSeverity::Info => 3,
                        },
                        code: None,
                        source: "aura".to_string(),
                        message: e.message.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        serde_json::to_value(diagnostics).unwrap_or_default()
    }

    /// 13.10: 代码格式化
    fn handle_formatting(&self, params: &serde_json::Value) -> serde_json::Value {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();

        let edits: Vec<TextEdit> = self
            .engine
            .docs()
            .get(uri)
            .map(|doc| {
                let formatted = format_source(&doc.text);
                if formatted != doc.text {
                    vec![TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: doc.text.lines().count() as u32,
                                character: 0,
                            },
                        },
                        new_text: formatted,
                    }]
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();

        serde_json::to_value(edits).unwrap_or_default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 代码格式化器（13.10）
// ─────────────────────────────────────────────────────────────────────────────

pub fn format_source(source: &str) -> String {
    let mut result = String::new();
    let mut indent_level: u32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            result.push('\n');
            continue;
        }

        if trimmed.starts_with('}') {
            indent_level = indent_level.saturating_sub(1);
        }

        let indent = "    ".repeat(indent_level as usize);
        result.push_str(&indent);
        result.push_str(trimmed);

        if trimmed.ends_with('{') || trimmed.ends_with('(') {
            indent_level += 1;
        }

        result.push('\n');
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// LSP 服务器主循环
// ─────────────────────────────────────────────────────────────────────────────

pub fn run_lsp_server() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut handler = LspHandler::new();

    // BufReader 用于逐行读取头，原始句柄用于读取 body
    let mut reader = io::BufReader::new(stdin.lock());
    let mut header_line = String::new();

    loop {
        header_line.clear();
        match reader.read_line(&mut header_line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }

        if header_line.trim().is_empty() {
            continue; // 空行分隔符
        }

        let content_length = match header_line.trim().split_once(":") {
            Some((_, v)) => v.trim().parse::<usize>().ok().unwrap_or(0),
            None => continue,
        };

        if content_length == 0 {
            continue;
        }

        // 读取空行分隔符
        let mut separator = String::new();
        if reader.read_line(&mut separator).is_err() {
            break;
        }

        // 读取 JSON body
        let mut buf = vec![0u8; content_length];
        if reader.read_exact(&mut buf).is_err() {
            continue;
        }
        let message = String::from_utf8_lossy(&buf).to_string();

        if let Some(response) = handler.handle(&message) {
            let response_str = serde_json::to_string(&response).unwrap_or_default();
            let mut out = stdout.lock();
            let _ = write!(out, "Content-Length: {}\r\n", response_str.len());
            let _ = write!(out, "\r\n");
            let _ = write!(out, "{}", response_str);
            let _ = out.flush();
        }

        if message.contains("\"method\":\"exit\"") || message.contains("\"method\": \"exit\"") {
            break;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_handler_initialize() {
        let mut handler = LspHandler::new();
        let response =
            handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#);
        assert!(response.is_some());
        let resp = response.unwrap();
        assert_eq!(resp.jsonrpc, "2.0");
        assert!(resp.result.is_object());
    }

    #[test]
    fn test_document_manager_open() {
        let mut dm = DocumentManager::new();
        let doc = dm.open("file:///test.aura", "fun main() {}", 1);
        assert_eq!(doc.uri, "file:///test.aura");
        assert_eq!(doc.version, 1);
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
    fn test_incremental_engine() {
        let mut engine = IncrementalEngine::new();
        engine.open("file:///test.aura", "fun main() {}", 1);
        assert!(engine.docs().get("file:///test.aura").is_some());
        engine.update("file:///test.aura", "fun main() {\n    println(1)\n}", 2);
        let doc = engine.docs().get("file:///test.aura").unwrap();
        assert_eq!(doc.version, 2);
    }

    #[test]
    fn test_completion() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#);
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
    fn test_definition() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#);
        assert!(response.is_some());
    }

    #[test]
    fn test_hover() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun add(a: Int, b: Int): Int { return a + b }\n","version":1}}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/hover","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#);
        assert!(response.is_some());
    }

    #[test]
    fn test_diagnostic() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun broken(\n","version":1}}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":"file:///test.aura"}}}"#);
        assert!(response.is_some());
    }

    #[test]
    fn test_format_source() {
        let input = "fun main(){println(1)}";
        let formatted = format_source(input);
        assert!(formatted.contains('\n'));
    }

    #[test]
    fn test_format_source_preserves_content() {
        let input = "fun main() {\n    println(1)\n}";
        let formatted = format_source(input);
        assert!(formatted.contains("fun main()"));
        assert!(formatted.contains("println(1)"));
    }

    #[test]
    fn test_incremental_update() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun a() {}","version":1}}}"#);
        handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/didChange","params":{"textDocument":{"uri":"file:///test.aura","version":2},"contentChanges":[{"text":"fun b() {}"}]}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":3,"method":"textDocument/completion","params":{"textDocument":{"uri":"file:///test.aura"},"position":{"line":0,"character":0}}}"#);
        let resp = response.unwrap();
        let items = resp.result.as_array().unwrap();
        let labels: Vec<String> =
            items.iter().map(|i| i["label"].as_str().unwrap().to_string()).collect();
        assert!(labels.contains(&"b".to_string()));
        assert!(!labels.contains(&"a".to_string()));
    }

    #[test]
    fn test_check_source() {
        let source = "fun main() {\n    val x = 1\n}";
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse_program();
        assert!(parser.errors().is_empty());
    }

    #[test]
    fn test_check_source_error() {
        let source = "fun broken(";
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        parser.parse_program();
        assert!(!parser.errors().is_empty());
    }

    #[test]
    fn test_repl_eval() {
        use crate::codegen::compile_source;
        use crate::vm::{Vm, VmOptions};
        let code = "println(\"Hello from REPL\")";
        let module = compile_source(code).unwrap();
        let opts = VmOptions::default();
        let mut vm = Vm::new(&module, opts).unwrap();
        let result = vm.run();
        assert!(result.is_ok());
    }

    #[test]
    fn test_document_state_analyze() {
        let mut state = DocumentState::new(
            "file:///test.aura",
            "fun add(a: Int, b: Int): Int { return a + b }",
            1,
        );
        state.analyze();
        assert!(state.ast.is_some());
        assert!(state.symbols.contains_key("add"));
    }

    #[test]
    fn test_symbol_info_lsp_kind() {
        let info = SymbolInfo {
            name: "test".to_string(),
            kind: SymbolKind::Function,
            span: Span::single(0, 1, 1),
            type_str: "fun test()".to_string(),
            doc: None,
            visibility: Visibility::Public,
        };
        assert_eq!(info.kind.to_lsp_kind(), 3);
    }

    #[test]
    fn test_position_range() {
        let pos = Position {
            line: 5,
            character: 10,
        };
        assert_eq!(pos.line, 5);
        assert_eq!(pos.character, 10);
    }

    #[test]
    fn test_text_edit() {
        let edit = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 5,
                    character: 0,
                },
            },
            new_text: "new content".to_string(),
        };
        assert_eq!(edit.new_text, "new content");
    }

    #[test]
    fn test_lsp_error() {
        let error = LspError {
            code: -32601,
            message: "Method not found".to_string(),
        };
        assert_eq!(error.code, -32601);
    }

    #[test]
    fn test_did_close() {
        let mut handler = LspHandler::new();
        handler.handle(r#"{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":{"textDocument":{"uri":"file:///test.aura","text":"fun main() {}","version":1}}}"#);
        let response = handler.handle(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/didClose","params":{"textDocument":{"uri":"file:///test.aura"}}}"#);
        assert!(response.is_some());
    }

    #[test]
    fn test_incremental_close() {
        let mut engine = IncrementalEngine::new();
        engine.open("file:///test.aura", "fun main() {}", 1);
        assert!(engine.docs().get("file:///test.aura").is_some());
        engine.close("file:///test.aura");
        assert!(engine.docs().get("file:///test.aura").is_none());
    }

    #[test]
    fn test_document_manager_uris() {
        let mut dm = DocumentManager::new();
        dm.open("file:///a.aura", "fun a() {}", 1);
        dm.open("file:///b.aura", "fun b() {}", 1);
        let uris = dm.uris();
        assert_eq!(uris.len(), 2);
    }

    #[test]
    fn test_format_source_nested() {
        let input = "fun outer(){fun inner(){println(1)}}";
        let formatted = format_source(input);
        // 格式化后应该包含所有函数名
        assert!(formatted.contains("fun outer()"));
        assert!(formatted.contains("fun inner()"));
    }
}
