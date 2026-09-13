//! Phase 2: Standard library index, symbol resolution and linking.
//!
//! Provides core data structures and operations for stdlib build:
//! - [`StdlibIndex`]: stdlib module index (module name, source/bytecode paths, function/type/constant lists)
//! - [`FfiIndex`]: FFI declaration index (function name, library, language, pre-loaded address)
//! - [`generate_stdlib_index`]: scan .aura files to build stdlib index
//! - [`generate_ffi_index`]: scan extern declarations to build FFI index
//! - [`load_stdlib_modules`]: load .auc bytecode files
//! - [`resolve_stdlib_symbol`]: stdlib symbol resolution
//! - [`link_stdlib_symbols`]: stdlib symbol linking
//!
//! Corresponds to Phase 2 §5.2 in the full Aura-ification plan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// Execution mode & FFI mode (aligned with bootstrap layer)
// ═══════════════════════════════════════════════════════════════════════════════

/// Execution mode (three-state: VM / JIT / AOT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    /// VM interpreted execution (default)
    #[default]
    Vm,
    /// JIT ahead-of-time compilation
    Jit,
    /// AOT compilation to native machine code
    Aot,
}

impl std::fmt::Display for ExecutionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionMode::Vm => write!(f, "vm"),
            ExecutionMode::Jit => write!(f, "jit"),
            ExecutionMode::Aot => write!(f, "aot"),
        }
    }
}

impl std::str::FromStr for ExecutionMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "vm" | "default" => Ok(ExecutionMode::Vm),
            "jit" => Ok(ExecutionMode::Jit),
            "aot" => Ok(ExecutionMode::Aot),
            _ => Err(format!("unknown execution mode: {}", s)),
        }
    }
}

/// FFI mode (default: AOT direct).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FfiMode {
    /// AOT direct: pre-load function addresses, direct call (default)
    #[default]
    Aot,
    /// Traditional C FFI: lookup function pointer by name, indirect call
    Cffi,
    /// Rust FFI: invoke Layer-1 native functions via registry
    RustFfi,
}

impl std::fmt::Display for FfiMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FfiMode::Aot => write!(f, "aot"),
            FfiMode::Cffi => write!(f, "cffi"),
            FfiMode::RustFfi => write!(f, "rustffi"),
        }
    }
}

impl std::str::FromStr for FfiMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "aot" | "default" => Ok(FfiMode::Aot),
            "cffi" => Ok(FfiMode::Cffi),
            "rustffi" | "rust" => Ok(FfiMode::RustFfi),
            _ => Err(format!("unknown FFI mode: {}", s)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stdlib index
// ═══════════════════════════════════════════════════════════════════════════════

/// Stdlib module information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdlibModule {
    /// Short module name (e.g. "Math", "String")
    pub name: String,
    /// Full module name (e.g. "aura.lang.std.Math")
    pub full_name: String,
    /// Source file path (.aura)
    pub source_path: PathBuf,
    /// Bytecode file path (.auc, filled after compilation)
    pub auc_path: Option<PathBuf>,
    /// Function names in this module
    pub function_names: Vec<String>,
    /// Type names in this module
    pub type_names: Vec<String>,
    /// Constant names in this module
    pub constant_names: Vec<String>,
    /// Whether the module contains extern declarations
    pub has_extern: bool,
}

/// Stdlib index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StdlibIndex {
    /// Module list
    pub modules: Vec<StdlibModule>,
    /// Output directory (where .auc files reside)
    pub output_dir: PathBuf,
    /// Execution mode
    pub execution_mode: ExecutionMode,
    /// FFI mode
    pub ffi_mode: FfiMode,
}

impl StdlibIndex {
    /// Create an empty index.
    pub fn new(output_dir: PathBuf, execution_mode: ExecutionMode, ffi_mode: FfiMode) -> Self {
        Self {
            modules: Vec::new(),
            output_dir,
            execution_mode,
            ffi_mode,
        }
    }

    /// Add a module.
    pub fn add_module(&mut self, module: StdlibModule) {
        self.modules.push(module);
    }

    /// Find a module by short name.
    pub fn find_module(&self, name: &str) -> Option<&StdlibModule> {
        self.modules.iter().find(|m| m.name == name || m.full_name.ends_with(&format!(".{}", name)))
    }

    /// Find a module by short name (mutable).
    pub fn find_module_mut(&mut self, name: &str) -> Option<&mut StdlibModule> {
        self.modules
            .iter_mut()
            .find(|m| m.name == name || m.full_name.ends_with(&format!(".{}", name)))
    }

    /// Number of modules.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// FFI index
// ═══════════════════════════════════════════════════════════════════════════════

/// FFI declaration information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FfiDeclaration {
    /// Function name (e.g. "fopen", "malloc")
    pub name: String,
    /// Library name (e.g. "libc", "mylib")
    pub library: String,
    /// Language ("c" or "rust")
    pub language: String,
    /// Declaring module (e.g. "FileSystem", "Builtin")
    pub module: String,
    /// Pre-loaded function address (AOT direct mode)
    pub function_address: Option<u64>,
}

/// FFI index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FfiIndex {
    /// FFI declaration list
    pub declarations: Vec<FfiDeclaration>,
    /// Function name → address mapping (AOT direct pre-loading)
    pub function_addresses: HashMap<String, u64>,
    /// FFI mode
    pub ffi_mode: FfiMode,
}

impl FfiIndex {
    /// Create an empty index.
    pub fn new(ffi_mode: FfiMode) -> Self {
        Self {
            declarations: Vec::new(),
            function_addresses: HashMap::new(),
            ffi_mode,
        }
    }

    /// Add an FFI declaration.
    pub fn add_declaration(&mut self, decl: FfiDeclaration) {
        if let Some(addr) = decl.function_address {
            self.function_addresses.insert(decl.name.clone(), addr);
        }
        self.declarations.push(decl);
    }

    /// Set a function address (AOT direct pre-loading).
    pub fn set_function_address(&mut self, name: &str, address: u64) {
        self.function_addresses.insert(name.to_string(), address);
        if let Some(decl) = self.declarations.iter_mut().find(|d| d.name == name) {
            decl.function_address = Some(address);
        }
    }

    /// Get a function address.
    pub fn get_function_address(&self, name: &str) -> Option<u64> {
        self.function_addresses.get(name).copied()
    }

    /// Pre-load all function addresses (AOT direct mode).
    ///
    /// Loads function addresses from shared libraries via dlsym / GetProcAddress.
    /// Failures do not abort the build; errors are collected and returned.
    pub fn preload_all_addresses(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        for decl in &mut self.declarations {
            if decl.language != "c" {
                continue; // Rust FFI does not need pre-loading
            }
            if decl.function_address.is_none() {
                match load_function_address(&decl.name, &decl.library) {
                    Ok(addr) => {
                        decl.function_address = Some(addr);
                        self.function_addresses.insert(decl.name.clone(), addr);
                    }
                    Err(e) => {
                        errors.push(format!("{}: {}", decl.name, e));
                    }
                }
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
    }

    /// Filter FFI declarations by module.
    pub fn declarations_for_module(&self, module: &str) -> Vec<&FfiDeclaration> {
        self.declarations.iter().filter(|d| d.module == module).collect()
    }

    /// Number of declarations.
    pub fn len(&self) -> usize {
        self.declarations.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.declarations.is_empty()
    }

    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stdlib index generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan .aura file list and generate a stdlib index.
///
/// Iterates each .aura file, parses the AST, extracts module name,
/// function names, type names and constant names.
pub fn generate_stdlib_index(
    aura_files: &[PathBuf],
    output_dir: PathBuf,
    execution_mode: ExecutionMode,
    ffi_mode: FfiMode,
) -> Result<StdlibIndex, String> {
    let mut index = StdlibIndex::new(output_dir, execution_mode, ffi_mode);

    for file in aura_files {
        let source = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {}", file.display(), e))?;

        let module_info = extract_module_info(file, &source)?;
        index.add_module(module_info);
    }

    Ok(index)
}

/// Extract module information from a single .aura file.
fn extract_module_info(file: &Path, source: &str) -> Result<StdlibModule, String> {
    use compiler::lexer::Lexer;
    use compiler::parser::Parser;

    // Lexing
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        return Err(format!("lex error: {}", e.message));
    }

    // Parsing
    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    // Extract module name from file name
    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module").to_string();
    let module_name = file_name.trim_end_matches(".aura").to_string();

    // Build full name: aura.lang.std.<ModuleName>
    let full_name = if file.to_string_lossy().contains("aura/lang/std") {
        format!("aura.lang.std.{}", module_name)
    } else if file.to_string_lossy().contains("aura/lang") {
        format!("aura.lang.{}", module_name)
    } else {
        format!("aura.lang.std.{}", module_name)
    };

    let mut function_names = Vec::new();
    let mut type_names = Vec::new();
    let mut constant_names = Vec::new();
    let mut has_extern = false;

    for decl in &program.declarations {
        match decl {
            compiler::ast::Decl::Function(fn_decl) => {
                function_names.push(fn_decl.name.clone());
            }
            compiler::ast::Decl::Struct(struct_decl) => {
                type_names.push(struct_decl.name.clone());
            }
            compiler::ast::Decl::Class(class_decl) => {
                type_names.push(class_decl.name.clone());
            }
            compiler::ast::Decl::Interface(interface_decl) => {
                type_names.push(interface_decl.name.clone());
            }
            compiler::ast::Decl::Enum(enum_decl) => {
                type_names.push(enum_decl.name.clone());
            }
            compiler::ast::Decl::Actor(actor_decl) => {
                type_names.push(actor_decl.name.clone());
            }
            compiler::ast::Decl::Object(object_decl) => {
                type_names.push(object_decl.name.clone());
            }
            compiler::ast::Decl::TypeAlias(type_alias) => {
                type_names.push(type_alias.name.clone());
            }
            compiler::ast::Decl::Extern(extern_decl) => {
                has_extern = true;
                for fn_decl in &extern_decl.functions {
                    function_names.push(fn_decl.name.clone());
                }
                // FFI constants (extern val NAME: Type)
                for stmt in &extern_decl.constants {
                    match stmt {
                        compiler::ast::Stmt::Val { name, .. }
                        | compiler::ast::Stmt::Var { name, .. } => {
                            constant_names.push(name.clone());
                        }
                        _ => {}
                    }
                }
            }
            compiler::ast::Decl::ExternObject(extern_if) => {
                has_extern = true;
                for fn_decl in &extern_if.functions {
                    function_names.push(fn_decl.name.clone());
                }
            }
            compiler::ast::Decl::Import(_) => {}
            compiler::ast::Decl::Annotation(_) => {}
        }
    }

    Ok(StdlibModule {
        name: module_name,
        full_name,
        source_path: file.to_path_buf(),
        auc_path: None,
        function_names,
        type_names,
        constant_names,
        has_extern,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// FFI index generation
// ═══════════════════════════════════════════════════════════════════════════════

/// Scan .aura file list, extract extern declarations, and generate an FFI index.
pub fn generate_ffi_index(aura_files: &[PathBuf], ffi_mode: FfiMode) -> Result<FfiIndex, String> {
    let mut index = FfiIndex::new(ffi_mode);

    for file in aura_files {
        let source = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {}", file.display(), e))?;

        let declarations = extract_ffi_declarations(file, &source)?;
        for decl in declarations {
            index.add_declaration(decl);
        }
    }

    // AOT direct mode: attempt to pre-load all function addresses
    if matches!(ffi_mode, FfiMode::Aot) {
        if let Err(e) = index.preload_all_addresses() {
            tracing::warn!("FFI AOT direct pre-loading partially failed: {}", e);
        }
    }

    Ok(index)
}

/// Extract FFI declarations from a single .aura file.
fn extract_ffi_declarations(file: &Path, source: &str) -> Result<Vec<FfiDeclaration>, String> {
    use compiler::lexer::Lexer;
    use compiler::parser::Parser;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize();
    if let Some(e) = lexer.errors().first() {
        return Err(format!("lex error: {}", e.message));
    }

    let mut parser = Parser::new(tokens);
    let program = parser.parse_program();

    let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("module").to_string();
    let module_name = file_name.trim_end_matches(".aura").to_string();

    let mut declarations = Vec::new();

    // Scan AST for extern declarations
    for decl in &program.declarations {
        if let compiler::ast::Decl::Extern(extern_decl) = decl {
            let library = extern_decl.library.clone().unwrap_or_else(|| "libc".to_string());
            let language = if extern_decl.abi == "c" { "c" } else { "rust" };

            for fn_decl in &extern_decl.functions {
                declarations.push(FfiDeclaration {
                    name: fn_decl.name.clone(),
                    library: library.clone(),
                    language: language.to_string(),
                    module: module_name.clone(),
                    function_address: None,
                });
            }
        }

        // Also scan extern interface declarations
        if let compiler::ast::Decl::ExternObject(extern_if) = decl {
            let library = extern_if.lib_path.clone().unwrap_or_else(|| "native".to_string());

            for fn_decl in &extern_if.functions {
                declarations.push(FfiDeclaration {
                    name: fn_decl.name.clone(),
                    library: library.clone(),
                    language: "rust".to_string(),
                    module: module_name.clone(),
                    function_address: None,
                });
            }
        }
    }

    // Text scan as a fallback for extern declarations not captured by AST
    let text_declarations = extract_extern_by_text(source, &module_name);
    for decl in text_declarations {
        if !declarations.iter().any(|d| d.name == decl.name) {
            declarations.push(decl);
        }
    }

    Ok(declarations)
}

/// Text-scan fallback: extract extern declarations from source text.
fn extract_extern_by_text(source: &str, module: &str) -> Vec<FfiDeclaration> {
    let mut declarations = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("extern") {
            continue;
        }

        let is_c = trimmed.contains("\"c\"") || trimmed.contains("'c'");
        let is_rust = trimmed.contains("\"rust\"") || trimmed.contains("'rust'");
        if !is_c && !is_rust {
            continue;
        }

        let language = if is_c { "c" } else { "rust" };
        let library = if is_c {
            extract_library_name(trimmed).unwrap_or_else(|| "libc".to_string())
        } else {
            "native".to_string()
        };

        // Extract function name: `extern "c" "libc" fun fopen(...)`
        if let Some(fun_part) = trimmed.strip_prefix("fun ") {
            let fn_name = fun_part.split('(').next().unwrap_or("").trim().to_string();
            if !fn_name.is_empty() {
                declarations.push(FfiDeclaration {
                    name: fn_name,
                    library,
                    language: language.to_string(),
                    module: module.to_string(),
                    function_address: None,
                });
            }
        }
    }

    declarations
}

/// Extract library name from an extern declaration line.
///
/// Format: `extern "c" "libc" fun fopen(...)`
fn extract_library_name(line: &str) -> Option<String> {
    let quotes: Vec<&str> = line.split('"').collect();
    if quotes.len() >= 4 {
        let lib_name = quotes[3].trim();
        if !lib_name.is_empty() && !lib_name.starts_with("fun") {
            return Some(lib_name.to_string());
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// FFI function address pre-loading (AOT direct)
// ═══════════════════════════════════════════════════════════════════════════════

/// Load a function address from a shared library (AOT direct core).
///
/// Uses `libloading` to load the shared library and resolve the function.
/// Returns the function address as u64. Failures are collected but do not
/// abort the build (the caller decides whether to fall back).
#[cfg(windows)]
fn load_function_address(name: &str, library: &str) -> Result<u64, String> {
    let lib_name = if library == "libc" || library == "native" {
        "msvcrt.dll".to_string()
    } else if library.ends_with(".dll") {
        library.to_string()
    } else {
        format!("{}.dll", library)
    };

    let lib = unsafe { libloading::Library::new(&lib_name) }
        .map_err(|e| format!("cannot load library {}: {}", lib_name, e))?;

    let func = unsafe {
        lib.get::<unsafe extern "C" fn()>(name.as_bytes())
            .map_err(|e| format!("cannot find function {}: {}", name, e))?
    };

    // Deref the symbol to get the function pointer, then cast to address
    let func_ptr: unsafe extern "C" fn() = *func;
    let ptr = func_ptr as usize as u64;
    Ok(ptr)
}

#[cfg(unix)]
fn load_function_address(name: &str, library: &str) -> Result<u64, String> {
    let lib_name = if library == "libc" || library == "native" {
        "libc.so.6".to_string()
    } else if library.starts_with("lib")
        && (library.ends_with(".so") || library.ends_with(".dylib"))
    {
        library.to_string()
    } else {
        format!("lib{}.so", library)
    };

    let lib = unsafe { libloading::Library::new(&lib_name) }
        .map_err(|e| format!("cannot load library {}: {}", lib_name, e))?;

    let func = unsafe {
        lib.get::<unsafe extern "C" fn()>(name.as_bytes())
            .map_err(|e| format!("cannot find function {}: {}", name, e))?
    };

    // Deref the symbol to get the function pointer, then cast to address
    let func_ptr: unsafe extern "C" fn() = *func;
    let ptr = func_ptr as usize as u64;
    Ok(ptr)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Stdlib module loading & symbol resolution
// ═══════════════════════════════════════════════════════════════════════════════

/// Load stdlib modules from .auc files.
///
/// Iterates all .auc files in `auc_dir`, reads them, and returns a list of
/// (module_name, bytecode_module) pairs.
pub fn load_stdlib_modules(
    auc_dir: &Path,
) -> Result<Vec<(String, compiler::codegen::BytecodeModule)>, String> {
    let mut modules = Vec::new();

    if !auc_dir.exists() {
        return Ok(modules);
    }

    let entries = std::fs::read_dir(auc_dir)
        .map_err(|e| format!("cannot read directory {}: {}", auc_dir.display(), e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "auc").unwrap_or(false) {
            let module_name =
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();

            match compiler::codegen::read_auc(&path.to_string_lossy()) {
                Ok(module) => modules.push((module_name, module)),
                Err(e) => {
                    tracing::warn!("failed to load {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(modules)
}

/// Symbol kind for stdlib resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Function,
    Type,
    Constant,
}

/// Resolve a stdlib symbol by name.
///
/// Searches the stdlib index for the symbol and returns (module_name, symbol_kind).
pub fn resolve_stdlib_symbol(
    index: &StdlibIndex,
    symbol_name: &str,
) -> Option<(String, SymbolKind)> {
    for module in &index.modules {
        if module.function_names.iter().any(|n| n == symbol_name) {
            return Some((module.name.clone(), SymbolKind::Function));
        }
        if module.type_names.iter().any(|n| n == symbol_name) {
            return Some((module.name.clone(), SymbolKind::Type));
        }
        if module.constant_names.iter().any(|n| n == symbol_name) {
            return Some((module.name.clone(), SymbolKind::Constant));
        }
    }
    None
}

/// Link stdlib symbols: resolve application-level stdlib calls to stdlib modules.
///
/// Returns a mapping: caller symbol → (stdlib module name, symbol kind).
pub fn link_stdlib_symbols(
    index: &StdlibIndex,
    app_symbols: &[String],
) -> HashMap<String, (String, SymbolKind)> {
    let mut links = HashMap::new();

    for symbol in app_symbols {
        // Direct match
        if let Some((module_name, kind)) = resolve_stdlib_symbol(index, symbol) {
            links.insert(symbol.clone(), (module_name, kind));
            continue;
        }

        // Strip module prefix (e.g. "Math.abs" → "abs")
        if let Some(dot_pos) = symbol.find('.') {
            let suffix = &symbol[dot_pos + 1..];
            if let Some((module_name, kind)) = resolve_stdlib_symbol(index, suffix) {
                links.insert(symbol.clone(), (module_name, kind));
            }
        }
    }

    links
}

// ═══════════════════════════════════════════════════════════════════════════════
// Source file scanning
// ═══════════════════════════════════════════════════════════════════════════════

/// Recursively scan a directory for all .aura files.
///
/// Returns sorted .aura file paths.
pub fn scan_aura_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    scan_aura_files_recursive(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn scan_aura_files_recursive(dir: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read directory {}: {}", dir.display(), e))?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            scan_aura_files_recursive(&path, result)?;
        } else if path.extension().map(|e| e == "aura").unwrap_or(false) {
            result.push(path);
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// C FFI library compilation
// ═══════════════════════════════════════════════════════════════════════════════

/// C FFI library compilation result.
#[derive(Debug, Clone)]
pub struct CffiCompileResult {
    /// Static library path (.a / .lib)
    pub library_path: Option<PathBuf>,
    /// Whether compilation succeeded
    pub success: bool,
    /// Compilation log
    pub log: String,
}

/// Compile C FFI library into a static library.
///
/// Compiles .c files in `cffi_dir` into a static library in `output_dir`.
/// Uses the system C compiler (gcc / cl.exe). Failures do not abort the build;
/// the caller may fall back to bytecode-only mode.
pub fn compile_cffi_library(cffi_dir: &Path, output_dir: &Path) -> CffiCompileResult {
    let mut log = String::new();

    if !cffi_dir.exists() {
        return CffiCompileResult {
            library_path: None,
            success: false,
            log: format!("C FFI directory does not exist: {}", cffi_dir.display()),
        };
    }

    let c_files: Vec<PathBuf> = std::fs::read_dir(cffi_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "c").unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();

    if c_files.is_empty() {
        return CffiCompileResult {
            library_path: None,
            success: false,
            log: "no .c files found".to_string(),
        };
    }

    std::fs::create_dir_all(output_dir).ok();

    let lib_name = if cfg!(windows) { "aura_std_cffi.lib" } else { "libaura_std_cffi.a" };
    let lib_path = output_dir.join(&lib_name);

    match compile_c_files(&c_files, &lib_path, cffi_dir) {
        Ok(compile_log) => {
            log.push_str(&compile_log);
            tracing::info!("C FFI library compiled: {}", lib_path.display());
            CffiCompileResult {
                library_path: Some(lib_path),
                success: true,
                log,
            }
        }
        Err(e) => {
            log.push_str(&format!("compilation failed: {}", e));
            tracing::warn!(
                "C FFI library compilation failed (fallback to bytecode-only): {}",
                e
            );
            CffiCompileResult {
                library_path: None,
                success: false,
                log,
            }
        }
    }
}

/// Compile .c files into a static library using the system C compiler.
fn compile_c_files(
    c_files: &[PathBuf],
    output: &Path,
    include_dir: &Path,
) -> Result<String, String> {
    use std::process::Command;

    let (compiler, args) = if cfg!(windows) {
        (
            "cl.exe",
            vec![
                "/c".to_string(),
                format!("/I{}", include_dir.display()),
            ],
        )
    } else {
        (
            "gcc",
            vec![
                "-c".to_string(),
                format!("-I{}", include_dir.display()),
            ],
        )
    };

    // Compile each .c file to .o
    let mut object_files = Vec::new();
    for c_file in c_files {
        let obj_name = c_file.file_stem().and_then(|s| s.to_str()).unwrap_or("obj").to_string();
        let obj_path = output.with_file_name(format!("{}.o", obj_name));

        let result = Command::new(&compiler)
            .args(&args)
            .arg(c_file)
            .arg(format!("-o{}", obj_path.display()))
            .output()
            .map_err(|e| format!("failed to run compiler: {}", e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(format!("compiler error: {}", stderr.trim()));
        }
        object_files.push(obj_path);
    }

    // Archive .o files into static library
    let archive_cmd = if cfg!(windows) {
        ("lib.exe", format!("/OUT:{}", output.display()))
    } else {
        ("ar", format!("rcs {}", output.display()))
    };

    let mut cmd = Command::new(&archive_cmd.0);
    if cfg!(windows) {
        cmd.arg(&archive_cmd.1);
        for obj in &object_files {
            cmd.arg(obj);
        }
    } else {
        cmd.arg(&archive_cmd.1);
        for obj in &object_files {
            cmd.arg(obj);
        }
    }

    let result = cmd.output().map_err(|e| format!("failed to run archiver: {}", e))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("archive error: {}", stderr.trim()));
    }

    Ok(format!(
        "compiled {} .c files into {}",
        c_files.len(),
        output.display()
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_mode_display() {
        assert_eq!(ExecutionMode::Vm.to_string(), "vm");
        assert_eq!(ExecutionMode::Jit.to_string(), "jit");
        assert_eq!(ExecutionMode::Aot.to_string(), "aot");
    }

    #[test]
    fn test_execution_mode_from_str() {
        assert_eq!("vm".parse::<ExecutionMode>(), Ok(ExecutionMode::Vm));
        assert_eq!("jit".parse::<ExecutionMode>(), Ok(ExecutionMode::Jit));
        assert_eq!("aot".parse::<ExecutionMode>(), Ok(ExecutionMode::Aot));
        assert!("unknown".parse::<ExecutionMode>().is_err());
    }

    #[test]
    fn test_ffi_mode_display() {
        assert_eq!(FfiMode::Aot.to_string(), "aot");
        assert_eq!(FfiMode::Cffi.to_string(), "cffi");
        assert_eq!(FfiMode::RustFfi.to_string(), "rustffi");
    }

    #[test]
    fn test_ffi_mode_from_str() {
        assert_eq!("aot".parse::<FfiMode>(), Ok(FfiMode::Aot));
        assert_eq!("cffi".parse::<FfiMode>(), Ok(FfiMode::Cffi));
        assert_eq!("rustffi".parse::<FfiMode>(), Ok(FfiMode::RustFfi));
        assert!("unknown".parse::<FfiMode>().is_err());
    }

    #[test]
    fn test_stdlib_index_new() {
        let index = StdlibIndex::new(
            PathBuf::from("target/build/stdlib"),
            ExecutionMode::Vm,
            FfiMode::Aot,
        );
        assert!(index.is_empty());
        assert_eq!(index.execution_mode, ExecutionMode::Vm);
        assert_eq!(index.ffi_mode, FfiMode::Aot);
    }

    #[test]
    fn test_stdlib_index_add_and_find_module() {
        let mut index = StdlibIndex::new(
            PathBuf::from("target/build/stdlib"),
            ExecutionMode::Vm,
            FfiMode::Aot,
        );

        let module = StdlibModule {
            name: "Math".to_string(),
            full_name: "aura.lang.std.Math".to_string(),
            source_path: PathBuf::from("core/aura/lang/std/Math.aura"),
            auc_path: None,
            function_names: vec![
                "abs".to_string(),
                "min".to_string(),
                "max".to_string(),
            ],
            type_names: vec![],
            constant_names: vec![],
            has_extern: false,
        };
        index.add_module(module);

        assert_eq!(index.len(), 1);
        let found = index.find_module("Math").unwrap();
        assert_eq!(found.name, "Math");
        assert_eq!(found.function_names.len(), 3);
    }

    #[test]
    fn test_ffi_index_new_and_add() {
        let mut index = FfiIndex::new(FfiMode::Aot);
        assert!(index.is_empty());

        let decl = FfiDeclaration {
            name: "fopen".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: Some(0x12345678),
        };
        index.add_declaration(decl);

        assert_eq!(index.len(), 1);
        assert_eq!(index.get_function_address("fopen"), Some(0x12345678));
    }

    #[test]
    fn test_ffi_index_set_function_address() {
        let mut index = FfiIndex::new(FfiMode::Aot);

        let decl = FfiDeclaration {
            name: "malloc".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "Builtin".to_string(),
            function_address: None,
        };
        index.add_declaration(decl);

        index.set_function_address("malloc", 0xDEADBEEF);

        assert_eq!(index.get_function_address("malloc"), Some(0xDEADBEEF));
        assert_eq!(index.declarations[0].function_address, Some(0xDEADBEEF));
    }

    #[test]
    fn test_ffi_index_declarations_for_module() {
        let mut index = FfiIndex::new(FfiMode::Aot);

        index.add_declaration(FfiDeclaration {
            name: "fopen".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: None,
        });
        index.add_declaration(FfiDeclaration {
            name: "fclose".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: None,
        });
        index.add_declaration(FfiDeclaration {
            name: "printf".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "IO".to_string(),
            function_address: None,
        });

        let fs_decls = index.declarations_for_module("FileSystem");
        assert_eq!(fs_decls.len(), 2);

        let io_decls = index.declarations_for_module("IO");
        assert_eq!(io_decls.len(), 1);
    }

    #[test]
    fn test_resolve_stdlib_symbol() {
        let mut index = StdlibIndex::new(
            PathBuf::from("target/build/stdlib"),
            ExecutionMode::Vm,
            FfiMode::Aot,
        );

        index.add_module(StdlibModule {
            name: "Math".to_string(),
            full_name: "aura.lang.std.Math".to_string(),
            source_path: PathBuf::from("core/aura/lang/std/Math.aura"),
            auc_path: None,
            function_names: vec![
                "abs".to_string(),
                "min".to_string(),
            ],
            type_names: vec![],
            constant_names: vec!["PI".to_string()],
            has_extern: false,
        });

        assert_eq!(
            resolve_stdlib_symbol(&index, "abs"),
            Some(("Math".to_string(), SymbolKind::Function))
        );
        assert_eq!(
            resolve_stdlib_symbol(&index, "PI"),
            Some(("Math".to_string(), SymbolKind::Constant))
        );
        assert_eq!(resolve_stdlib_symbol(&index, "unknown"), None);
    }

    #[test]
    fn test_link_stdlib_symbols() {
        let mut index = StdlibIndex::new(
            PathBuf::from("target/build/stdlib"),
            ExecutionMode::Vm,
            FfiMode::Aot,
        );

        index.add_module(StdlibModule {
            name: "Math".to_string(),
            full_name: "aura.lang.std.Math".to_string(),
            source_path: PathBuf::from("core/aura/lang/std/Math.aura"),
            auc_path: None,
            function_names: vec![
                "abs".to_string(),
                "min".to_string(),
            ],
            type_names: vec![],
            constant_names: vec![],
            has_extern: false,
        });

        let app_symbols = vec![
            "abs".to_string(),
            "Math.abs".to_string(),
            "unknown".to_string(),
        ];

        let links = link_stdlib_symbols(&index, &app_symbols);

        assert_eq!(links.len(), 2);
        assert!(links.contains_key("abs"));
        assert!(links.contains_key("Math.abs"));
        assert!(!links.contains_key("unknown"));
    }

    #[test]
    fn test_scan_aura_files_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let files = scan_aura_files(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_scan_aura_files_recursive() {
        let tmp = tempfile::tempdir().unwrap();

        let subdir = tmp.path().join("sub");
        std::fs::create_dir_all(&subdir).unwrap();

        std::fs::write(tmp.path().join("a.aura"), "fun main() {}").unwrap();
        std::fs::write(subdir.join("b.aura"), "fun foo() {}").unwrap();
        std::fs::write(tmp.path().join("c.txt"), "not aura").unwrap();

        let files = scan_aura_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("a.aura")));
        assert!(files.iter().any(|f| f.ends_with("b.aura")));
    }

    #[test]
    fn test_generate_stdlib_index_empty() {
        let index = generate_stdlib_index(
            &[],
            PathBuf::from("target/build/stdlib"),
            ExecutionMode::Vm,
            FfiMode::Aot,
        )
        .unwrap();
        assert!(index.is_empty());
    }

    #[test]
    fn test_generate_ffi_index_empty() {
        let index = generate_ffi_index(&[], FfiMode::Aot).unwrap();
        assert!(index.is_empty());
    }

    #[test]
    fn test_generate_ffi_index_with_extern() {
        let tmp = tempfile::tempdir().unwrap();
        let aura_file = tmp.path().join("FileSystem.aura");
        std::fs::write(
            &aura_file,
            r#"
extern "c" "libc" {
    fun fopen(path: String, mode: String): Pointer
    fun fclose(stream: Pointer): Int
}
"#,
        )
        .unwrap();

        let index = generate_ffi_index(&[aura_file.clone()], FfiMode::Aot).unwrap();
        assert!(index.len() >= 2);
    }

    #[test]
    fn test_stdlib_index_json_roundtrip() {
        let index = StdlibIndex {
            modules: vec![
                StdlibModule {
                    name: "Math".to_string(),
                    full_name: "aura.lang.std.Math".to_string(),
                    source_path: PathBuf::from("core/aura/lang/std/Math.aura"),
                    auc_path: Some(PathBuf::from("target/build/Math.auc")),
                    function_names: vec!["abs".to_string()],
                    type_names: vec![],
                    constant_names: vec![],
                    has_extern: false,
                },
            ],
            output_dir: PathBuf::from("target/build/stdlib"),
            execution_mode: ExecutionMode::Vm,
            ffi_mode: FfiMode::Aot,
        };

        let json = index.to_json().unwrap();
        let loaded = StdlibIndex::from_json(&json).unwrap();

        assert_eq!(loaded.modules.len(), 1);
        assert_eq!(loaded.modules[0].name, "Math");
        assert_eq!(loaded.execution_mode, ExecutionMode::Vm);
        assert_eq!(loaded.ffi_mode, FfiMode::Aot);
    }

    #[test]
    fn test_ffi_index_json_roundtrip() {
        let mut index = FfiIndex::new(FfiMode::Aot);
        index.add_declaration(FfiDeclaration {
            name: "fopen".to_string(),
            library: "libc".to_string(),
            language: "c".to_string(),
            module: "FileSystem".to_string(),
            function_address: Some(0x12345678),
        });

        let json = index.to_json().unwrap();
        let loaded = FfiIndex::from_json(&json).unwrap();

        assert_eq!(loaded.declarations.len(), 1);
        assert_eq!(loaded.declarations[0].name, "fopen");
        assert_eq!(loaded.ffi_mode, FfiMode::Aot);
    }

    #[test]
    fn test_extract_library_name() {
        assert_eq!(
            extract_library_name(
                "extern \"c\" \"libc\" fun fopen(path: String, mode: String): Pointer"
            ),
            Some("libc".to_string())
        );
        assert_eq!(
            extract_library_name("extern \"c\" \"mylib\" fun my_func(x: Int): Int"),
            Some("mylib".to_string())
        );
        assert_eq!(
            extract_library_name("extern \"c\" fun simple_func(x: Int): Int"),
            None
        );
    }
}
